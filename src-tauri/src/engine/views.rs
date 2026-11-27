//! Runs as the UI and Tracket see them: the run, its workflow's steps, every job, pending approvals.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::workflow::{Step, Workflow};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TicketRef {
    pub key: String,
    pub title: String,
    pub url: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: String,
    pub step_id: String,
    pub kind: String,
    pub state: String,
    pub attempt: i64,
    pub output: Option<String>,
    pub error: Option<String>,
    pub cost_usd: Option<f64>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub has_session: bool,
    /// Where it ran: `""` for the workflow itself, `each[2]/` for the third item of the loop `each`
    pub scope: String,
    /// Redone since (after a rejection or a retry): kept for the history only
    pub superseded: bool,
    /// The JSON output of a step that asked for one; a loop's items
    pub data: Option<Value>,
    /// For an ask (`kind: ask`): the job that asked, and its question
    pub parent_job: Option<String>,
    pub question: Option<String>,
    /// For a launch step (`kind: launch`): the run it started
    pub launched_run: Option<String>,
    /// What happened to the job, oldest first: started, tool asks and answers, done or failed
    pub log: Vec<LogLine>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub at: String,
    pub what: String,
    pub detail: Option<String>,
}

/// One line of the recent activity, with the run it belongs to.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    pub at: String,
    pub what: String,
    pub detail: Option<String>,
    pub run_id: Option<String>,
    pub ticket_key: Option<String>,
    pub workflow_name: Option<String>,
    pub number: Option<i64>,
}

/// An MCP server as the latest agent session saw it.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct McpServerView {
    pub name: String,
    pub status: String,
    pub source: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct McpServers {
    /// When that session started; None when no agent has run yet
    pub seen_at: Option<String>,
    pub servers: Vec<McpServerView>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalView {
    pub id: String,
    /// `gate` (an approval step) or `tool` (an agent asking before a tool call)
    pub kind: String,
    pub job_id: Option<String>,
    pub summary: String,
    pub tool_name: Option<String>,
    pub input: Option<Value>,
    pub created_at: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub number: i64,
    pub ticket: TicketRef,
    pub folder: String,
    pub started_by: String,
    /// What someone asked for, on a run started from a request
    pub request: Option<String>,
    pub state: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub steps: Vec<Step>,
    /// Whether its workflow has a board
    pub board: bool,
    /// Oldest first; a step done twice (retried, or sent back by a rejection) has two
    pub jobs: Vec<JobView>,
    /// Only the pending ones
    pub approvals: Vec<ApprovalView>,
    /// The run's board, oldest first
    pub notes: Vec<super::board::NoteView>,
    pub cost_usd: f64,
    /// For a run launched by another run's step: that run
    pub parent_run: Option<String>,
}

const RUN_COLUMNS: &str = "id, workflow_id, workflow_name, workflow_json, number, ticket_key, ticket_title, ticket_url,
    folder, started_by, state, started_at, ended_at, request,
    (SELECT j.run_id FROM jobs j WHERE j.id = runs.parent_job)";

fn run_from_row(r: &Row) -> rusqlite::Result<RunView> {
    let wf: Option<Workflow> = serde_json::from_str(&r.get::<_, String>(3)?).ok();
    Ok(RunView {
        id: r.get(0)?,
        workflow_id: r.get(1)?,
        workflow_name: r.get(2)?,
        number: r.get(4)?,
        ticket: TicketRef { key: r.get(5)?, title: r.get(6)?, url: r.get(7)? },
        folder: r.get(8)?,
        started_by: r.get(9)?,
        request: r.get(13)?,
        state: r.get(10)?,
        started_at: r.get(11)?,
        ended_at: r.get(12)?,
        board: wf.as_ref().is_none_or(|w| w.board),
        steps: wf.map(|w| w.steps).unwrap_or_default(),
        jobs: Vec::new(),
        approvals: Vec::new(),
        notes: Vec::new(),
        cost_usd: 0.0,
        parent_run: r.get(14)?,
    })
}

fn fill(conn: &Connection, run: &mut RunView) -> Result<()> {
    let mut jobs = conn.prepare(
        "SELECT id, step_id, kind, state, attempt, output, error, cost_usd, started_at, ended_at, sdk_session_id IS NOT NULL,
         scope, superseded, data, parent_job, question,
         (SELECT c.id FROM runs c WHERE c.parent_job = jobs.id ORDER BY c.rowid DESC LIMIT 1)
         FROM jobs WHERE run_id = ?1 ORDER BY rowid",
    )?;
    run.jobs = jobs
        .query_map([&run.id], |r| {
            Ok(JobView {
                id: r.get(0)?,
                step_id: r.get(1)?,
                kind: r.get(2)?,
                state: r.get(3)?,
                attempt: r.get(4)?,
                output: r.get(5)?,
                error: r.get(6)?,
                cost_usd: r.get(7)?,
                started_at: r.get(8)?,
                ended_at: r.get(9)?,
                has_session: r.get(10)?,
                scope: r.get(11)?,
                superseded: r.get(12)?,
                data: r.get::<_, Option<String>>(13)?.and_then(|d| serde_json::from_str(&d).ok()),
                parent_job: r.get(14)?,
                question: r.get(15)?,
                launched_run: r.get(16)?,
                log: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut log = conn.prepare("SELECT at, what, detail FROM activity WHERE job_id = ?1 ORDER BY id")?;
    for job in &mut run.jobs {
        job.log = log
            .query_map([&job.id], |r| Ok(LogLine { at: r.get(0)?, what: r.get(1)?, detail: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?;
    }
    run.cost_usd = run.jobs.iter().filter_map(|j| j.cost_usd).sum();
    run.notes = super::board::notes(conn, &run.id, None)?;

    let mut approvals = conn.prepare(
        "SELECT id, kind, job_id, summary, tool_name, input, created_at FROM approvals
         WHERE run_id = ?1 AND state = 'pending' ORDER BY created_at",
    )?;
    run.approvals = approvals
        .query_map([&run.id], |r| {
            let input: Option<String> = r.get(5)?;
            Ok(ApprovalView {
                id: r.get(0)?,
                kind: r.get(1)?,
                job_id: r.get(2)?,
                summary: r.get(3)?,
                tool_name: r.get(4)?,
                input: input.and_then(|i| serde_json::from_str(&i).ok()),
                created_at: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(())
}

/// A ticket's latest run, in a few words: what Tracket shows on the ticket's row.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TicketSummary {
    pub ticket_key: String,
    pub run_id: String,
    pub number: i64,
    pub workflow_name: String,
    pub state: String,
    pub started_at: String,
    /// The oldest decision waiting, if any: approving it is what moves the run on
    pub approval_id: Option<String>,
    /// `gate` (a step's approval) or `tool` (an agent asking to use a tool)
    pub approval_kind: Option<String>,
    pub approval_summary: Option<String>,
    /// The step that failed or was interrupted, to retry it
    pub failed_job_id: Option<String>,
    /// How many of its jobs are working now
    pub working: u32,
}

/// The latest run of each ticket that has one, newest first.
pub fn tickets(conn: &Connection) -> Result<Vec<TicketSummary>> {
    let mut stmt = conn.prepare(
        // A ticket's latest run started from outside; the runs its launch steps started count as part of it
        // (`root_run`), so a decision or a failure down the chain shows on the ticket
        "SELECT r.ticket_key, r.id, r.number, r.workflow_name, r.state, r.started_at,
           (SELECT a.id FROM approvals a JOIN runs t ON t.id = a.run_id WHERE (t.id = r.id OR t.root_run = r.id)
              AND a.state = 'pending' ORDER BY a.created_at LIMIT 1),
           (SELECT a.summary FROM approvals a JOIN runs t ON t.id = a.run_id WHERE (t.id = r.id OR t.root_run = r.id)
              AND a.state = 'pending' ORDER BY a.created_at LIMIT 1),
           (SELECT j.id FROM jobs j JOIN runs t ON t.id = j.run_id WHERE (t.id = r.id OR t.root_run = r.id)
              AND j.superseded = 0 AND j.state IN ('failed', 'interrupted', 'rejected') ORDER BY j.rowid DESC LIMIT 1),
           (SELECT a.kind FROM approvals a JOIN runs t ON t.id = a.run_id WHERE (t.id = r.id OR t.root_run = r.id)
              AND a.state = 'pending' ORDER BY a.created_at LIMIT 1),
           (SELECT COUNT(*) FROM jobs j JOIN runs t ON t.id = j.run_id WHERE (t.id = r.id OR t.root_run = r.id)
              AND j.state = 'running' AND j.kind != 'launch')
         FROM runs r
         WHERE r.root_run IS NULL
           AND r.started_at = (SELECT MAX(o.started_at) FROM runs o WHERE o.ticket_key = r.ticket_key AND o.root_run IS NULL)
         ORDER BY r.started_at DESC LIMIT 500",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(TicketSummary {
                ticket_key: r.get(0)?,
                run_id: r.get(1)?,
                number: r.get(2)?,
                workflow_name: r.get(3)?,
                state: r.get(4)?,
                started_at: r.get(5)?,
                approval_id: r.get(6)?,
                approval_kind: r.get(9)?,
                approval_summary: r.get(7)?,
                failed_job_id: r.get(8)?,
                working: r.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// The latest 100 runs, or a ticket's runs, newest first.
pub fn runs(conn: &Connection, ticket: Option<&str>) -> Result<Vec<RunView>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {RUN_COLUMNS} FROM runs WHERE ?1 IS NULL OR ticket_key = ?1 ORDER BY started_at DESC LIMIT 100"
    ))?;
    let mut runs: Vec<RunView> = stmt.query_map(params![ticket], run_from_row)?.collect::<rusqlite::Result<_>>()?;
    for run in &mut runs {
        fill(conn, run)?;
    }
    Ok(runs)
}

pub fn run(conn: &Connection, run_id: &str) -> Result<RunView> {
    let mut run = conn
        .query_row(&format!("SELECT {RUN_COLUMNS} FROM runs WHERE id = ?1"), [run_id], run_from_row)
        .optional()?
        .ok_or_else(|| AppError::new("notFound", "No such run"))?;
    fill(conn, &mut run)?;
    Ok(run)
}

pub fn job_messages(conn: &Connection, job_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare("SELECT body FROM messages WHERE job_id = ?1 ORDER BY seq")?;
    let bodies: Vec<String> = stmt.query_map([job_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(bodies.iter().filter_map(|b| serde_json::from_str(b).ok()).collect())
}

/// The latest activity across runs, newest first.
pub fn activity(conn: &Connection, limit: u32) -> Result<Vec<ActivityView>> {
    let mut stmt = conn.prepare(
        "SELECT a.at, a.what, a.detail, a.run_id, r.ticket_key, r.workflow_name, r.number
         FROM activity a LEFT JOIN runs r ON r.id = a.run_id
         WHERE a.run_id IS NOT NULL ORDER BY a.id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |r| {
            Ok(ActivityView {
                at: r.get(0)?,
                what: r.get(1)?,
                detail: r.get(2)?,
                run_id: r.get(3)?,
                ticket_key: r.get(4)?,
                workflow_name: r.get(5)?,
                number: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Sprocket's own server (the board and ask_agent), which the agent host adds to sessions: not one of the user's
const BOARD_SERVER: &str = "sprocket";

/// The user's MCP servers, as the most recent agent session that loaded them listed them in its `init` message.
/// Sessions of agents with no MCP server load none of the user's (only Sprocket's own, at most), so they're skipped.
pub fn mcp_servers(conn: &Connection) -> Result<McpServers> {
    let mut stmt = conn.prepare(
        "SELECT body, at FROM messages
         WHERE json_extract(body, '$.type') = 'system' AND json_extract(body, '$.subtype') = 'init'
           AND EXISTS (SELECT 1 FROM json_each(body, '$.mcp_servers') WHERE json_extract(value, '$.name') != ?1)
         ORDER BY rowid DESC LIMIT 1",
    )?;
    let latest: Option<(String, Option<String>)> = stmt.query_row([BOARD_SERVER], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((body, at)) = latest else {
        return Ok(McpServers { seen_at: None, servers: Vec::new() });
    };
    let init: Value = serde_json::from_str(&body).unwrap_or_default();
    let servers = init["mcp_servers"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|s| McpServerView {
                    name: s["name"].as_str().unwrap_or_default().to_string(),
                    status: s["status"].as_str().unwrap_or("unknown").to_string(),
                    source: s["source"].as_str().map(str::to_string),
                })
                .filter(|s| !s.name.is_empty() && s.name != BOARD_SERVER)
                .collect()
        })
        .unwrap_or_default();
    Ok(McpServers { seen_at: at, servers })
}
