//! How a run moves on. `drive` walks the workflow's steps and, from the jobs written so far, works out
//! what is done, what is still going, and what starts now: the next agent or approval, a condition's
//! branch, a loop's next items. It runs after every change, so nothing else has to know what comes next.
//!
//! A job is a step done once in one place: the workflow itself (scope `""`), or an item of a loop
//! (`each[2]/`, nested `each[2]/files[0]/`). Jobs redone after a rejection or a retry stay in the history,
//! superseded. Every function works inside the caller's transaction and queues host messages in `Out`,
//! which are sent only once the transaction has committed.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::db::NOW;
use crate::error::{AppError, Result};
use crate::expr;
use crate::host::protocol::{AgentSpec, BoardAccess, CoreMessage, HostMessage};
use crate::workflow::{as_template, ActionKind, ItemFailure, Step, StepKind, Workflow, MAX_ITEMS};

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

pub struct Ended<'a> {
    pub state: &'a str,
    pub output: Option<&'a str>,
    pub error: Option<&'a str>,
    pub cost: Option<f64>,
    pub data: Option<&'a Value>,
}

impl<'a> Ended<'a> {
    pub fn new(state: &'a str) -> Self {
        Self { state, output: None, error: None, cost: None, data: None }
    }
}

pub fn end_job(conn: &Connection, job_id: &str, e: Ended) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE jobs SET state = ?2, output = coalesce(?3, output), error = ?4, cost_usd = coalesce(?5, cost_usd),
             data = coalesce(?6, data), ended_at = {NOW} WHERE id = ?1"
        ),
        params![job_id, e.state, e.output, e.error, e.cost, e.data.map(Value::to_string)],
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

// ---- Jobs and scopes ----

pub struct Job {
    pub id: String,
    pub run_id: String,
    pub step_id: String,
    pub scope: String,
    pub kind: String,
    pub state: String,
    pub output: Option<String>,
    pub data: Option<String>,
    pub session: Option<String>,
    pub superseded: bool,
}

const JOB_COLUMNS: &str = "id, run_id, step_id, scope, kind, state, output, data, sdk_session_id, superseded";

fn job_from_row(r: &rusqlite::Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get(0)?,
        run_id: r.get(1)?,
        step_id: r.get(2)?,
        scope: r.get(3)?,
        kind: r.get(4)?,
        state: r.get(5)?,
        output: r.get(6)?,
        data: r.get(7)?,
        session: r.get(8)?,
        superseded: r.get(9)?,
    })
}

pub fn job(conn: &Connection, job_id: &str) -> Result<Option<Job>> {
    Ok(conn.query_row(&format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?1"), [job_id], job_from_row).optional()?)
}

/// The step's job in that scope that counts: the latest one not superseded.
fn current(conn: &Connection, run_id: &str, scope: &str, step_id: &str) -> Result<Option<Job>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {JOB_COLUMNS} FROM jobs WHERE run_id = ?1 AND scope = ?2 AND step_id = ?3 AND superseded = 0
                 ORDER BY rowid DESC LIMIT 1"
            ),
            params![run_id, scope, step_id],
            job_from_row,
        )
        .optional()?)
}

fn item_scope(scope: &str, loop_id: &str, index: usize) -> String {
    format!("{scope}{loop_id}[{index}]/")
}

/// `each[2]/files[0]/` → `[("each", 2), ("files", 0)]`
fn scope_parts(scope: &str) -> Vec<(&str, usize)> {
    scope
        .split('/')
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            let (id, rest) = p.split_once('[')?;
            Some((id, rest.strip_suffix(']')?.parse().ok()?))
        })
        .collect()
}

/// The first `n` levels of a scope.
fn scope_prefix(scope: &str, n: usize) -> String {
    scope.split_inclusive('/').take(n).collect()
}

/// Whether anything has happened in a loop item yet.
fn started(conn: &Connection, run_id: &str, scope: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM jobs WHERE run_id = ?1 AND superseded = 0 AND substr(scope, 1, length(?2)) = ?2)",
        params![run_id, scope],
        |r| r.get(0),
    )?)
}

/// What a finished job gives to the steps after it: its JSON output, its text, a loop's results.
fn value_of(job: &Job) -> Value {
    if job.state != "done" {
        return Value::Null;
    }
    if job.kind == "loop" {
        return job.output.as_deref().and_then(|o| serde_json::from_str(o).ok()).unwrap_or(Value::Null);
    }
    if let Some(data) = job.data.as_deref().and_then(|d| serde_json::from_str(d).ok()) {
        return data;
    }
    job.output.clone().map(Value::String).unwrap_or(Value::Null)
}

/// The value of a path (`steps.triage.output.files`, `file`) for a step running in `scope`.
fn lookup(conn: &Connection, run: &RunRow, scope: &str, path: &[String]) -> Result<Value> {
    let parts = scope_parts(scope);
    if path[0] == "steps" {
        let (Some(id), Some(place)) = (path.get(1), path.get(1).and_then(|id| run.wf.find(id))) else {
            return Ok(Value::Null);
        };
        // The step is in the loops around this one (checked when the workflow was loaded): same items
        let levels = place.loops.len();
        if levels > parts.len() {
            return Ok(Value::Null);
        }
        let job = current(conn, &run.id, &scope_prefix(scope, levels), id)?;
        return Ok(expr::dig(job.as_ref().map(value_of).unwrap_or(Value::Null), path.get(3..).unwrap_or_default()));
    }
    // A loop's item: the innermost loop with that name
    for level in (0..parts.len()).rev() {
        let (loop_id, index) = parts[level];
        let Some(place) = run.wf.find(loop_id) else { continue };
        if place.step.item.as_deref() != Some(path[0].as_str()) {
            continue;
        }
        let items: Value = current(conn, &run.id, &scope_prefix(scope, level), loop_id)?
            .and_then(|j| j.data)
            .and_then(|d| serde_json::from_str(&d).ok())
            .unwrap_or(Value::Null);
        let item = items.get(index).cloned().unwrap_or(Value::Null);
        return Ok(expr::dig(item, &path[1..]));
    }
    Ok(Value::Null)
}

fn evaluate(conn: &Connection, run: &RunRow, scope: &str, e: &expr::Expr) -> Value {
    e.eval(&|path| {
        lookup(conn, run, scope, path).unwrap_or_else(|err| {
            log::warn!("engine: reading {}: {}", path.join("."), err.message);
            Value::Null
        })
    })
}

/// A text with its `{{ … }}` filled in.
fn render(conn: &Connection, run: &RunRow, scope: &str, text: &str) -> String {
    let template = as_template(text);
    let lookup = |path: &[String]| lookup(conn, run, scope, path).unwrap_or(Value::Null);
    expr::render(&template, &lookup).unwrap_or_else(|_| text.to_string())
}

fn insert_job(conn: &Connection, run_id: &str, scope: &str, step_id: &str, kind: &str, state: &str) -> Result<String> {
    let attempt: i64 = conn.query_row(
        "SELECT count(*) + 1 FROM jobs WHERE run_id = ?1 AND step_id = ?2 AND scope = ?3",
        [run_id, step_id, scope],
        |r| r.get(0),
    )?;
    let id = new_id();
    conn.execute(
        &format!(
            "INSERT INTO jobs (id, run_id, step_id, scope, kind, state, attempt, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, {NOW})"
        ),
        params![id, run_id, step_id, scope, kind, state, attempt],
    )?;
    Ok(id)
}

/// `fix` in a loop item reads `fix (each 3)` in the logs.
pub fn label(step: &str, scope: &str) -> String {
    match scope_parts(scope).last() {
        Some((loop_id, index)) => format!("{step} ({loop_id} {})", index + 1),
        None => step.to_string(),
    }
}

// ---- Starting a step ----

fn agent_spec(wf: &Workflow, agent_id: &str) -> Result<AgentSpec> {
    let a = wf
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| AppError::new("database", format!("The workflow has no agent `{agent_id}`")))?;
    let board = match (wf.board, a.board) {
        (false, _) => BoardAccess::Off,
        (true, false) => BoardAccess::Read,
        (true, true) => BoardAccess::Post,
    };
    Ok(AgentSpec { model: a.model.clone(), ask: a.ask, tools: a.tools.clone(), mcp: a.mcp.clone(), instructions: a.instructions.clone(), board })
}

fn compose_prompt(conn: &Connection, run: &RunRow, step: &Step, scope: &str, feedback: Option<&str>) -> String {
    let mut prompt = format!(
        "Jira ticket {}: {}\n{}\n\nYou are the {} agent of the workflow \"{}\", run by Sprocket. Your step: {}.\n",
        run.ticket_key,
        run.ticket_title,
        run.ticket_url,
        step.agent.as_deref().unwrap_or("assigned"),
        run.wf.name,
        label(&step.id, scope),
    );
    // Only an agent given MCP servers can reach Jira
    let has_mcp = run.wf.agents.iter().any(|a| Some(a.id.as_str()) == step.agent.as_deref() && !a.mcp.is_empty());
    if let Some(action) = &step.action {
        let text = render(conn, run, scope, &action.text);
        let to = action.to.as_deref().map(|t| render(conn, run, scope, t)).unwrap_or_default();
        prompt.push_str(&match action.kind {
            ActionKind::Comment => format!("\nAdd this comment to the Jira ticket {}, exactly as written:\n\n{text}\n", run.ticket_key),
            ActionKind::Status => {
                let note = if text.trim().is_empty() { String::new() } else { format!("\n{text}\n") };
                format!("\nMove the Jira ticket {} to the status \"{to}\".\n{note}", run.ticket_key)
            }
            ActionKind::Slack => format!("\nPost this message to the Slack channel {to}, exactly as written:\n\n{text}\n"),
            ActionKind::Other => format!("\n{text}\n"),
        });
        prompt.push_str("\nDo only this, nothing else, then answer with one line saying what you did.\n");
        return prompt;
    }
    if has_mcp {
        prompt.push_str("Read the ticket (description, comments, linked issues) with your Jira tools when you need its details.\n");
    }
    if run.wf.board {
        let posts = run.wf.agents.iter().find(|a| Some(a.id.as_str()) == step.agent.as_deref()).is_none_or(|a| a.board);
        prompt.push_str(if posts {
            "This run has a board shared by its agents and the people following it. Start by reading it with read_notes: \
             earlier steps and people leave remarks there, and why a result was rejected. Post with post_note what a later \
             step or a person should know that your result doesn't say.\n"
        } else {
            "This run has a board shared by its agents and the people following it. Start by reading it with read_notes: \
             earlier steps and people leave remarks there, and why a result was rejected.\n"
        });
    }
    if let Some(instructions) = &step.instructions {
        prompt.push_str(&format!("\n{}\n", render(conn, run, scope, instructions)));
    }
    if let Some(input) = &step.input {
        prompt.push_str(&format!("\n## Input from earlier steps\n\n{}\n", render(conn, run, scope, input)));
    }
    if let Some(feedback) = feedback {
        prompt.push_str(&format!("\n## Your previous result was rejected\n\n{feedback}\n\nTake this into account.\n"));
    }
    if !step.output.is_empty() {
        let fields: Vec<String> = step.output.iter().map(|f| f.name.clone()).collect();
        prompt.push_str(&format!("\nEnd with your result as the structured output ({}).\n", fields.join(", ")));
    }
    prompt
}

/// Starts an agent, action or approval step in `scope`.
fn start_step(conn: &Connection, run: &RunRow, step: &Step, scope: &str, feedback: Option<String>, session: Option<String>, out: &mut Out) -> Result<Status> {
    let name = label(&step.id, scope);
    if step.kind == StepKind::Approval {
        let job_id = insert_job(conn, &run.id, scope, &step.id, "approval", "waiting")?;
        let summary = step
            .input
            .as_deref()
            .map(|t| render(conn, run, scope, t))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("Approve the step `{name}`"));
        conn.execute(
            &format!("INSERT INTO approvals (id, kind, run_id, job_id, summary, state, created_at) VALUES (?1, 'gate', ?2, ?3, ?4, 'pending', {NOW})"),
            params![new_id(), run.id, job_id, summary],
        )?;
        activity(conn, &run.id, Some(&job_id), "gate.opened", &name)?;
        return Ok(Status::Busy);
    }

    let kind = if step.kind == StepKind::Action { "action" } else { "agent" };
    if !out.ready {
        let job_id = insert_job(conn, &run.id, scope, &step.id, kind, "interrupted")?;
        end_job(conn, &job_id, Ended { error: Some("The agent host isn't running"), ..Ended::new("interrupted") })?;
        activity(conn, &run.id, Some(&job_id), "job.interrupted", "The agent host isn't running")?;
        return Ok(Status::Interrupted);
    }
    let job_id = insert_job(conn, &run.id, scope, &step.id, kind, "running")?;
    let agent = agent_spec(&run.wf, step.agent.as_deref().unwrap_or_default())?;
    let cwd = run.folder.clone();
    let output_schema = step.output_schema();
    activity(conn, &run.id, Some(&job_id), if session.is_some() { "job.resumed" } else { "job.started" }, &name)?;
    out.messages.push(match session {
        Some(sdk_session_id) => CoreMessage::JobResume {
            job_id,
            cwd,
            sdk_session_id,
            prompt: "You were interrupted. Continue your step where you left off.".into(),
            agent,
            output_schema,
        },
        None => CoreMessage::JobStart { job_id, cwd, prompt: compose_prompt(conn, run, step, scope, feedback.as_deref()), agent, output_schema },
    });
    Ok(Status::Busy)
}

// ---- Driving a run ----

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Done,
    /// Something runs, or waits for someone
    Busy,
    Failed,
    Interrupted,
}

fn status_of(state: &str) -> Status {
    match state {
        "done" => Status::Done,
        "running" | "waiting" => Status::Busy,
        "interrupted" => Status::Interrupted,
        _ => Status::Failed,
    }
}

fn block(conn: &Connection, run: &RunRow, steps: &[Step], scope: &str, out: &mut Out) -> Result<Status> {
    for step in steps {
        match node(conn, run, step, scope, out)? {
            Status::Done => continue,
            other => return Ok(other),
        }
    }
    Ok(Status::Done)
}

fn node(conn: &Connection, run: &RunRow, step: &Step, scope: &str, out: &mut Out) -> Result<Status> {
    let job = current(conn, &run.id, scope, &step.id)?;
    match step.kind {
        StepKind::Agent | StepKind::Action | StepKind::Approval => match job {
            None => start_step(conn, run, step, scope, None, None, out),
            Some(j) => Ok(status_of(&j.state)),
        },
        StepKind::Condition => {
            let branch = match job {
                Some(j) if j.state == "done" => j.output.unwrap_or_default(),
                Some(j) => return Ok(status_of(&j.state)),
                None => {
                    let text = step.condition.as_deref().unwrap_or_default();
                    let job_id = insert_job(conn, &run.id, scope, &step.id, "condition", "running")?;
                    match expr::parse(text) {
                        Ok(e) => {
                            let branch = if expr::truthy(&evaluate(conn, run, scope, &e)) { "then" } else { "else" };
                            end_job(conn, &job_id, Ended { output: Some(branch), ..Ended::new("done") })?;
                            activity(conn, &run.id, Some(&job_id), "condition.decided", &format!("{}: {branch}", label(&step.id, scope)))?;
                            branch.to_string()
                        }
                        Err(why) => {
                            end_job(conn, &job_id, Ended { error: Some(&why), ..Ended::new("failed") })?;
                            activity(conn, &run.id, Some(&job_id), "job.failed", &why)?;
                            return Ok(Status::Failed);
                        }
                    }
                }
            };
            block(conn, run, if branch == "then" { &step.then } else { &step.otherwise }, scope, out)
        }
        StepKind::ForEach => for_each(conn, run, step, scope, job, out),
        // No job of its own: its state is its branches'. Every branch is driven, so all of them start
        StepKind::Parallel => {
            let mut statuses = Vec::with_capacity(step.branches.len());
            for lane in &step.branches {
                statuses.push(block(conn, run, lane, scope, out)?);
            }
            Ok([Status::Busy, Status::Interrupted, Status::Failed].into_iter().find(|s| statuses.contains(s)).unwrap_or(Status::Done))
        }
    }
}

/// A loop: its items go through its steps, `parallel` at a time; it ends when they all have.
fn for_each(conn: &Connection, run: &RunRow, step: &Step, scope: &str, job: Option<Job>, out: &mut Out) -> Result<Status> {
    let job = match job {
        Some(j) => j,
        None => {
            let job_id = insert_job(conn, &run.id, scope, &step.id, "loop", "running")?;
            let items = match expr::parse(step.for_each.as_deref().unwrap_or_default()).map(|e| evaluate(conn, run, scope, &e)) {
                Ok(Value::Array(items)) if items.len() <= MAX_ITEMS => Ok(items),
                Ok(Value::Array(items)) => Err(format!("{} items, more than the {MAX_ITEMS} a loop takes", items.len())),
                Ok(Value::Null) => Ok(Vec::new()),
                // One value (text, a number…) is a list of one
                Ok(one) => Ok(vec![one]),
                Err(why) => Err(why),
            };
            match items {
                Ok(items) => {
                    conn.execute("UPDATE jobs SET data = ?2 WHERE id = ?1", params![job_id, Value::Array(items.clone()).to_string()])?;
                    activity(conn, &run.id, Some(&job_id), "loop.started", &format!("{}: {} items", label(&step.id, scope), items.len()))?;
                }
                Err(why) => {
                    end_job(conn, &job_id, Ended { error: Some(&why), ..Ended::new("failed") })?;
                    activity(conn, &run.id, Some(&job_id), "job.failed", &why)?;
                    return Ok(Status::Failed);
                }
            }
            current(conn, &run.id, scope, &step.id)?.ok_or_else(|| AppError::new("database", "The loop's job is gone"))?
        }
    };
    if job.state != "running" {
        return Ok(status_of(&job.state));
    }

    let count = job.data.as_deref().and_then(|d| serde_json::from_str::<Vec<Value>>(d).ok()).map_or(0, |v| v.len());
    let mut statuses: Vec<Option<Status>> = vec![None; count];
    for (i, status) in statuses.iter_mut().enumerate() {
        let s = item_scope(scope, &step.id, i);
        if started(conn, &run.id, &s)? {
            *status = Some(block(conn, run, &step.steps, &s, out)?);
        }
    }
    let busy = |statuses: &[Option<Status>]| statuses.iter().filter(|s| **s == Some(Status::Busy)).count();
    let has = |statuses: &[Option<Status>], what: Status| statuses.contains(&Some(what));
    let stopping = |statuses: &[Option<Status>]| has(statuses, Status::Failed) && step.on_item_failure == ItemFailure::Stop;
    for i in 0..count {
        if statuses[i].is_some() {
            continue;
        }
        if stopping(&statuses) || has(&statuses, Status::Interrupted) || busy(&statuses) >= step.parallel as usize {
            break;
        }
        statuses[i] = Some(block(conn, run, &step.steps, &item_scope(scope, &step.id, i), out)?);
    }

    if busy(&statuses) > 0 {
        return Ok(Status::Busy);
    }
    if has(&statuses, Status::Interrupted) {
        return Ok(Status::Interrupted);
    }
    if stopping(&statuses) {
        let failed = statuses.iter().position(|s| *s == Some(Status::Failed)).unwrap_or(0) + 1;
        let why = format!("Item {failed} failed");
        end_job(conn, &job.id, Ended { error: Some(&why), ..Ended::new("failed") })?;
        activity(conn, &run.id, Some(&job.id), "job.failed", &why)?;
        return Ok(Status::Failed);
    }
    if statuses.iter().any(Option::is_none) {
        return Ok(Status::Busy);
    }
    // Each item's result is what its last step gave
    let mut results = Vec::with_capacity(count);
    for i in 0..count {
        let s = item_scope(scope, &step.id, i);
        let mut value = Value::Null;
        for inner in step.steps.iter().rev().filter(|s| s.kind != StepKind::Condition) {
            if let Some(j) = current(conn, &run.id, &s, &inner.id)? {
                value = value_of(&j);
                break;
            }
        }
        results.push(value);
    }
    let failed = statuses.iter().filter(|s| **s == Some(Status::Failed)).count();
    end_job(conn, &job.id, Ended { output: Some(&Value::Array(results).to_string()), ..Ended::new("done") })?;
    let detail = format!("{}: {count} items{}", label(&step.id, scope), if failed > 0 { format!(", {failed} failed") } else { String::new() });
    activity(conn, &run.id, Some(&job.id), "loop.done", &detail)?;
    Ok(Status::Done)
}

/// Moves the run on as far as it can go, and sets its state from where it got.
pub fn drive(conn: &Connection, run_id: &str, out: &mut Out) -> Result<()> {
    let run = load_run(conn, run_id)?;
    if !is_active(&run.state) {
        return Ok(());
    }
    match block(conn, &run, &run.wf.steps, "", out)? {
        Status::Done => {
            activity(conn, &run.id, None, "run.passed", "")?;
            set_run_state(conn, &run.id, "passed")
        }
        Status::Failed => set_run_state(conn, &run.id, "failed"),
        Status::Interrupted => {
            expire_approvals(conn, &run.id, None, false)?;
            set_run_state(conn, &run.id, "interrupted")
        }
        Status::Busy => refresh_waiting(conn, &run.id),
    }
}

// ---- Going back ----

/// The jobs of these steps, in `scope` and the loop items under it, no longer count.
fn supersede(conn: &Connection, run_id: &str, scope: &str, steps: &[&Step]) -> Result<()> {
    fn ids<'a>(step: &'a Step, out: &mut Vec<&'a str>) {
        out.push(&step.id);
        step.children().for_each(|c| ids(c, out));
    }
    let mut all = Vec::new();
    steps.iter().for_each(|s| ids(s, &mut all));
    for id in all {
        conn.execute(
            "UPDATE jobs SET superseded = 1 WHERE run_id = ?1 AND step_id = ?2 AND superseded = 0 AND substr(scope, 1, length(?3)) = ?3",
            params![run_id, id, scope],
        )?;
    }
    Ok(())
}

/// A rejected gate with `onReject`: the steps from the target to the gate are redone, the target first,
/// told why.
pub fn send_back(conn: &Connection, run: &RunRow, gate: &Job, feedback: String, out: &mut Out) -> Result<bool> {
    let Some(place) = run.wf.find(&gate.step_id) else { return Ok(false) };
    let Some(target) = place.step.on_reject.as_deref() else { return Ok(false) };
    let Some(from) = place.block.iter().position(|s| s.id == target) else { return Ok(false) };
    let redo: Vec<&Step> = place.block[from..=place.index].iter().collect();
    supersede(conn, &run.id, &gate.scope, &redo)?;
    start_step(conn, run, &place.block[from], &gate.scope, Some(feedback), None, out)?;
    Ok(true)
}

/// Runs a failed or interrupted job again, with the other interrupted ones of its run: an agent resumes
/// its session when it has one; a condition or loop is worked out again.
pub fn retry(conn: &Connection, run: &RunRow, job: &Job, out: &mut Out) -> Result<()> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOB_COLUMNS} FROM jobs WHERE run_id = ?1 AND superseded = 0 AND state = 'interrupted' AND id != ?2"
    ))?;
    let others: Vec<Job> = stmt.query_map([&run.id, &job.id], job_from_row)?.collect::<rusqlite::Result<_>>()?;
    set_run_state(conn, &run.id, "running")?;
    for j in std::iter::once(job).chain(&others) {
        let Some(place) = run.wf.find(&j.step_id) else { continue };
        activity(conn, &run.id, Some(&j.id), "job.retried", &label(&j.step_id, &j.scope))?;
        let what: Vec<&Step> = if j.kind == "loop" { vec![place.step] } else { Vec::new() };
        supersede(conn, &run.id, &j.scope, &what)?;
        conn.execute("UPDATE jobs SET superseded = 1 WHERE id = ?1", [&j.id])?;
        // The loops around it, failed because of it, go on again
        let parts = scope_parts(&j.scope);
        for (level, (loop_id, _)) in parts.iter().enumerate() {
            conn.execute(
                "UPDATE jobs SET state = 'running', error = NULL, ended_at = NULL
                 WHERE run_id = ?1 AND scope = ?2 AND step_id = ?3 AND superseded = 0 AND state = 'failed'",
                params![run.id, scope_prefix(&j.scope, level), loop_id],
            )?;
        }
        if matches!(j.kind.as_str(), "agent" | "action" | "approval") {
            start_step(conn, run, place.step, &j.scope, None, j.session.clone(), out)?;
        }
    }
    drive(conn, &run.id, out)
}

// ---- What the host says ----

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
            let Some(j) = job(conn, &job_id)? else { return Ok(()) };
            if !is_active(&j.state) {
                return Ok(());
            }
            conn.execute(
                &format!(
                    "INSERT INTO approvals (id, kind, run_id, job_id, summary, state, ask_id, tool_name, input, created_at)
                     VALUES (?1, 'tool', ?2, ?3, ?4, 'pending', ?5, ?4, ?6, {NOW})"
                ),
                params![new_id(), j.run_id, job_id, tool_name, ask_id, input.to_string()],
            )?;
            conn.execute("UPDATE jobs SET state = 'waiting' WHERE id = ?1", [&job_id])?;
            activity(conn, &j.run_id, Some(&job_id), "tool.asked", &tool_name)?;
            refresh_waiting(conn, &j.run_id)?;
        }
        HostMessage::JobEnded { job_id, result, output, cost_usd, error, data } => {
            let Some(j) = job(conn, &job_id)? else { return Ok(()) };
            if !is_active(&j.state) {
                // Stopped by a cancel, or already interrupted: keep that, but record what it cost
                conn.execute("UPDATE jobs SET cost_usd = coalesce(?2, cost_usd) WHERE id = ?1", params![job_id, cost_usd])?;
                return Ok(());
            }
            expire_approvals(conn, &j.run_id, Some(&job_id), false)?;
            let name = label(&j.step_id, &j.scope);
            if result == "success" {
                // A JSON output may come with no text: show the JSON
                let text = output.filter(|o| !o.trim().is_empty()).or_else(|| data.as_ref().map(expr::to_text));
                end_job(conn, &job_id, Ended { output: text.as_deref(), cost: cost_usd, data: data.as_ref(), ..Ended::new("done") })?;
                activity(conn, &j.run_id, Some(&job_id), "job.done", &name)?;
            } else {
                let why = error.unwrap_or(result);
                end_job(conn, &job_id, Ended { error: Some(&why), cost: cost_usd, ..Ended::new("failed") })?;
                activity(conn, &j.run_id, Some(&job_id), "job.failed", &why)?;
            }
            drive(conn, &j.run_id, out)?;
        }
        HostMessage::BoardPost { request_id, job_id, message, to } => {
            super::board::on_post(conn, request_id, &job_id, &message, to.as_deref(), out)?;
        }
        HostMessage::BoardRead { request_id, job_id, since } => {
            super::board::on_read(conn, request_id, &job_id, since, out)?;
        }
        // Handled by the host supervisor
        HostMessage::Ready { .. } | HostMessage::Error { .. } => {}
    }
    Ok(())
}

/// Agent jobs that were running are gone: they become Interrupted, and so do their runs.
/// Gates are kept: nothing was holding them open.
pub fn interrupt_all(conn: &Connection, why: &str) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id, run_id FROM jobs WHERE kind IN ('agent', 'action') AND state IN ('running', 'waiting')")?;
    let jobs: Vec<(String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for (job_id, run_id) in &jobs {
        activity(conn, run_id, Some(job_id), "job.interrupted", why)?;
    }
    let mut runs: Vec<String> = jobs.into_iter().map(|(_, run_id)| run_id).collect();
    runs.sort();
    runs.dedup();
    conn.execute(
        &format!(
            "UPDATE jobs SET state = 'interrupted', error = ?1, ended_at = {NOW}
             WHERE kind IN ('agent', 'action') AND state IN ('running', 'waiting')"
        ),
        [why],
    )?;
    for run_id in runs {
        expire_approvals(conn, &run_id, None, false)?;
        activity(conn, &run_id, None, "run.interrupted", why)?;
        set_run_state(conn, &run_id, "interrupted")?;
    }
    Ok(())
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    #[test]
    fn reads_scopes() {
        assert_eq!(scope_parts("each[2]/files[10]/"), [("each", 2), ("files", 10)]);
        assert!(scope_parts("").is_empty());
        assert_eq!(scope_prefix("each[2]/files[10]/", 1), "each[2]/");
        assert_eq!(scope_prefix("each[2]/", 0), "");
        assert_eq!(item_scope("each[2]/", "fix", 0), "each[2]/fix[0]/");
        assert_eq!(label("fix", "each[2]/"), "fix (each 3)");
    }
}
