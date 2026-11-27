//! The workflow engine. It walks a run through its workflow's steps (see steps.rs for how):
//! - an agent or action step starts a job on the agent host and moves on when the job succeeds;
//! - an approval step opens a gate that waits, for hours if need be, for someone to decide;
//! - a condition takes one branch, a loop takes its items through its steps, a few at a time;
//! - a tool ask (an agent asking before a tool call) waits for an answer that goes back to the host.
//!
//! All state is in SQLite and every change also writes an activity row, so the app can restart
//! (or the host crash) without losing track: gates survive, running jobs come back Interrupted.

mod board;
mod colleagues;
mod team;
mod steps;
pub mod triggers;
mod views;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

use crate::db::{Db, NOW};
use crate::error::{AppError, Result};
use crate::host::protocol::{Behavior, CoreMessage, HostMessage};
use crate::workflow::{self, Workflow};
use steps::{load_run, Ended, Out};
pub use board::NoteView;
pub use views::{ActivityView, McpServers, RunView, TicketRef};

/// The longest request a webhook may send: it becomes part of agents' prompts.
const MAX_HOOK_REQUEST: usize = 20_000;

/// Where the engine's effects go: the agent host, and the UI that shows runs.
pub trait Sink: Send + Sync {
    fn send(&self, message: CoreMessage) -> bool;
    fn host_ready(&self) -> bool;
    /// Runs changed: the UI reloads them
    fn changed(&self);
}

/// What the agent host tells the engine.
pub enum HostEvent {
    Message(HostMessage),
    /// The host process ended: whatever it was running is gone
    Exited,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRun {
    pub workflow: String,
    /// None: a run on the folder, with no Jira ticket
    #[serde(default)]
    pub ticket: Option<TicketRef>,
    /// The ticket's worktree, made by Tracket; the repository, for a run with no ticket
    pub folder: String,
    pub started_by: String,
    /// What someone asked for in words (Tracket's "Ask an agent"); its steps read it as `{{ request }}`
    #[serde(default)]
    pub request: Option<String>,
}

pub struct Engine {
    db: Db,
    sink: Box<dyn Sink>,
    workflows_dir: PathBuf,
}

/// `ABC-123`, the same rule as Tracket's.
fn valid_issue_key(key: &str) -> bool {
    let Some((project, number)) = key.split_once('-') else { return false };
    project.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && project.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

impl Engine {
    pub fn new(db: Db, sink: Box<dyn Sink>, workflows_dir: PathBuf) -> Self {
        Self { db, sink, workflows_dir }
    }

    /// Runs one change in a transaction, then sends what it queued for the host and tells the UI.
    fn change<T>(&self, f: impl FnOnce(&Connection, &mut Out) -> Result<T>) -> Result<T> {
        self.change_and(true, f)
    }

    fn change_and<T>(&self, notify: bool, f: impl FnOnce(&Connection, &mut Out) -> Result<T>) -> Result<T> {
        let (value, prepare) = self.apply(notify, f)?;
        self.prepare_worktrees(prepare);
        Ok(value)
    }

    /// The change itself: the transaction, then the host's messages and the UI.
    fn apply<T>(&self, notify: bool, f: impl FnOnce(&Connection, &mut Out) -> Result<T>) -> Result<(T, Vec<steps::Prepare>)> {
        let mut out =
            Out { ready: self.sink.host_ready(), messages: Vec::new(), workflows_dir: self.workflows_dir.clone(), prepare: Vec::new() };
        let value = {
            let conn = self.db.0.lock().unwrap_or_else(|e| e.into_inner());
            let tx = conn.unchecked_transaction()?;
            let value = f(&tx, &mut out)?;
            tx.commit()?;
            value
        };
        for message in out.messages {
            self.sink.send(message);
        }
        if notify {
            self.sink.changed();
        }
        Ok((value, out.prepare))
    }

    /// Worktrees for launch steps on another ticket: git runs here, outside the transaction, then each step goes on
    /// (which may ask for more).
    fn prepare_worktrees(&self, mut queue: Vec<steps::Prepare>) {
        while !queue.is_empty() {
            let p = queue.remove(0);
            let folder = crate::worktree::ticket_worktree(std::path::Path::new(&p.from), &p.key);
            match self.apply(true, |conn, out| steps::prepared(conn, &p.job_id, folder, out)) {
                Ok(((), more)) => queue.extend(more),
                Err(e) => log::warn!("engine: launching on {}: {}", p.key, e.message),
            }
        }
    }

    /// Starts a workflow on a ticket. `req.folder` comes from the local API and becomes the agents' working
    /// directory: only a git working tree on this computer is accepted, as resolved by `worktree_folder`.
    pub fn start_run(&self, req: StartRun) -> Result<RunView> {
        let wf = self.workflow(&req.workflow)?;
        self.start_workflow(&wf, req.ticket.as_ref(), &req.folder, &req.started_by, req.request.as_deref())
    }

    fn workflow(&self, id: &str) -> Result<Workflow> {
        workflow::load_all(&self.workflows_dir)?
            .workflows
            .into_iter()
            .find(|w| w.id == id)
            .ok_or_else(|| AppError::new("notFound", format!("No workflow `{id}`")))
    }

    /// Run now, from Sprocket: on a repository, with no ticket.
    pub fn run_now(&self, workflow: &str, folder: &str, request: Option<&str>) -> Result<RunView> {
        let wf = self.workflow(workflow)?;
        self.start_workflow(&wf, None, folder, "You, in Sprocket", request)
    }

    /// A webhook call: only a workflow with a webhook trigger, in that trigger's folder (never one the caller names).
    pub fn start_from_webhook(&self, workflow: &str, request: Option<&str>, ticket: Option<&TicketRef>) -> Result<RunView> {
        let wf = self.workflow(workflow)?;
        let trigger = wf
            .triggers
            .iter()
            .find(|t| t.webhook)
            .ok_or_else(|| AppError::new("notFound", format!("{} has no webhook trigger", wf.name)))?;
        if request.is_some_and(|r| r.chars().count() > MAX_HOOK_REQUEST) {
            return Err(AppError::new("invalid", format!("The request is longer than {MAX_HOOK_REQUEST} characters")));
        }
        let folder = trigger.folder.clone().unwrap_or_default();
        let request = request.map(str::to_string).or_else(|| trigger.request.clone());
        self.start_workflow(&wf, ticket, &folder, "Webhook", request.as_deref())
    }

    /// Starts each schedule whose time has come: called every few seconds, with the time now.
    pub fn tick_schedules(&self, now: chrono::DateTime<chrono::Local>) -> Result<()> {
        for wf in workflow::load_all(&self.workflows_dir)?.workflows {
            for t in wf.triggers.iter().filter(|t| t.schedule.is_some()) {
                let Ok(cron) = workflow::cron(t.schedule.as_deref().unwrap_or_default()) else { continue };
                let key = triggers::mark_key(t);
                let last = {
                    let conn = self.db.0.lock().unwrap_or_else(|e| e.into_inner());
                    triggers::last_looked(&conn, &wf.id, &key)?
                };
                let due = triggers::due(&cron, last, now);
                let started = (due == triggers::Due::Start).then(|| {
                    self.start_workflow(&wf, None, t.folder.as_deref().unwrap_or_default(), "Schedule", t.request.as_deref())
                });
                if due == triggers::Due::No && last.is_some() {
                    continue;
                }
                let conn = self.db.0.lock().unwrap_or_else(|e| e.into_inner());
                let result = started.as_ref().map(|r| r.as_ref().map(|run| run.id.as_str()).map_err(|e| e.message.as_str()));
                triggers::mark(&conn, &wf.id, &key, now, result)?;
                drop(conn);
                if let Some(Err(e)) = &started {
                    log::warn!("The schedule of {} started nothing: {}", wf.name, e.message);
                    self.sink.changed();
                }
            }
        }
        Ok(())
    }

    /// The workflow's triggers: when schedules run next, and what each last started.
    pub fn trigger_statuses(&self, workflow: &str) -> Result<Vec<triggers::TriggerStatus>> {
        let wf = self.workflow(workflow)?;
        let conn = self.db.0.lock().unwrap_or_else(|e| e.into_inner());
        triggers::statuses(&conn, &wf, chrono::Local::now())
    }

    /// Starts `wf` on `ticket` (or on the folder alone), in `folder`, which must be a git working tree on this
    /// computer (`worktree_folder`): it becomes the agents' working directory.
    fn start_workflow(&self, wf: &Workflow, ticket: Option<&TicketRef>, folder: &str, started_by: &str, request: Option<&str>) -> Result<RunView> {
        if let Some(t) = ticket {
            if !valid_issue_key(&t.key) {
                return Err(AppError::new("invalid", "The ticket key must look like ABC-123"));
            }
        }
        let folder = worktree_folder(folder)?;
        let request = request.map(str::trim).filter(|r| !r.is_empty()).map(str::to_string);
        if wf.for_requests && request.is_none() {
            return Err(AppError::new("invalid", format!("{} runs what you ask: say what you want done", wf.name)));
        }
        let none = TicketRef { key: String::new(), title: String::new(), url: String::new() };
        let ticket = ticket.unwrap_or(&none);
        let run_id = self.change(|conn, out| {
            let id = steps::create_run(
                conn,
                steps::NewRun {
                    wf,
                    ticket_key: &ticket.key,
                    ticket_title: &ticket.title,
                    ticket_url: &ticket.url,
                    folder: &folder,
                    started_by,
                    request: request.as_deref(),
                    parent_job: None,
                    root_run: None,
                    after_run: None,
                },
            )?;
            steps::drive(conn, &id, out)?;
            Ok(id)
        })?;
        self.run(&run_id)
    }

    /// Approve or reject a pending gate or tool ask.
    pub fn decide(&self, approval_id: &str, approve: bool, reason: Option<String>, by: &str) -> Result<()> {
        self.change(|conn, out| {
            let row: Option<(String, String, Option<String>, Option<String>)> = conn
                .query_row(
                    "SELECT kind, run_id, job_id, ask_id FROM approvals WHERE id = ?1 AND state = 'pending'",
                    [approval_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            let Some((kind, run_id, job_id, ask_id)) = row else {
                return Err(AppError::new("conflict", "That approval was already decided, or has expired"));
            };
            let state = if approve { "approved" } else { "rejected" };
            conn.execute(
                &format!("UPDATE approvals SET state = ?2, reason = ?3, decided_by = ?4, decided_at = {NOW} WHERE id = ?1"),
                params![approval_id, state, reason, by],
            )?;
            steps::activity(conn, &run_id, job_id.as_deref(), &format!("{kind}.{state}"), &match &reason { Some(r) => format!("{by}: {r}"), None => by.to_string() })?;
            let job_id = job_id.unwrap_or_default();

            if kind == "tool" {
                out.messages.push(CoreMessage::ToolAnswer {
                    ask_id: ask_id.unwrap_or_default(),
                    behavior: if approve { Behavior::Allow } else { Behavior::Deny },
                    message: reason,
                });
                conn.execute("UPDATE jobs SET state = 'running' WHERE id = ?1 AND state = 'waiting'", [&job_id])?;
                return steps::refresh_waiting(conn, &run_id);
            }

            // A run stopped elsewhere (an interrupted loop item) can't go on from here: resume it first.
            // Returning the error rolls the whole decision back.
            let run = load_run(conn, &run_id)?;
            if !steps::is_active(&run.state) {
                return Err(AppError::new("conflict", format!("The run is {}: resume it before deciding", run.state)));
            }
            let gate = steps::job(conn, &job_id)?.ok_or_else(|| AppError::new("notFound", "No such job"))?;
            if gate.kind == "team" {
                // A team stopped at a limit: go on, or fail the step
                team::decide_limit(conn, &run, &gate, approve, out)?;
                return steps::drive(conn, &run_id, out);
            }
            if approve {
                steps::end_job(conn, &job_id, Ended { output: reason.as_deref(), ..Ended::new("done") })?;
            } else {
                steps::end_job(conn, &job_id, Ended { error: reason.as_deref(), ..Ended::new("rejected") })?;
                board::note_rejection(conn, &run, &gate, by, reason.as_deref(), out)?;
                let feedback = reason.unwrap_or_else(|| "Rejected without a reason".into());
                steps::send_back(conn, &run, &gate, feedback, out)?;
            }
            steps::drive(conn, &run_id, out)
        })
    }

    /// A person's note on a run's board, for everyone or for someone (`to`: a step or an agent type).
    pub fn post_note(&self, run_id: &str, by: &str, body: &str, to: Option<&str>) -> Result<NoteView> {
        self.change(|conn, out| {
            board::person_may_post(conn, run_id)?;
            let id = board::post(conn, run_id, board::NewNote { job_id: None, author: by, agent: None, to, body }, out)?;
            Ok(board::notes(conn, run_id, Some(id - 1))?.remove(0))
        })
    }

    pub fn cancel_run(&self, run_id: &str, by: &str) -> Result<()> {
        self.change(|conn, out| {
            let run = load_run(conn, run_id)?;
            // A failed run can be closed too, so it no longer shows as failed in Tracket
            if !steps::is_active(&run.state) && !matches!(run.state.as_str(), "interrupted" | "failed") {
                return Err(AppError::new("conflict", format!("The run is already {}", run.state)));
            }
            // The runs its launch steps started stop with it
            steps::cancel(conn, run_id, by, out)?;
            // Their workflows have room again
            steps::wake_queued(conn, out)
        })
    }

    /// Runs a failed or interrupted step again. An agent resumes its session when it has one.
    pub fn retry_job(&self, job_id: &str) -> Result<()> {
        self.change(|conn, out| {
            let job = steps::job(conn, job_id)?.ok_or_else(|| AppError::new("notFound", "No such job"))?;
            let run = load_run(conn, &job.run_id)?;
            let retryable = matches!(job.state.as_str(), "failed" | "interrupted" | "rejected") && !job.superseded;
            if !retryable || !matches!(run.state.as_str(), "failed" | "interrupted") {
                return Err(AppError::new("conflict", "Only a failed or interrupted step of a stopped run can be retried"));
            }
            steps::retry(conn, &run, &job, out)
        })
    }

    pub fn on_host(&self, event: HostEvent) {
        // An agent's messages come by the dozen: the transcript polls for them instead of every run reloading
        let notify = !matches!(event, HostEvent::Message(HostMessage::JobMessage { .. }));
        let result = self.change_and(notify, |conn, out| match event {
            HostEvent::Message(message) => steps::on_host_message(conn, message, out),
            HostEvent::Exited => steps::interrupt_all(conn, "The agent host stopped"),
        });
        if let Err(e) = result {
            log::warn!("engine: {}", e.message);
        }
    }

    /// At start: nothing is running yet, so whatever was running before is Interrupted.
    pub fn recover(&self) -> Result<()> {
        self.change(|conn, _| steps::interrupt_all(conn, "Sprocket was closed while it ran"))
    }

    pub fn runs(&self, ticket: Option<&str>) -> Result<Vec<RunView>> {
        let conn = self.db.0.lock().unwrap_or_else(|e| e.into_inner());
        views::runs(&conn, ticket)
    }

    /// The latest run of each ticket, in a few words.
    pub fn tickets(&self) -> Result<Vec<views::TicketSummary>> {
        let conn = self.db.0.lock().unwrap_or_else(|e| e.into_inner());
        views::tickets(&conn)
    }

    pub fn run(&self, run_id: &str) -> Result<RunView> {
        let conn = self.db.0.lock().unwrap_or_else(|e| e.into_inner());
        views::run(&conn, run_id)
    }

    pub fn activity(&self, limit: u32) -> Result<Vec<ActivityView>> {
        let conn = self.db.0.lock().unwrap_or_else(|e| e.into_inner());
        views::activity(&conn, limit)
    }

    pub fn mcp_servers(&self) -> Result<McpServers> {
        let conn = self.db.0.lock().unwrap_or_else(|e| e.into_inner());
        views::mcp_servers(&conn)
    }

    /// The agent session's messages, as the SDK sent them.
    pub fn job_messages(&self, job_id: &str) -> Result<Vec<serde_json::Value>> {
        let conn = self.db.0.lock().unwrap_or_else(|e| e.into_inner());
        views::job_messages(&conn, job_id)
    }
}

/// The folder agents may work in, as `start_run` gets it from the local API: an absolute path on this computer
/// to the top of a git working tree (Tracket's ticket worktrees are). It is resolved first, so `..` and links
/// can't lead out of it, and the resolved path is what is kept. A relative path, a network share, or any other
/// folder (the home folder, `C:\Windows`…) is refused.
pub fn worktree_folder(input: &str) -> Result<String> {
    let invalid = |why: &str| AppError::new("invalid", format!("The worktree folder {why}"));
    let given = std::path::Path::new(input.trim());
    if !given.is_absolute() {
        return Err(invalid("must be a full path"));
    }
    let resolved = std::fs::canonicalize(given).map_err(|_| invalid("doesn't exist"))?;
    if !resolved.is_dir() {
        return Err(invalid("isn't a folder"));
    }
    // Windows resolves a local drive to `\\?\C:\…` and a share to `\\?\UNC\…`
    let text = resolved.to_string_lossy().to_string();
    let local = match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        Some(_) => return Err(invalid("must be on this computer")),
        None if text.starts_with(r"\\") => return Err(invalid("must be on this computer")),
        None => text,
    };
    // A repository has a `.git` folder, a linked worktree a `.git` file
    if !resolved.join(".git").exists() {
        return Err(invalid("must be the top of a git working tree"));
    }
    Ok(local)
}
