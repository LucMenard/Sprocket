//! The run's board: notes shared by the run's agents and the people following it. Agents post and read
//! through the agent host's `sprocket` tools; people through the app or Tracket; a rejection's reason is
//! posted too, so later steps know why the run went back.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::db::NOW;
use crate::error::{AppError, Result};
use crate::host::protocol::CoreMessage;

use super::steps::{self, Job, Out, RunRow};

/// Long enough for a real remark, short enough to keep the board readable.
pub const MAX_NOTE: usize = 4000;

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NoteView {
    pub id: i64,
    pub at: String,
    /// The step that posted it (`fix (each 2)`), or the person
    pub author: String,
    /// The agent type, for a note an agent posted; none for a person
    pub agent: Option<String>,
    pub job_id: Option<String>,
    /// Who it is for: a step or an agent type; everyone when none
    pub to: Option<String>,
    pub body: String,
}

pub struct NewNote<'a> {
    pub job_id: Option<&'a str>,
    pub author: &'a str,
    pub agent: Option<&'a str>,
    pub to: Option<&'a str>,
    pub body: &'a str,
}

fn check(body: &str) -> Result<&str> {
    let body = body.trim();
    if body.is_empty() {
        return Err(AppError::new("invalid", "The note is empty"));
    }
    if body.chars().count() > MAX_NOTE {
        return Err(AppError::new("invalid", format!("A note is at most {MAX_NOTE} characters")));
    }
    Ok(body)
}

pub fn post(conn: &Connection, run_id: &str, note: NewNote) -> Result<i64> {
    let body = check(note.body)?;
    let to = note.to.map(str::trim).filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case("everyone"));
    conn.execute(
        &format!("INSERT INTO notes (run_id, job_id, author, agent, addressee, body, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, {NOW})"),
        params![run_id, note.job_id, note.author, note.agent, to, body],
    )?;
    let id = conn.last_insert_rowid();
    steps::activity(conn, run_id, note.job_id, "note.posted", &format!("{}{}", note.author, to.map(|t| format!(" → {t}")).unwrap_or_default()))?;
    Ok(id)
}

/// The run's notes, oldest first, after `since` when given.
pub fn notes(conn: &Connection, run_id: &str, since: Option<i64>) -> Result<Vec<NoteView>> {
    let mut stmt = conn.prepare(
        "SELECT id, at, author, agent, job_id, addressee, body FROM notes WHERE run_id = ?1 AND id > ?2 ORDER BY id",
    )?;
    let rows = stmt
        .query_map(params![run_id, since.unwrap_or(0)], |r| {
            Ok(NoteView { id: r.get(0)?, at: r.get(1)?, author: r.get(2)?, agent: r.get(3)?, job_id: r.get(4)?, to: r.get(5)?, body: r.get(6)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// The board as an agent reads it. Notes for its step or its agent type say so.
fn as_text(notes: &[NoteView], me: &[&str]) -> String {
    if notes.is_empty() {
        return "The board has no notes yet.".into();
    }
    notes
        .iter()
        .map(|n| {
            let who = match &n.agent {
                Some(agent) => format!("{} ({agent})", n.author),
                None => format!("{} (a person)", n.author),
            };
            let to = match &n.to {
                Some(t) if me.iter().any(|m| m.eq_ignore_ascii_case(t)) => format!(" → {t}, that is you"),
                Some(t) => format!(" → {t}"),
                None => String::new(),
            };
            let time = n.at.get(11..16).unwrap_or(&n.at);
            format!("#{} · {time} UTC · {who}{to}\n{}", n.id, n.body)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The agent behind a job, and whether it may use the board. The run must still be going.
fn asker(conn: &Connection, job_id: &str) -> Result<(Job, RunRow, Option<String>)> {
    let job = steps::job(conn, job_id)?.ok_or_else(|| AppError::new("notFound", "No such job"))?;
    if !steps::is_active(&job.state) {
        return Err(AppError::new("conflict", "This step has ended"));
    }
    let run = steps::load_run(conn, &job.run_id)?;
    if !run.wf.board {
        return Err(AppError::new("conflict", "This workflow has no board"));
    }
    let agent = super::colleagues::agent_of(&run, &job);
    Ok((job, run, agent))
}

pub fn on_post(conn: &Connection, request_id: String, job_id: &str, message: &str, to: Option<&str>, out: &mut Out) -> Result<()> {
    let result = asker(conn, job_id).and_then(|(job, run, agent)| {
        let may = run.wf.agents.iter().find(|a| Some(&a.id) == agent.as_ref()).is_none_or(|a| a.board);
        if !may {
            return Err(AppError::new("conflict", "Your agent type may read the board, not post to it"));
        }
        let author = steps::label(&job.step_id, &job.scope);
        post(conn, &run.id, NewNote { job_id: Some(job_id), author: &author, agent: agent.as_deref(), to, body: message })
    });
    out.messages.push(match result {
        Ok(id) => CoreMessage::BoardAnswer { request_id, text: format!("Posted as note #{id}."), error: false },
        Err(e) => CoreMessage::BoardAnswer { request_id, text: e.message, error: true },
    });
    Ok(())
}

pub fn on_read(conn: &Connection, request_id: String, job_id: &str, since: Option<i64>, out: &mut Out) -> Result<()> {
    let result = asker(conn, job_id).and_then(|(job, run, agent)| {
        let list = notes(conn, &run.id, since)?;
        let mut me = vec![job.step_id.as_str()];
        if let Some(agent) = agent.as_deref() {
            me.push(agent);
        }
        Ok(as_text(&list, &me))
    });
    out.messages.push(match result {
        Ok(text) => CoreMessage::BoardAnswer { request_id, text, error: false },
        Err(e) => CoreMessage::BoardAnswer { request_id, text: e.message, error: true },
    });
    Ok(())
}

/// A rejected gate's reason, for the steps that redo the work and the ones after.
pub fn note_rejection(conn: &Connection, run: &RunRow, gate: &Job, by: &str, reason: Option<&str>) -> Result<()> {
    let Some(reason) = reason.map(str::trim).filter(|r| !r.is_empty()) else { return Ok(()) };
    if !run.wf.board {
        return Ok(());
    }
    let target = run.wf.find(&gate.step_id).and_then(|p| p.step.on_reject.clone());
    let body = format!("Rejected at {}: {reason}", steps::label(&gate.step_id, &gate.scope));
    post(conn, &run.id, NewNote { job_id: Some(&gate.id), author: by, agent: None, to: target.as_deref(), body: &body }).map(|_| ())
}

/// Whether a run exists and has a board, for a person posting to it.
pub fn person_may_post(conn: &Connection, run_id: &str) -> Result<()> {
    let json: Option<String> = conn.query_row("SELECT workflow_json FROM runs WHERE id = ?1", [run_id], |r| r.get(0)).optional()?;
    let json = json.ok_or_else(|| AppError::new("notFound", "No such run"))?;
    let board = serde_json::from_str::<serde_json::Value>(&json).ok().and_then(|w| w.get("board").and_then(|b| b.as_bool())).unwrap_or(true);
    if !board {
        return Err(AppError::new("conflict", "This workflow has no board"));
    }
    Ok(())
}
