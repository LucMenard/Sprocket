//! A team step: several agent types working on one step at the same time, talking on the run's board.
//!
//! The step has a job of its own (`kind: team`), and each member a job under it (`kind: member`, step id
//! `<step>~<agent type>` like an ask, `parent_job` the team's). The lead ends the step with `finish(summary)`,
//! or by ending its turn once nobody else is working; the summary is the step's output. A member hitting a stop
//! rule (turns, budget, time) stops the team and opens an approval on the team's job: approve and the members
//! resume their sessions with the same limits again; reject and the step fails.

use rusqlite::{params, Connection};

use crate::db::NOW;
use crate::error::{AppError, Result};
use crate::host::protocol::{CoreMessage, Limits, TeamRole};
use crate::workflow::{Step, Team};

use super::steps::{self, Ended, Job, Out, RunRow, Status};

/// The step's team settings (checked when the workflow was loaded).
fn team_of<'a>(run: &'a RunRow, step_id: &str) -> Result<(&'a Step, &'a Team)> {
    let step = run.wf.find(step_id).map(|p| p.step).ok_or_else(|| AppError::new("database", format!("No step `{step_id}`")))?;
    let team = step.team.as_ref().ok_or_else(|| AppError::new("database", format!("`{step_id}` isn't a team")))?;
    Ok((step, team))
}

fn member_prompt(conn: &Connection, run: &RunRow, step: &Step, team: &Team, scope: &str, me: &str, feedback: Option<&str>) -> String {
    let others: Vec<&str> = team.members.iter().map(String::as_str).filter(|m| *m != me).collect();
    let mut p = format!(
        "{}\n\nYou are the {me} agent, one of a team ({}) working together on the step {} of the workflow \
         \"{}\", run by Sprocket.\n",
        run.heading(),
        team.members.join(", "),
        steps::label(&step.id, scope),
        run.wf.name,
    );
    p.push_str(&format!("\n## The team's goal\n\n{}\n", steps::render(conn, run, scope, &team.goal)));
    if let Some(request) = &run.request {
        p.push_str(&format!("\n## What was asked\n\n{request}\n"));
    }
    p.push_str(&format!(
        "\n## Working together\n\nTalk with the team on the run's board: read_notes to catch up, post_note to say something to \
         everyone or to one of {} (`to`), and wait_for_note to wait until someone else posts. Start by reading the board.\n",
        others.join(", ")
    ));
    p.push_str(&match team.writer.as_deref() {
        Some(w) if w == me => "You are the only one who changes files; the others read and advise.\n".to_string(),
        Some(w) => format!("{w} is the only one who changes files: you read the code and advise.\n"),
        None => "Nobody changes files in this step: read, discuss and decide.\n".to_string(),
    });
    p.push_str(&if me == team.lead {
        "You lead the team. When the goal is reached, call finish with the step's result (what was decided and done): it \
         ends the step for everyone.\n"
            .to_string()
    } else {
        format!("{} leads the team and ends the step. When you have nothing more to add, end your turn.\n", team.lead)
    });
    if let Some(feedback) = feedback {
        p.push_str(&format!("\n## The team's previous result was rejected\n\n{feedback}\n\nTake this into account.\n"));
    }
    p
}

/// Starts one member: a new job under the team's, a fresh session or a resumed one.
fn start_member(conn: &Connection, run: &RunRow, team_job: &Job, agent: &str, start: MemberStart, out: &mut Out) -> Result<()> {
    let (step, team) = team_of(run, &team_job.step_id)?;
    let step_id = format!("{}~{agent}", step.id);
    let attempt: i64 = conn.query_row("SELECT count(*) + 1 FROM jobs WHERE parent_job = ?1 AND step_id = ?2", params![team_job.id, step_id], |r| r.get(0))?;
    let id = steps::new_id();
    conn.execute(
        &format!(
            "INSERT INTO jobs (id, run_id, step_id, scope, kind, state, attempt, started_at, parent_job)
             VALUES (?1, ?2, ?3, ?4, 'member', 'running', ?5, {NOW}, ?6)"
        ),
        params![id, run.id, step_id, team_job.scope, attempt, team_job.id],
    )?;
    steps::activity(conn, &run.id, Some(&id), "job.started", &format!("{agent} in {}", step.id))?;
    let mut spec = steps::agent_spec(&run.wf, agent)?;
    // Members don't ask colleagues: an ask could change files, past the one writer
    spec.colleagues = Vec::new();
    spec.limits = Some(Limits { max_turns: team.limits.max_turns, budget_usd: team.limits.budget_usd, minutes: team.limits.minutes });
    spec.read_only = team.writer.as_deref() != Some(agent);
    spec.team = Some(TeamRole { lead: agent == team.lead, members: team.members.clone() });
    let cwd = run.folder.clone();
    out.messages.push(match start {
        MemberStart::Fresh(feedback) => CoreMessage::JobStart {
            job_id: id,
            cwd,
            prompt: member_prompt(conn, run, step, team, &team_job.scope, agent, feedback.as_deref()),
            agent: spec,
            output_schema: None,
        },
        MemberStart::Resume { session, prompt } => CoreMessage::JobResume { job_id: id, cwd, sdk_session_id: session, prompt, agent: spec, output_schema: None },
    });
    Ok(())
}

enum MemberStart {
    Fresh(Option<String>),
    Resume { session: String, prompt: String },
}

/// The team step starts: its job, and every member at once.
pub fn start(conn: &Connection, run: &RunRow, step: &Step, scope: &str, feedback: Option<String>, out: &mut Out) -> Result<Status> {
    let team = step.team.as_ref().ok_or_else(|| AppError::new("database", format!("`{}` isn't a team", step.id)))?;
    let state = if out.ready { "running" } else { "interrupted" };
    let attempt: i64 = conn.query_row("SELECT count(*) + 1 FROM jobs WHERE run_id = ?1 AND step_id = ?2 AND scope = ?3", params![run.id, step.id, scope], |r| r.get(0))?;
    let team_id = steps::new_id();
    conn.execute(
        &format!("INSERT INTO jobs (id, run_id, step_id, scope, kind, state, attempt, started_at) VALUES (?1, ?2, ?3, ?4, 'team', ?5, ?6, {NOW})"),
        params![team_id, run.id, step.id, scope, state, attempt],
    )?;
    if !out.ready {
        steps::end_job(conn, &team_id, Ended { error: Some("The agent host isn't running"), ..Ended::new("interrupted") })?;
        steps::activity(conn, &run.id, Some(&team_id), "job.interrupted", "The agent host isn't running")?;
        return Ok(Status::Interrupted);
    }
    steps::activity(conn, &run.id, Some(&team_id), "team.started", &format!("{}: {}", step.id, team.members.join(", ")))?;
    let team_job = steps::job(conn, &team_id)?.ok_or_else(|| AppError::new("database", "The team's job is gone"))?;
    for m in &team.members {
        start_member(conn, run, &team_job, m, MemberStart::Fresh(feedback.clone()), out)?;
    }
    Ok(Status::Busy)
}

/// The members of a team job, latest job per agent type, with their agent type.
fn members(conn: &Connection, team_job: &str) -> Result<Vec<(String, Job)>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM jobs WHERE parent_job = ?1 AND kind = 'member' AND rowid IN
         (SELECT max(rowid) FROM jobs WHERE parent_job = ?1 AND kind = 'member' GROUP BY step_id)",
    )?;
    let ids: Vec<String> = stmt.query_map([team_job], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::new();
    for id in ids {
        if let Some(j) = steps::job(conn, &id)? {
            let agent = j.step_id.split_once('~').map(|(_, a)| a.to_string()).unwrap_or_default();
            out.push((agent, j));
        }
    }
    Ok(out)
}

/// Stops the members still working (but `keep`), and what they were waiting for.
fn stop_members(conn: &Connection, run_id: &str, team_job: &str, keep: Option<&str>, out: &mut Out) -> Result<()> {
    for (_, m) in members(conn, team_job)? {
        if Some(m.id.as_str()) == keep || !steps::is_active(&m.state) {
            continue;
        }
        out.messages.push(CoreMessage::JobStop { job_id: m.id.clone() });
        steps::end_job(conn, &m.id, Ended::new("stopped"))?;
        steps::expire_approvals(conn, run_id, Some(&m.id), false)?;
        conn.execute("DELETE FROM board_waits WHERE job_id = ?1", [&m.id])?;
    }
    Ok(())
}

/// The step is done: its output is the summary; the members still working stop.
fn finish(conn: &Connection, team_job: &Job, summary: &str, keep: Option<&str>, out: &mut Out) -> Result<()> {
    steps::end_job(conn, &team_job.id, Ended { output: Some(summary), ..Ended::new("done") })?;
    steps::activity(conn, &team_job.run_id, Some(&team_job.id), "team.finished", &team_job.step_id)?;
    stop_members(conn, &team_job.run_id, &team_job.id, keep, out)
}

/// `finish(summary)` from the lead.
pub fn on_finish(conn: &Connection, request_id: String, job_id: &str, summary: &str, out: &mut Out) -> Result<()> {
    let result = (|| -> Result<String> {
        let member = steps::job(conn, job_id)?.ok_or_else(|| AppError::new("notFound", "No such job"))?;
        let run = steps::load_run(conn, &member.run_id)?;
        let team_job_id: Option<String> = conn.query_row("SELECT parent_job FROM jobs WHERE id = ?1", [job_id], |r| r.get(0))?;
        let team_job = team_job_id.and_then(|id| steps::job(conn, &id).ok().flatten()).filter(|_| member.kind == "member");
        let Some(team_job) = team_job else { return Err(AppError::new("conflict", "Only a team's lead finishes its step")) };
        let (_, team) = team_of(&run, &team_job.step_id)?;
        if member.step_id.split_once('~').map(|(_, a)| a) != Some(team.lead.as_str()) {
            return Err(AppError::new("conflict", format!("{} leads the team: only it finishes the step", team.lead)));
        }
        if !steps::is_active(&team_job.state) {
            return Err(AppError::new("conflict", "The step has ended already"));
        }
        let summary = summary.trim();
        if summary.is_empty() {
            return Err(AppError::new("conflict", "Say the step's result in the summary"));
        }
        finish(conn, &team_job, summary, Some(job_id), out)?;
        Ok(run.id)
    })();
    match result {
        Ok(run_id) => {
            out.messages.push(CoreMessage::BoardAnswer { request_id, text: "The step is done, with your summary as its result. End your turn now.".into(), error: false });
            steps::drive(conn, &run_id, out)
        }
        Err(e) => {
            out.messages.push(CoreMessage::BoardAnswer { request_id, text: e.message, error: true });
            Ok(())
        }
    }
}

/// Whether a member's failure is one of the stop rules.
fn limit_hit(why: &str) -> Option<&'static str> {
    if why.contains("max_turns") {
        Some("turn limit")
    } else if why.contains("max_budget") {
        Some("budget")
    } else if why.contains("time_limit") {
        Some("time limit")
    } else {
        None
    }
}

/// A member's session ended. Returns whether the run should move on.
pub fn on_member_ended(conn: &Connection, member: &Job, outcome: std::result::Result<Option<&str>, &str>, cost: Option<f64>, out: &mut Out) -> Result<()> {
    let agent = member.step_id.split_once('~').map(|(_, a)| a.to_string()).unwrap_or_default();
    match outcome {
        Ok(text) => steps::end_job(conn, &member.id, Ended { output: text, cost, ..Ended::new("done") })?,
        Err(why) => steps::end_job(conn, &member.id, Ended { error: Some(why), cost, ..Ended::new("failed") })?,
    }
    conn.execute("DELETE FROM board_waits WHERE job_id = ?1", [&member.id])?;
    let parent: Option<String> = conn.query_row("SELECT parent_job FROM jobs WHERE id = ?1", [&member.id], |r| r.get(0))?;
    let Some(team_job) = parent.and_then(|id| steps::job(conn, &id).ok().flatten()) else { return Ok(()) };
    if !steps::is_active(&team_job.state) {
        // The step has ended (finished, stopped): nothing more to do
        return Ok(());
    }
    let run = steps::load_run(conn, &member.run_id)?;
    let (_, team) = team_of(&run, &team_job.step_id)?;
    let still = members(conn, &team_job.id)?.into_iter().filter(|(_, m)| steps::is_active(&m.state)).map(|(a, _)| a).collect::<Vec<_>>();

    match outcome {
        Ok(text) if agent == team.lead => {
            if still.is_empty() {
                // Nobody else is working: the lead's last word is the result
                finish(conn, &team_job, text.unwrap_or("(no summary)"), None, out)?;
            } else {
                // The lead stopped while the others work: back to it, to wait for them and finish
                let session: Option<String> = conn.query_row("SELECT sdk_session_id FROM jobs WHERE id = ?1", [&member.id], |r| r.get(0))?;
                let nudges: i64 = conn.query_row("SELECT count(*) FROM jobs WHERE parent_job = ?1 AND step_id = ?2", params![team_job.id, member.step_id], |r| r.get(0))?;
                match session.filter(|_| nudges < 4) {
                    Some(session) => {
                        let prompt = format!(
                            "Your team is still working ({}). Wait for them with wait_for_note, answer what they need, then call \
                             finish with the step's result when the goal is reached.",
                            still.join(", ")
                        );
                        start_member(conn, &run, &team_job, &agent, MemberStart::Resume { session, prompt }, out)?;
                    }
                    None => finish(conn, &team_job, text.unwrap_or("(no summary)"), None, out)?,
                }
            }
            super::board::wake(conn, &run.id, Some(&format!("{agent} ended its turn")), out)?;
        }
        Ok(_) => super::board::wake(conn, &run.id, Some(&format!("{agent} ended its turn: it has nothing more to add")), out)?,
        Err(why) => {
            stop_members(conn, &run.id, &team_job.id, None, out)?;
            match limit_hit(why) {
                Some(what) => {
                    // Stop rule: ask whether to go on
                    let summary = format!(
                        "The team in {} stopped: {agent} reached its {what}. Approve to let the members go on, each with the same \
                         limits again; reject to fail the step.",
                        team_job.step_id
                    );
                    conn.execute(
                        &format!("INSERT INTO approvals (id, kind, run_id, job_id, summary, state, created_at) VALUES (?1, 'gate', ?2, ?3, ?4, 'pending', {NOW})"),
                        params![steps::new_id(), run.id, team_job.id, summary],
                    )?;
                    conn.execute("UPDATE jobs SET state = 'waiting' WHERE id = ?1", [&team_job.id])?;
                    steps::activity(conn, &run.id, Some(&team_job.id), "team.limit", &format!("{agent}: {what}"))?;
                }
                None => {
                    let reason = format!("{agent} failed: {why}");
                    steps::end_job(conn, &team_job.id, Ended { error: Some(&reason), ..Ended::new("failed") })?;
                    steps::activity(conn, &run.id, Some(&team_job.id), "job.failed", &reason)?;
                }
            }
        }
    }
    Ok(())
}

/// The members that hadn't finished resume their sessions (or start again without one).
pub fn resume(conn: &Connection, run: &RunRow, team_job: &Job, prompt: &str, out: &mut Out) -> Result<()> {
    conn.execute("UPDATE jobs SET state = 'running', error = NULL, ended_at = NULL WHERE id = ?1", [&team_job.id])?;
    let team_job = steps::job(conn, &team_job.id)?.ok_or_else(|| AppError::new("database", "The team's job is gone"))?;
    for (agent, m) in members(conn, &team_job.id)? {
        if m.state == "done" {
            continue;
        }
        let start = match m.session.clone() {
            Some(session) => MemberStart::Resume { session, prompt: prompt.into() },
            None => MemberStart::Fresh(None),
        };
        start_member(conn, run, &team_job, &agent, start, out)?;
    }
    Ok(())
}

/// The approval after a stop rule: go on, or fail the step.
pub fn decide_limit(conn: &Connection, run: &RunRow, team_job: &Job, approve: bool, out: &mut Out) -> Result<()> {
    if approve {
        steps::activity(conn, &run.id, Some(&team_job.id), "team.resumed", &team_job.step_id)?;
        return resume(conn, run, team_job, "You were stopped by a limit and may now go on: pick up where you left off.", out);
    }
    steps::end_job(conn, &team_job.id, Ended { error: Some("Stopped at its limits"), ..Ended::new("failed") })?;
    Ok(())
}
