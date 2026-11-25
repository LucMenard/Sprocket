//! How a run moves from step to step. Every function works inside the caller's transaction and
//! queues host messages in `Out`, which are sent only once the transaction has committed.

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::NOW;
use crate::error::{AppError, Result};
use crate::host::protocol::{AgentSpec, CoreMessage, HostMessage};
use crate::workflow::{StepKind, Workflow};

pub struct Out {
    pub ready: bool,
    pub messages: Vec<CoreMessage>,
}

pub struct RunRow {
    pub id: String,
    pub wf: Workflow,
    pub ticket_key: String,
    pub ticket_title: String,
    pub ticket_url: String,
    pub folder: String,
    pub state: String,
}

pub fn new_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS random source");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn is_active(state: &str) -> bool {
    matches!(state, "running" | "waiting")
}

/// One row of the activity log; `job_id` puts it in that job's log too.
pub fn activity(conn: &Connection, run_id: &str, job_id: Option<&str>, what: &str, detail: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO activity (run_id, job_id, what, detail) VALUES (?1, ?2, ?3, ?4)",
        params![run_id, job_id, what, detail],
    )?;
    Ok(())
}

pub fn load_run(conn: &Connection, run_id: &str) -> Result<RunRow> {
    let row = conn
        .query_row(
            "SELECT workflow_json, ticket_key, ticket_title, ticket_url, folder, state FROM runs WHERE id = ?1",
            [run_id],
            |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::new("notFound", "No such run"))?;
    let wf = serde_json::from_str(&row.0).map_err(|e| AppError::new("database", format!("Run {run_id}: {e}")))?;
    Ok(RunRow { id: run_id.into(), wf, ticket_key: row.1, ticket_title: row.2, ticket_url: row.3, folder: row.4, state: row.5 })
}

pub fn step_index(wf: &Workflow, step_id: &str) -> Result<usize> {
    wf.steps
        .iter()
        .position(|s| s.id == step_id)
        .ok_or_else(|| AppError::new("database", format!("The workflow has no step `{step_id}`")))
}

pub fn set_run_state(conn: &Connection, run_id: &str, state: &str) -> Result<()> {
    let ended = if is_active(state) { "NULL".to_string() } else { NOW.to_string() };
    conn.execute(&format!("UPDATE runs SET state = ?2, ended_at = {ended} WHERE id = ?1"), params![run_id, state])?;
    Ok(())
}

/// Waiting while anything is pending, running otherwise; a finished run stays as it is.
pub fn refresh_waiting(conn: &Connection, run_id: &str) -> Result<()> {
    let state: String = conn.query_row("SELECT state FROM runs WHERE id = ?1", [run_id], |r| r.get(0))?;
    if !is_active(&state) {
        return Ok(());
    }
    let pending: i64 =
        conn.query_row("SELECT count(*) FROM approvals WHERE run_id = ?1 AND state = 'pending'", [run_id], |r| r.get(0))?;
    set_run_state(conn, run_id, if pending > 0 { "waiting" } else { "running" })
}

pub fn end_job(conn: &Connection, job_id: &str, state: &str, output: Option<&str>, error: Option<&str>, cost: Option<f64>) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE jobs SET state = ?2, output = coalesce(?3, output), error = ?4, cost_usd = coalesce(?5, cost_usd),
             ended_at = {NOW} WHERE id = ?1"
        ),
        params![job_id, state, output, error, cost],
    )?;
    Ok(())
}

/// Pending approvals of a run (or of one job) can no longer be answered. Gates too when `gates`.
pub fn expire_approvals(conn: &Connection, run_id: &str, job_id: Option<&str>, gates: bool) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE approvals SET state = 'expired', decided_at = {NOW}
             WHERE run_id = ?1 AND state = 'pending' AND (?2 IS NULL OR job_id = ?2) AND (kind = 'tool' OR ?3)"
        ),
        params![run_id, job_id, gates],
    )?;
    Ok(())
}

/// The value of `steps.<id>.output`: the last successful output of that step in this run.
fn resolve(conn: &Connection, run_id: &str, expr: &str) -> Result<String> {
    let Some(step) = expr.strip_prefix("steps.").and_then(|s| s.strip_suffix(".output")) else {
        return Ok(expr.to_string());
    };
    let output: Option<String> = conn
        .query_row(
            "SELECT output FROM jobs WHERE run_id = ?1 AND step_id = ?2 AND state = 'done' ORDER BY started_at DESC LIMIT 1",
            [run_id, step],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(output.unwrap_or_else(|| format!("(step `{step}` has no output)")))
}

pub fn compose_prompt(run: &RunRow, index: usize, input: Option<&str>, feedback: Option<&str>) -> String {
    let step = &run.wf.steps[index];
    let mut prompt = format!(
        "Jira ticket {}: {}\n{}\n\nYou are the {} agent of the workflow \"{}\", run by Sprocket. Your step: {}.\n",
        run.ticket_key,
        run.ticket_title,
        run.ticket_url,
        step.agent.as_deref().unwrap_or("assigned"),
        run.wf.name,
        step.id,
    );
    // Only an agent given MCP servers can reach Jira
    let has_mcp = run.wf.agents.iter().any(|a| Some(a.id.as_str()) == step.agent.as_deref() && !a.mcp.is_empty());
    if has_mcp {
        prompt.push_str("Read the ticket (description, comments, linked issues) with your Jira tools when you need its details.\n");
    }
    if let Some(instructions) = &step.instructions {
        prompt.push_str(&format!("\n{instructions}\n"));
    }
    if let Some(input) = input {
        prompt.push_str(&format!("\n## Input from earlier steps\n\n{input}\n"));
    }
    if let Some(feedback) = feedback {
        prompt.push_str(&format!("\n## Your previous result was rejected\n\n{feedback}\n\nTake this into account.\n"));
    }
    prompt
}

fn agent_spec(wf: &Workflow, agent_id: &str) -> Result<AgentSpec> {
    let a = wf
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| AppError::new("database", format!("The workflow has no agent `{agent_id}`")))?;
    Ok(AgentSpec { model: a.model.clone(), ask: a.ask, tools: a.tools.clone(), mcp: a.mcp.clone(), instructions: a.instructions.clone() })
}

fn insert_job(conn: &Connection, run_id: &str, step_id: &str, kind: &str, state: &str) -> Result<String> {
    let attempt: i64 = conn.query_row(
        "SELECT count(*) + 1 FROM jobs WHERE run_id = ?1 AND step_id = ?2",
        [run_id, step_id],
        |r| r.get(0),
    )?;
    let id = new_id();
    conn.execute(
        &format!("INSERT INTO jobs (id, run_id, step_id, kind, state, attempt, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, {NOW})"),
        params![id, run_id, step_id, kind, state, attempt],
    )?;
    Ok(id)
}

pub fn start_step(conn: &Connection, run: &RunRow, index: usize, feedback: Option<String>, out: &mut Out) -> Result<()> {
    start(conn, run, index, feedback, None, out)
}

pub fn start_step_resuming(conn: &Connection, run: &RunRow, index: usize, session: Option<String>, out: &mut Out) -> Result<()> {
    start(conn, run, index, None, session, out)
}

fn start(conn: &Connection, run: &RunRow, index: usize, feedback: Option<String>, session: Option<String>, out: &mut Out) -> Result<()> {
    let step = &run.wf.steps[index];
    let input = step.input.as_deref().map(|e| resolve(conn, &run.id, e)).transpose()?;

    if step.kind == StepKind::Approval {
        let job_id = insert_job(conn, &run.id, &step.id, "approval", "waiting")?;
        let summary = input.unwrap_or_else(|| format!("Approve the step `{}`", step.id));
        conn.execute(
            &format!("INSERT INTO approvals (id, kind, run_id, job_id, summary, state, created_at) VALUES (?1, 'gate', ?2, ?3, ?4, 'pending', {NOW})"),
            params![new_id(), run.id, job_id, summary],
        )?;
        activity(conn, &run.id, Some(&job_id), "gate.opened", &step.id)?;
        return set_run_state(conn, &run.id, "waiting");
    }

    if !out.ready {
        let job_id = insert_job(conn, &run.id, &step.id, "agent", "interrupted")?;
        end_job(conn, &job_id, "interrupted", None, Some("The agent host isn't running"), None)?;
        activity(conn, &run.id, Some(&job_id), "job.interrupted", "The agent host isn't running")?;
        return set_run_state(conn, &run.id, "interrupted");
    }
    let job_id = insert_job(conn, &run.id, &step.id, "agent", "running")?;
    let agent = agent_spec(&run.wf, step.agent.as_deref().unwrap_or_default())?;
    let cwd = run.folder.clone();
    activity(conn, &run.id, Some(&job_id), if session.is_some() { "job.resumed" } else { "job.started" }, &step.id)?;
    out.messages.push(match session {
        Some(sdk_session_id) => CoreMessage::JobResume {
            job_id,
            cwd,
            sdk_session_id,
            prompt: "You were interrupted. Continue your step where you left off.".into(),
            agent,
        },
        None => CoreMessage::JobStart { job_id, cwd, prompt: compose_prompt(run, index, input.as_deref(), feedback.as_deref()), agent },
    });
    set_run_state(conn, &run.id, "running")
}

/// The step at `index` is done: start the next one, or finish the run.
pub fn advance(conn: &Connection, run: &RunRow, index: usize, out: &mut Out) -> Result<()> {
    if index + 1 < run.wf.steps.len() {
        return start_step(conn, run, index + 1, None, out);
    }
    activity(conn, &run.id, None, "run.passed", "")?;
    set_run_state(conn, &run.id, "passed")
}

fn job_run(conn: &Connection, job_id: &str) -> Result<Option<(String, String, String)>> {
    Ok(conn
        .query_row("SELECT run_id, step_id, state FROM jobs WHERE id = ?1", [job_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?)
}

pub fn on_host_message(conn: &Connection, message: HostMessage, out: &mut Out) -> Result<()> {
    match message {
        HostMessage::JobStarted { job_id, sdk_session_id } => {
            conn.execute("UPDATE jobs SET sdk_session_id = ?2 WHERE id = ?1", [&job_id, &sdk_session_id])?;
        }
        HostMessage::JobMessage { job_id, message } => {
            conn.execute(
                &format!("INSERT INTO messages (job_id, seq, body, at) SELECT ?1, coalesce(max(seq), 0) + 1, ?2, {NOW} FROM messages WHERE job_id = ?1"),
                params![job_id, message.to_string()],
            )?;
        }
        HostMessage::ToolAsk { ask_id, job_id, tool_name, input } => {
            let Some((run_id, _, state)) = job_run(conn, &job_id)? else { return Ok(()) };
            if !is_active(&state) {
                return Ok(());
            }
            conn.execute(
                &format!(
                    "INSERT INTO approvals (id, kind, run_id, job_id, summary, state, ask_id, tool_name, input, created_at)
                     VALUES (?1, 'tool', ?2, ?3, ?4, 'pending', ?5, ?4, ?6, {NOW})"
                ),
                params![new_id(), run_id, job_id, tool_name, ask_id, input.to_string()],
            )?;
            conn.execute("UPDATE jobs SET state = 'waiting' WHERE id = ?1", [&job_id])?;
            activity(conn, &run_id, Some(&job_id), "tool.asked", &tool_name)?;
            set_run_state(conn, &run_id, "waiting")?;
        }
        HostMessage::JobEnded { job_id, result, output, cost_usd, error } => {
            let Some((run_id, step_id, state)) = job_run(conn, &job_id)? else { return Ok(()) };
            if !is_active(&state) {
                // Stopped by a cancel, or already interrupted: keep that, but record what it cost
                conn.execute("UPDATE jobs SET cost_usd = coalesce(?2, cost_usd) WHERE id = ?1", params![job_id, cost_usd])?;
                return Ok(());
            }
            expire_approvals(conn, &run_id, Some(&job_id), false)?;
            let run = load_run(conn, &run_id)?;
            if result == "success" {
                end_job(conn, &job_id, "done", output.as_deref(), None, cost_usd)?;
                activity(conn, &run_id, Some(&job_id), "job.done", &step_id)?;
                let index = step_index(&run.wf, &step_id)?;
                advance(conn, &run, index, out)?;
            } else {
                let why = error.unwrap_or(result);
                end_job(conn, &job_id, "failed", None, Some(&why), cost_usd)?;
                activity(conn, &run_id, Some(&job_id), "job.failed", &why)?;
                set_run_state(conn, &run_id, "failed")?;
            }
        }
        // Handled by the host supervisor
        HostMessage::Ready { .. } | HostMessage::Error { .. } => {}
    }
    Ok(())
}

/// Agent jobs that were running are gone: they become Interrupted, and so do their runs.
/// Gates are kept: nothing was holding them open.
pub fn interrupt_all(conn: &Connection, why: &str) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id, run_id FROM jobs WHERE kind = 'agent' AND state IN ('running', 'waiting')")?;
    let jobs: Vec<(String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for (job_id, run_id) in &jobs {
        activity(conn, run_id, Some(job_id), "job.interrupted", why)?;
    }
    let mut runs: Vec<String> = jobs.into_iter().map(|(_, run_id)| run_id).collect();
    runs.sort();
    runs.dedup();
    conn.execute(
        &format!("UPDATE jobs SET state = 'interrupted', error = ?1, ended_at = {NOW} WHERE kind = 'agent' AND state IN ('running', 'waiting')"),
        [why],
    )?;
    for run_id in runs {
        expire_approvals(conn, &run_id, None, false)?;
        activity(conn, &run_id, None, "run.interrupted", why)?;
        set_run_state(conn, &run_id, "interrupted")?;
    }
    Ok(())
}
