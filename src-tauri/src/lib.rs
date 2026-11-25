mod agent;
mod api;
mod catalog;
mod commands;
mod db;
mod engine;
mod error;
mod host;
mod token;
mod tray;
mod workflow;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

/// What every part of the core shares.
pub struct Ctx {
    pub host: Arc<host::Host>,
    pub engine: Arc<engine::Engine>,
    pub token: String,
    pub data_dir: PathBuf,
    pub workflows_dir: PathBuf,
    /// Where the local API listens, or why it doesn't; None while it starts
    api: Mutex<Option<Result<String, String>>>,
    /// The latest calls to the local API, newest first, so Integrations can show Tracket is connected
    calls: Mutex<VecDeque<ApiCall>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiCall {
    /// Milliseconds since 1970
    pub at: u64,
    pub method: String,
    pub path: String,
    pub status: u16,
}

const CALLS_KEPT: usize = 20;

impl Ctx {
    pub fn api(&self) -> Option<Result<String, String>> {
        self.api.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_api(&self, state: Result<String, String>) {
        *self.api.lock().unwrap_or_else(|e| e.into_inner()) = Some(state);
    }

    pub fn record_call(&self, call: ApiCall) {
        let mut calls = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        calls.push_front(call);
        calls.truncate(CALLS_KEPT);
    }

    pub fn calls(&self) -> Vec<ApiCall> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
    }
}

/// The engine's effects: jobs go to the agent host, and the UI hears that runs changed.
struct LiveSink {
    host: Arc<host::Host>,
    app: AppHandle,
}

impl engine::Sink for LiveSink {
    fn send(&self, message: host::protocol::CoreMessage) -> bool {
        self.host.send(message)
    }
    fn host_ready(&self) -> bool {
        self.host.is_ready()
    }
    fn changed(&self) {
        let _ = self.app.emit("runs-changed", ());
    }
}

fn start(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db = db::Db::open(&data_dir.join("sprocket.sqlite")).map_err(|e| e.message)?;
    db.activity(None, "app.started", Some(env!("CARGO_PKG_VERSION"))).map_err(|e| e.message)?;
    let token = token::load_or_create().map_err(|e| e.message)?;

    let host = host::Host::new();
    let workflows_dir = data_dir.join("workflows");
    let sink = LiveSink { host: host.clone(), app: app.handle().clone() };
    let engine = Arc::new(engine::Engine::new(db, Box::new(sink), workflows_dir.clone()));
    // Nothing runs yet: whatever was running when the app last stopped is Interrupted
    engine.recover().map_err(|e| e.message)?;
    let handler = engine.clone();
    host.set_handler(Arc::new(move |event| handler.on_host(event)));

    let ctx = Arc::new(Ctx {
        host: host.clone(),
        engine,
        token,
        workflows_dir,
        data_dir,
        api: Mutex::new(None),
        calls: Mutex::new(VecDeque::new()),
    });
    // Makes the folder with the example in it on first start
    let _ = workflow::load_all(&ctx.workflows_dir);
    app.manage(ctx.clone());

    api::serve(ctx);
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let launch = host::find_claude()
        .ok_or_else(|| "Claude Code isn't installed: no claude.exe in ~/.local/bin or on the PATH".to_string())
        .and_then(|claude| host::dev_launch(&repo, &claude));
    host.supervise(launch);

    tray::create(app)?;
    open_view_from_env(app);
    Ok(())
}

/// Debug builds only: `SPROCKET_VIEW=runs` opens on that screen, and `SPROCKET_THEME=light|dark` shows that
/// theme for this session (the saved choice is left alone), to check a screen without clicking.
#[cfg(debug_assertions)]
fn open_view_from_env(app: &tauri::App) {
    let word = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_lowercase()));
    let view = word("SPROCKET_VIEW");
    let theme = word("SPROCKET_THEME").filter(|t| t == "light" || t == "dark");
    if view.is_none() && theme.is_none() {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if let Some(view) = view {
                let _ = window.eval(format!("location.hash = '{view}'"));
            }
            if let Some(theme) = theme {
                let _ = window.eval(format!("document.documentElement.classList.toggle('dark', {})", theme == "dark"));
            }
        });
    }
}

#[cfg(not(debug_assertions))]
fn open_view_from_env(_: &tauri::App) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // A second launch (from Tracket, or the Start menu) shows this one instead
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).build())
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::list_workflows,
            commands::api_token,
            commands::open_data_dir,
            commands::list_runs,
            commands::start_run,
            commands::decide_approval,
            commands::cancel_run,
            commands::retry_job,
            commands::job_messages,
            commands::activity,
            commands::mcp_servers,
            commands::api_calls,
            commands::open_workflow_file,
            commands::list_agents,
            commands::save_workflow,
            commands::create_workflow,
            commands::delete_workflow,
            commands::save_agent,
            commands::delete_agent,
            commands::share_agent,
        ])
        .setup(|app| start(app))
        .on_window_event(|window, event| {
            // Closing the window keeps the app, and its runs, in the tray
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building the app");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(ctx) = app.try_state::<Arc<Ctx>>() {
                ctx.host.shutdown();
            }
        }
    });
}
