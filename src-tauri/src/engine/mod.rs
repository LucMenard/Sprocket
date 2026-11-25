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
mod steps;
mod views;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

use crate::db::{Db, NOW};
use crate::error::{AppError, Result};
use crate::host::protocol::{Behavior, CoreMessage, HostMessage};
use crate::workflow;
use steps::{load_run, Ended, Out};
pub use board::NoteView;
pub use views::{ActivityView, McpServers, RunView, TicketRef};

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
    pub ticket: TicketRef,
    /// The ticket's worktree, made by Tracket
    pub folder: String,
    pub started_by: String,
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
        let mut out = Out { ready: self.sink.host_ready(), messages: Vec::new() };
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
        Ok(value)
    }

    pub fn start_run(&self, req: StartRun) -> Result<RunView> {
        if !valid_issue_key(&req.ticket.key) {
            return Err(AppError::new("invalid", "The ticket key must look like ABC-123"));
        }
        if !std::path::Path::new(&req.folder).is_dir() {
            return Err(AppError::new("invalid", format!("The folder {} doesn't exist", req.folder)));
        }
        let wf = workflow::load_all(&self.workflows_dir)?
            .workflows
            .into_iter()
            .find(|w| w.id == req.workflow)
            .ok_or_else(|| AppError::new("notFound", format!("No workflow `{}`", req.workflow)))?;

        let run_id = self.change(|conn, out| {
            let active: u32 = conn.query_row(
                "SELECT count(*) FROM runs WHERE workflow_id = ?1 AND state IN ('running', 'waiting')",
                [&wf.id],
                |r| r.get(0),
            )?;
            if active >= wf.max_parallel_runs {
                return Err(AppError::new(
                    "busy",
                    format!("{} already has {active} run(s) going, its limit", wf.name),
                ));
            }
            let number: i64 = conn.query_row(
                "SELECT coalesce(max(number), 0) + 1 FROM runs WHERE workflow_id = ?1",
                [&wf.id],
                |r| r.get(0),
            )?;
            let id = steps::new_id();
            let snapshot = serde_json::to_string(&wf).expect("serializable");
            conn.execute(
                &format!(
                    "INSERT INTO runs (id, workflow_id, workflow_name, workflow_json, number, ticket_key, ticket_title,
                     ticket_url, folder, started_by, state, started_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'running', {NOW})"
                ),
                params![id, wf.id, wf.name, snapshot, number, req.ticket.key, req.ticket.title, req.ticket.url,
                    req.folder, req.started_by],
            )?;
            steps::activity(conn, &id, None, "run.started", &format!("{} #{number} by {}", wf.name, req.started_by))?;
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
            if approve {
                steps::end_job(conn, &job_id, Ended { output: reason.as_deref(), ..Ended::new("done") })?;
            } else {
                steps::end_job(conn, &job_id, Ended { error: reason.as_deref(), ..Ended::new("rejected") })?;
                board::note_rejection(conn, &run, &gate, by, reason.as_deref())?;
                let feedback = reason.unwrap_or_else(|| "Rejected without a reason".into());
                steps::send_back(conn, &run, &gate, feedback, out)?;
            }
            steps::drive(conn, &run_id, out)
        })
    }

    /// A person's note on a run's board, for everyone or for someone (`to`: a step or an agent type).
    pub fn post_note(&self, run_id: &str, by: &str, body: &str, to: Option<&str>) -> Result<NoteView> {
        self.change(|conn, _| {
            board::person_may_post(conn, run_id)?;
            let id = board::post(conn, run_id, board::NewNote { job_id: None, author: by, agent: None, to, body })?;
            Ok(board::notes(conn, run_id, Some(id - 1))?.remove(0))
        })
    }

    pub fn cancel_run(&self, run_id: &str, by: &str) -> Result<()> {
        self.change(|conn, out| {
            let run = load_run(conn, run_id)?;
            if !steps::is_active(&run.state) && run.state != "interrupted" {
                return Err(AppError::new("conflict", format!("The run is already {}", run.state)));
            }
            let mut stmt = conn.prepare("SELECT id FROM jobs WHERE run_id = ?1 AND kind IN ('agent', 'action', 'ask') AND state IN ('running', 'waiting')")?;
            let running: Vec<String> = stmt.query_map([run_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            for job_id in running {
                out.messages.push(CoreMessage::JobStop { job_id });
            }
            conn.execute(
                &format!("UPDATE jobs SET state = 'stopped', ended_at = {NOW} WHERE run_id = ?1 AND state IN ('running', 'waiting')"),
                [run_id],
            )?;
            steps::expire_approvals(conn, run_id, None, true)?;
            steps::activity(conn, run_id, None, "run.cancelled", by)?;
            steps::set_run_state(conn, run_id, "cancelled")
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
