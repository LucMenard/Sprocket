//! An agent asking another agent type a question (`ask_agent`). The asked one runs as a short job of its own,
//! under the asking job (`parent_job`), in the same worktree, with the run's board; its answer is the tool's
//! result. It can't ask anyone in turn, and a step asks at most `colleagues.maxAsks` times.
//!
//! An ask's step id is `<asking step>~<agent type>`: it is no step of the workflow, so the scheduler never
//! sees it, and Tracket (which looks jobs up by step) doesn't either.

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::NOW;
use crate::error::{AppError, Result};
use crate::host::protocol::{Colleague, CoreMessage};

use super::steps::{self, Job, Out, RunRow};

/// The agent type doing a job: an ask's is in its step id.
pub fn agent_of(run: &RunRow, job: &Job) -> Option<String> {
    if job.kind == "ask" {
        return job.step_id.split_once('~').map(|(_, agent)| agent.to_string());
    }
    run.wf.find(&job.step_id).and_then(|p| p.step.agent.clone())
}

/// Who an agent may ask: none when its agent type may not, or for an ask (no chains).
pub fn available(run: &RunRow, agent: &str, kind: &str) -> Vec<Colleague> {
    let may = kind != "ask" && run.wf.agents.iter().find(|a| a.id == agent).is_none_or(|a| a.colleagues);
    if !may {
        return Vec::new();
    }
    run.wf
        .colleagues
        .can_ask
        .iter()
        .map(|id| Colleague { id: id.clone(), description: run.wf.agents.iter().find(|a| &a.id == id).and_then(|a| a.description.clone()) })
        .collect()
}

fn ask_prompt(run: &RunRow, asker: &Job, asker_agent: &str, agent: &str, question: &str) -> String {
    let mut p = format!(
        "Jira ticket {}: {}\n{}\n\nYou are the {agent} agent of the workflow \"{}\", run by Sprocket. The {asker_agent} agent, \
         working on the step {}, asks you a question. Answer it, in a few paragraphs at most: it reads your answer and \
         carries on with its step. Look at the code, the ticket or the run's board as you need. Don't change files unless \
         the question asks you to.\n",
        run.ticket_key,
        run.ticket_title,
        run.ticket_url,
        run.wf.name,
        steps::label(&asker.step_id, &asker.scope),
    );
    p.push_str(&format!("\n## The question\n\n{question}\n"));
    p
}

/// `ask_agent` from a job: checks it may, then starts the colleague's job. The answer is sent when it ends.
pub fn on_ask(conn: &Connection, request_id: String, job_id: &str, agent: &str, question: &str, out: &mut Out) -> Result<()> {
    match start(conn, &request_id, job_id, agent, question, out) {
        Ok(()) => Ok(()),
        Err(e) => {
            out.messages.push(CoreMessage::ColleagueAnswer { request_id, text: e.message, error: true });
            Ok(())
        }
    }
}

fn start(conn: &Connection, request_id: &str, job_id: &str, agent: &str, question: &str, out: &mut Out) -> Result<()> {
    let refuse = |why: String| AppError::new("conflict", why);
    let asker = steps::job(conn, job_id)?.ok_or_else(|| AppError::new("notFound", "No such job"))?;
    if !steps::is_active(&asker.state) {
        return Err(refuse("This step has ended".into()));
    }
    let run = steps::load_run(conn, &asker.run_id)?;
    let asker_agent = agent_of(&run, &asker).unwrap_or_default();
    let may_ask = available(&run, &asker_agent, &asker.kind);
    if may_ask.is_empty() {
        return Err(refuse(if asker.kind == "ask" {
            "You were asked a question: answer it yourself, without asking others".into()
        } else {
            "You may not ask other agent types in this workflow".into()
        }));
    }
    if !may_ask.iter().any(|c| c.id == agent) {
        let names: Vec<&str> = may_ask.iter().map(|c| c.id.as_str()).collect();
        return Err(refuse(format!("`{agent}` can't be asked here. You may ask: {}", names.join(", "))));
    }
    let question = question.trim();
    if question.is_empty() {
        return Err(refuse("The question is empty".into()));
    }
    let asked: u32 = conn.query_row("SELECT count(*) FROM jobs WHERE parent_job = ?1", [job_id], |r| r.get(0))?;
    if asked >= run.wf.colleagues.max_asks {
        return Err(refuse(format!("This step has asked {asked} times already, its limit: carry on with what you have")));
    }
    if !out.ready {
        return Err(refuse("The agent host isn't ready".into()));
    }

    let id = steps::new_id();
    conn.execute(
        &format!(
            "INSERT INTO jobs (id, run_id, step_id, scope, kind, state, attempt, started_at, parent_job, question, ask_request)
             VALUES (?1, ?2, ?3, ?4, 'ask', 'running', ?5, {NOW}, ?6, ?7, ?8)"
        ),
        params![id, run.id, format!("{}~{agent}", asker.step_id), asker.scope, asked + 1, job_id, question, request_id],
    )?;
    let short: String = question.chars().take(80).collect();
    steps::activity(conn, &run.id, Some(job_id), "colleague.asked", &format!("{agent}: {short}"))?;
    steps::activity(conn, &run.id, Some(&id), "job.started", &format!("{agent}, asked by {}", steps::label(&asker.step_id, &asker.scope)))?;
    let mut spec = steps::agent_spec(&run.wf, agent)?;
    spec.colleagues = Vec::new();
    out.messages.push(CoreMessage::JobStart {
        job_id: id,
        cwd: run.folder.clone(),
        prompt: ask_prompt(&run, &asker, &asker_agent, agent, question),
        agent: spec,
        output_schema: None,
    });
    Ok(())
}

/// An ask ended: its answer (or why there is none) goes back to the asking agent.
pub fn on_answered(conn: &Connection, ask: &Job, answer: std::result::Result<&str, &str>, out: &mut Out) -> Result<()> {
    let request: Option<String> = conn.query_row("SELECT ask_request FROM jobs WHERE id = ?1", [&ask.id], |r| r.get(0)).optional()?.flatten();
    let parent: Option<String> = conn.query_row("SELECT parent_job FROM jobs WHERE id = ?1", [&ask.id], |r| r.get(0)).optional()?.flatten();
    let agent = ask.step_id.split_once('~').map(|(_, a)| a).unwrap_or("colleague");
    let (text, error, what) = match answer {
        Ok(text) => (text.to_string(), false, "colleague.answered"),
        Err(why) => (format!("{agent} couldn't answer: {why}"), true, "colleague.failed"),
    };
    if let Some(parent) = &parent {
        steps::activity(conn, &ask.run_id, Some(parent), what, agent)?;
    }
    if let Some(request_id) = request {
        out.messages.push(CoreMessage::ColleagueAnswer { request_id, text, error });
    }
    Ok(())
}

/// The asking job ended: its asks still going have nobody to answer to.
pub fn stop_asks_of(conn: &Connection, job_id: &str, out: &mut Out) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id FROM jobs WHERE parent_job = ?1 AND state IN ('running', 'waiting')")?;
    let asks: Vec<String> = stmt.query_map([job_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    for id in asks {
        out.messages.push(CoreMessage::JobStop { job_id: id.clone() });
        steps::end_job(conn, &id, steps::Ended::new("stopped"))?;
    }
    Ok(())
}
