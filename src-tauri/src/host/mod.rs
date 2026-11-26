//! Starts the agent host (Node sidecar), restarts it when it dies, and stops it with the app.
//! The host and the agents it runs are killed as a process tree on quit, so no `claude.exe` is left.

pub mod protocol;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use protocol::{CoreMessage, HostMessage, PROTOCOL};

use crate::engine::HostEvent;

/// Who hears about jobs and tool asks: the engine.
pub type Handler = Arc<dyn Fn(HostEvent) + Send + Sync>;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Waits between restarts; after the last one the host stays failed until the app restarts.
const BACKOFF: [u64; 5] = [1, 2, 5, 10, 30];
/// A host that ran this long counts as healthy: the backoff starts over.
const HEALTHY_AFTER: Duration = Duration::from_secs(60);

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum HostState {
    Starting,
    #[serde(rename_all = "camelCase")]
    Ready { protocol: u32, sdk: String, claude_code: String },
    Failed { message: String },
    Stopped,
}

/// How to run the host: `node host.cjs` in development, the sidecar exe once bundled.
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
}

pub struct Host {
    state: Mutex<HostState>,
    tx: Mutex<Option<mpsc::UnboundedSender<CoreMessage>>>,
    pid: Mutex<Option<u32>>,
    quitting: AtomicBool,
    handler: Mutex<Option<Handler>>,
}

impl Host {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(HostState::Starting),
            tx: Mutex::new(None),
            pid: Mutex::new(None),
            quitting: AtomicBool::new(false),
            handler: Mutex::new(None),
        })
    }

    pub fn state(&self) -> HostState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_state(&self, state: HostState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
    }

    pub fn set_handler(&self, handler: Handler) {
        *self.handler.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
    }

    fn emit(&self, event: HostEvent) {
        let handler = self.handler.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(handler) = handler {
            handler(event);
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state(), HostState::Ready { .. })
    }

    /// Queues a message for the host; false when it isn't running.
    pub fn send(&self, message: CoreMessage) -> bool {
        let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        tx.as_ref().is_some_and(|tx| tx.send(message).is_ok())
    }

    /// Runs the host until the app quits. `launch` is None when Claude Code wasn't found.
    pub fn supervise(self: Arc<Self>, launch: Result<Launch, String>) {
        let launch = match launch {
            Ok(launch) => launch,
            Err(message) => return self.set_state(HostState::Failed { message }),
        };
        tauri::async_runtime::spawn(async move {
            let mut failures = 0usize;
            while !self.quitting.load(Ordering::SeqCst) {
                let started = Instant::now();
                self.set_state(HostState::Starting);
                if let Err(message) = self.run_once(&launch).await {
                    self.set_state(HostState::Failed { message });
                }
                if self.quitting.load(Ordering::SeqCst) {
                    break;
                }
                if started.elapsed() >= HEALTHY_AFTER {
                    failures = 0;
                }
                let Some(wait) = BACKOFF.get(failures) else { break };
                failures += 1;
                if !matches!(self.state(), HostState::Failed { .. }) {
                    self.set_state(HostState::Failed { message: "The agent host stopped; restarting".into() });
                }
                tokio::time::sleep(Duration::from_secs(*wait)).await;
            }
            if self.quitting.load(Ordering::SeqCst) {
                self.set_state(HostState::Stopped);
            }
        });
    }

    async fn run_once(&self, launch: &Launch) -> Result<(), String> {
        let mut child = Command::new(&launch.program)
            .args(&launch.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Couldn't start the agent host ({}): {e}", launch.program.display()))?;
        *self.pid.lock().unwrap_or_else(|e| e.into_inner()) = child.id();

        let (tx, mut rx) = mpsc::unbounded_channel::<CoreMessage>();
        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);

        let mut stdin = child.stdin.take().expect("piped");
        tauri::async_runtime::spawn(async move {
            while let Some(message) = rx.recv().await {
                let Ok(mut line) = serde_json::to_string(&message) else { continue };
                line.push('\n');
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        let stderr = child.stderr.take().expect("piped");
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log::info!(target: "agent-host", "{line}");
            }
        });

        let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match serde_json::from_str::<HostMessage>(&line) {
                Ok(message) => self.on_message(message),
                Err(e) => log::warn!("Unreadable line from the agent host ({e}): {line}"),
            }
        }

        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let status = match child.wait().await {
            Ok(status) => status.to_string(),
            Err(e) => e.to_string(),
        };
        // Not ready any more before the engine hears of it, so nothing new is sent meanwhile
        if !matches!(self.state(), HostState::Failed { .. }) {
            self.set_state(HostState::Failed { message: format!("The agent host exited ({status})") });
        }
        self.emit(HostEvent::Exited);
        match self.state() {
            HostState::Failed { message } => Err(message),
            _ => Err(format!("The agent host exited ({status})")),
        }
    }

    fn on_message(&self, message: HostMessage) {
        match message {
            HostMessage::Ready { protocol, sdk, claude_code } if protocol == PROTOCOL => {
                self.set_state(HostState::Ready { protocol, sdk, claude_code });
            }
            HostMessage::Ready { protocol, .. } => {
                self.set_state(HostState::Failed {
                    message: format!("The agent host speaks protocol {protocol}, this app {PROTOCOL}. Rebuild it."),
                });
                self.kill_tree();
            }
            HostMessage::Error { message } => self.set_state(HostState::Failed { message }),
            other => self.emit(HostEvent::Message(other)),
        }
    }

    fn kill_tree(&self) {
        if let Some(pid) = self.pid.lock().unwrap_or_else(|e| e.into_inner()).take() {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    /// On quit: no restart, and the host with its agents is killed.
    pub fn shutdown(&self) {
        self.quitting.store(true, Ordering::SeqCst);
        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.kill_tree();
    }
}

/// Claude Code as installed by its Windows installer, or found on the PATH. `SPROCKET_CLAUDE_PATH` wins.
pub fn find_claude() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SPROCKET_CLAUDE_PATH").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    let installed = home.map(|h| h.join(".local").join("bin").join("claude.exe"));
    if let Some(path) = installed.filter(|p| p.is_file()) {
        return Some(path);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).map(|dir| dir.join("claude.exe")).find(|p| p.is_file())
    })
}

fn host_args(claude: &Path) -> Vec<String> {
    vec!["--claude".into(), claude.display().to_string(), "--settings".into(), "user,project".into()]
}

/// In development: `node agent-host/dist/host.cjs`, from the repository.
pub fn dev_launch(repo: &Path, claude: &Path) -> Result<Launch, String> {
    let script = repo.join("agent-host").join("dist").join("host.cjs");
    if !script.is_file() {
        return Err(format!("{} is missing: run `npm run host:build`", script.display()));
    }
    let mut args = vec![script.display().to_string()];
    args.extend(host_args(claude));
    Ok(Launch { program: PathBuf::from("node"), args })
}

/// Installed: the agent host exe the installer puts next to the app (`externalBin`, built by `npm run host:sea`).
pub fn bundled_launch(claude: &Path) -> Result<Launch, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let host = exe.with_file_name("sprocket-agent-host.exe");
    if !host.is_file() {
        return Err(format!("{} is missing: reinstall Sprocket", host.display()));
    }
    Ok(Launch { program: host, args: host_args(claude) })
}
