//! What starts a workflow besides Tracket: a schedule, another workflow's run ending, a webhook, and Run now in
//! Sprocket. A schedule or a webhook works in the folder its trigger names, with no Jira ticket.

use chrono::{DateTime, Local};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::steps::{self, activity, NewRun, Out, RunRow};
use crate::error::Result;
use crate::workflow::{self, AfterWhen, Trigger, Workflow};

/// How late a scheduled time may still start a run: past that (Sprocket was closed, or the computer asleep) it is
/// skipped, not caught up on.
pub const GRACE_SECONDS: i64 = 120;

/// The most runs one chain of "after another workflow" may start, one after the other.
const MAX_AFTER_CHAIN: usize = 10;

/// What a schedule does now, given the last time it was looked at.
#[derive(Debug, PartialEq, Eq)]
pub enum Due {
    /// Its time has come: start a run
    Start,
    /// Its time went by too long ago: skip it
    Skip,
    /// Not yet
    No,
}

/// Whether a schedule last looked at `last` is due at `now`. Never looked at: not due, it counts from now.
pub fn due(cron: &croner::Cron, last: Option<DateTime<Local>>, now: DateTime<Local>) -> Due {
    let Some(last) = last else { return Due::No };
    match cron.find_next_occurrence(&last, false) {
        Ok(next) if next <= now => {
            if (now - next).num_seconds() <= GRACE_SECONDS {
                Due::Start
            } else {
                Due::Skip
            }
        }
        _ => Due::No,
    }
}

/// The next times a schedule starts a run, after `from`.
pub fn next_times(cron: &croner::Cron, from: DateTime<Local>, count: usize) -> Vec<DateTime<Local>> {
    let mut times = Vec::new();
    let mut at = from;
    while times.len() < count {
        match cron.find_next_occurrence(&at, false) {
            Ok(next) => {
                times.push(next);
                at = next;
            }
            Err(_) => break,
        }
    }
    times
}

/// Names a trigger in `trigger_marks`: a changed schedule or folder starts afresh.
pub fn mark_key(t: &Trigger) -> String {
    format!("schedule|{}|{}", t.schedule.as_deref().unwrap_or_default(), t.folder.as_deref().unwrap_or_default())
}

pub fn last_looked(conn: &Connection, workflow_id: &str, key: &str) -> Result<Option<DateTime<Local>>> {
    let at: Option<String> =
        conn.query_row("SELECT at FROM trigger_marks WHERE workflow_id = ?1 AND key = ?2", [workflow_id, key], |r| r.get(0)).optional()?;
    Ok(at.and_then(|a| DateTime::parse_from_rfc3339(&a).ok()).map(|a| a.with_timezone(&Local)))
}

/// Records that the schedule was looked at `now`; `started`, when it tried to start a run: the run, or why not.
pub fn mark(conn: &Connection, workflow_id: &str, key: &str, now: DateTime<Local>, started: Option<std::result::Result<&str, &str>>) -> Result<()> {
    let at = now.to_rfc3339();
    match started {
        None => conn.execute(
            "INSERT INTO trigger_marks (workflow_id, key, at) VALUES (?1, ?2, ?3)
             ON CONFLICT (workflow_id, key) DO UPDATE SET at = excluded.at",
            params![workflow_id, key, at],
        )?,
        Some(result) => {
            let (run, error) = match result {
                Ok(run) => (Some(run), None),
                Err(e) => (None, Some(e)),
            };
            conn.execute(
                "INSERT INTO trigger_marks (workflow_id, key, at, run_id, error, tried_at) VALUES (?1, ?2, ?3, ?4, ?5, ?3)
                 ON CONFLICT (workflow_id, key) DO UPDATE SET at = excluded.at, tried_at = excluded.at,
                   run_id = coalesce(excluded.run_id, run_id), error = excluded.error",
                params![workflow_id, key, at, run, error],
            )?
        }
    };
    Ok(())
}

/// A trigger as the editor shows it: when a schedule runs next, and what its last start gave.
#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TriggerStatus {
    /// The next times, for a schedule
    pub next: Vec<String>,
    /// The last time it tried to start a run
    pub tried_at: Option<String>,
    /// The last run it started
    pub run_id: Option<String>,
    pub run_number: Option<i64>,
    pub run_state: Option<String>,
    /// Why its last try started nothing
    pub error: Option<String>,
}

/// Each of the workflow's triggers, in order.
pub fn statuses(conn: &Connection, wf: &Workflow, now: DateTime<Local>) -> Result<Vec<TriggerStatus>> {
    let mut all = Vec::new();
    for t in &wf.triggers {
        let mut status = TriggerStatus::default();
        if let Some(expression) = &t.schedule {
            if let Ok(cron) = workflow::cron(expression) {
                status.next = next_times(&cron, now, 3).iter().map(DateTime::to_rfc3339).collect();
            }
            let row = conn
                .query_row(
                    "SELECT m.tried_at, m.run_id, m.error, r.number, r.state FROM trigger_marks m LEFT JOIN runs r ON r.id = m.run_id
                     WHERE m.workflow_id = ?1 AND m.key = ?2",
                    [&wf.id, &mark_key(t)],
                    |r| {
                        Ok(TriggerStatus {
                            next: Vec::new(),
                            tried_at: r.get(0)?,
                            run_id: r.get(1)?,
                            error: r.get(2)?,
                            run_number: r.get(3)?,
                            run_state: r.get(4)?,
                        })
                    },
                )
                .optional()?;
            if let Some(last) = row {
                status = TriggerStatus { next: status.next, ..last };
            }
        } else {
            // After another workflow, a webhook: their latest run
            let by = if t.webhook { "Webhook".to_string() } else { "After %".to_string() };
            let row: Option<(String, i64, String, String)> = conn
                .query_row(
                    "SELECT id, number, state, started_at FROM runs WHERE workflow_id = ?1 AND started_by LIKE ?2
                     ORDER BY started_at DESC LIMIT 1",
                    [&wf.id, &by],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            if let Some((id, number, state, at)) = row {
                status.run_id = Some(id);
                status.run_number = Some(number);
                status.run_state = Some(state);
                status.tried_at = Some(at);
            }
        }
        all.push(status);
    }
    Ok(all)
}

fn when_matches(when: AfterWhen, state: &str) -> bool {
    match when {
        AfterWhen::Passed => state == "passed",
        AfterWhen::Failed => state == "failed",
        AfterWhen::Ended => state == "passed" || state == "failed",
    }
}

/// The workflows of this run and of the runs that started it, one after the other.
fn after_chain(conn: &Connection, run_id: &str) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    let mut at = Some(run_id.to_string());
    while let Some(id) = at {
        let row: Option<(String, Option<String>)> =
            conn.query_row("SELECT workflow_id, after_run FROM runs WHERE id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        let Some((workflow_id, after)) = row else { break };
        ids.push(workflow_id);
        at = after.filter(|_| ids.len() <= MAX_AFTER_CHAIN);
    }
    Ok(ids)
}

/// A run ended as `state`: starts the workflows set to run after it, on its ticket and in its folder. Only a run
/// started from outside counts, not one a launch step started.
pub fn after_ended(conn: &Connection, run: &RunRow, state: &str, out: &mut Out) -> Result<()> {
    let parent: Option<String> = conn.query_row("SELECT parent_job FROM runs WHERE id = ?1", [&run.id], |r| r.get(0))?;
    if parent.is_some() || (state != "passed" && state != "failed") {
        return Ok(());
    }
    let Ok(list) = workflow::load_all(&out.workflows_dir) else { return Ok(()) };
    let followers: Vec<(Workflow, Trigger)> = list
        .workflows
        .into_iter()
        .flat_map(|wf| {
            let triggers: Vec<Trigger> = wf.triggers.iter().filter(|t| t.after.as_deref() == Some(run.wf.id.as_str())).cloned().collect();
            triggers.into_iter().map(move |t| (wf.clone(), t))
        })
        .filter(|(_, t)| when_matches(t.when.unwrap_or_default(), state))
        .collect();
    if followers.is_empty() {
        return Ok(());
    }
    let chain = after_chain(conn, &run.id)?;
    for (wf, t) in followers {
        // Once per run: a retried run that ends again doesn't start its followers again
        let started: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM runs WHERE after_run = ?1 AND workflow_id = ?2)",
            [&run.id, &wf.id],
            |r| r.get(0),
        )?;
        if started {
            continue;
        }
        let skip = |why: String| activity(conn, &run.id, None, "trigger.skipped", &format!("{}: {why}", wf.name));
        if chain.contains(&wf.id) || chain.len() >= MAX_AFTER_CHAIN {
            skip("it already ran in this chain, so it would start a loop".into())?;
            continue;
        }
        let request = t.request.clone().or_else(|| run.request.clone());
        if wf.for_requests && request.is_none() {
            skip("it runs what it is asked, and nothing was".into())?;
            continue;
        }
        let by = format!("After {} #{}", run.wf.name, run.number);
        let created = steps::create_run(
            conn,
            NewRun {
                wf: &wf,
                ticket_key: &run.ticket_key,
                ticket_title: &run.ticket_title,
                ticket_url: &run.ticket_url,
                folder: &run.folder,
                started_by: &by,
                request: request.as_deref(),
                parent_job: None,
                root_run: None,
                after_run: Some(&run.id),
            },
        );
        match created {
            Ok(id) => {
                let number: i64 = conn.query_row("SELECT number FROM runs WHERE id = ?1", [&id], |r| r.get(0))?;
                activity(conn, &run.id, None, "trigger.started", &format!("{} #{number}", wf.name))?;
                steps::drive(conn, &id, out)?;
            }
            Err(e) if e.kind == "busy" => skip(e.message)?,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32, s: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 27, h, m, s).unwrap()
    }

    #[test]
    fn a_schedule_is_due_once_and_skips_times_long_gone() {
        let nine = workflow::cron("0 9 * * *").unwrap();
        // Never looked at: it counts from now
        assert_eq!(due(&nine, None, at(9, 0, 0)), Due::No);
        assert_eq!(due(&nine, Some(at(8, 59, 30)), at(8, 59, 50)), Due::No);
        assert_eq!(due(&nine, Some(at(8, 59, 50)), at(9, 0, 10)), Due::Start);
        // Looked at again just after: done for today
        assert_eq!(due(&nine, Some(at(9, 0, 10)), at(9, 0, 30)), Due::No);
        // Sprocket was closed at nine: skipped, not caught up on
        assert_eq!(due(&nine, Some(at(8, 0, 0)), at(11, 0, 0)), Due::Skip);
        assert_eq!(due(&nine, Some(at(8, 0, 0)), at(9, 1, 30)), Due::Start);
    }

    #[test]
    fn the_next_times_of_a_schedule() {
        let weekdays = workflow::cron("30 8 * * 1-5").unwrap();
        // 2026-11-27 is a Friday
        let next = next_times(&weekdays, at(12, 0, 0), 2);
        assert_eq!(next.iter().map(|t| t.format("%a %H:%M").to_string()).collect::<Vec<_>>(), ["Mon 08:30", "Tue 08:30"]);
        assert!(workflow::cron("every day").is_err());
        assert!(workflow::cron("0 9 * *").is_err());
        assert!(workflow::cron("61 9 * * *").is_err());
    }
}
