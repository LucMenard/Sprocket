//! The engine's flows, with a fake agent host that records what it was sent.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::db::Db;

#[derive(Default)]
struct Fake {
    sent: Mutex<Vec<CoreMessage>>,
    down: AtomicBool,
    changes: AtomicUsize,
}

struct FakeSink(Arc<Fake>);

impl Sink for FakeSink {
    fn send(&self, message: CoreMessage) -> bool {
        self.0.sent.lock().unwrap().push(message);
        true
    }
    fn host_ready(&self) -> bool {
        !self.0.down.load(Ordering::SeqCst)
    }
    fn changed(&self) {
        self.0.changes.fetch_add(1, Ordering::SeqCst);
    }
}

struct World {
    engine: Engine,
    fake: Arc<Fake>,
    folder: String,
    _dir: tempfile::TempDir,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let workflows = dir.path().join("workflows");
    workflow::load_all(&workflows).unwrap(); // writes the example: plan → approve → build
    let fake = Arc::new(Fake::default());
    let db = Db::open(&dir.path().join("sprocket.sqlite")).unwrap();
    let engine = Engine::new(db, Box::new(FakeSink(fake.clone())), workflows);
    World { engine, fake, folder: dir.path().display().to_string(), _dir: dir }
}

impl World {
    fn start(&self) -> RunView {
        self.engine
            .start_run(StartRun {
                workflow: "fix-a-bug".into(),
                ticket: TicketRef { key: "SIS-244".into(), title: "Fix it".into(), url: "https://x/browse/SIS-244".into() },
                folder: self.folder.clone(),
                started_by: "Luc".into(),
            })
            .unwrap()
    }

    /// The id and prompt of the last job.start sent to the host.
    fn last_start(&self) -> (String, String) {
        self.fake
            .sent
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|m| match m {
                CoreMessage::JobStart { job_id, prompt, .. } => Some((job_id.clone(), prompt.clone())),
                _ => None,
            })
            .expect("a job.start")
    }

    fn host(&self, message: serde_json::Value) {
        self.engine.on_host(HostEvent::Message(serde_json::from_value(message).unwrap()));
    }

    fn end(&self, job_id: &str, output: &str) {
        self.host(json!({"type": "job.ended", "jobId": job_id, "result": "success", "output": output, "costUsd": 0.5}));
    }

    fn run(&self, id: &str) -> RunView {
        self.engine.run(id).unwrap()
    }
}

#[test]
fn a_run_goes_plan_gate_build_and_passes() {
    let w = world();
    let run = w.start();
    assert_eq!((run.number, run.state.as_str()), (1, "running"));
    let (plan_job, prompt) = w.last_start();
    assert!(prompt.contains("Jira ticket SIS-244: Fix it") && prompt.contains("Do not change any file"));

    w.end(&plan_job, "1. change foo");
    let run = w.run(&run.id);
    assert_eq!(run.state, "waiting");
    assert_eq!(run.approvals.len(), 1);
    assert_eq!((run.approvals[0].kind.as_str(), run.approvals[0].summary.as_str()), ("gate", "1. change foo"));

    w.engine.decide(&run.approvals[0].id, true, None, "Luc").unwrap();
    let (build_job, prompt) = w.last_start();
    assert!(prompt.contains("## Input from earlier steps\n\n1. change foo"));
    assert_eq!(w.run(&run.id).state, "running");

    w.end(&build_job, "PR #12");
    let run = w.run(&run.id);
    assert_eq!(run.state, "passed");
    assert!(run.ended_at.is_some());
    assert_eq!(run.cost_usd, 1.0);
    let states: Vec<_> = run.jobs.iter().map(|j| (j.step_id.as_str(), j.state.as_str())).collect();
    assert_eq!(states, [("plan", "done"), ("approve", "done"), ("build", "done")]);
}

#[test]
fn rejecting_the_gate_sends_the_run_back_with_the_reason() {
    let w = world();
    let run = w.start();
    let (plan_job, _) = w.last_start();
    w.end(&plan_job, "plan A");
    let gate = w.run(&run.id).approvals[0].id.clone();

    w.engine.decide(&gate, false, Some("Also update the docs".into()), "Luc").unwrap();
    let (again, prompt) = w.last_start();
    assert_ne!(again, plan_job);
    assert!(prompt.contains("## Your previous result was rejected\n\nAlso update the docs"));
    let run = w.run(&run.id);
    assert_eq!(run.state, "running");
    let plan_attempts: Vec<_> = run.jobs.iter().filter(|j| j.step_id == "plan").map(|j| j.attempt).collect();
    assert_eq!(plan_attempts, [1, 2]);
    // The same gate can't be decided twice
    assert_eq!(w.engine.decide(&gate, true, None, "Luc").unwrap_err().kind, "conflict");
}

#[test]
fn a_tool_ask_waits_then_its_answer_goes_back_to_the_host() {
    let w = world();
    let run = w.start();
    let (job, _) = w.last_start();
    w.host(json!({"type": "tool.ask", "askId": "ask-1", "jobId": job, "toolName": "Bash", "input": {"command": "ls"}}));
    let view = w.run(&run.id);
    assert_eq!(view.state, "waiting");
    let ask = &view.approvals[0];
    assert_eq!((ask.kind.as_str(), ask.tool_name.as_deref()), ("tool", Some("Bash")));
    assert_eq!(ask.input, Some(json!({"command": "ls"})));

    w.engine.decide(&ask.id, false, Some("not that".into()), "Luc").unwrap();
    let answered = w.fake.sent.lock().unwrap().iter().any(|m| {
        matches!(m, CoreMessage::ToolAnswer { ask_id, behavior: Behavior::Deny, message: Some(why) } if ask_id == "ask-1" && why == "not that")
    });
    assert!(answered);
    assert_eq!(w.run(&run.id).state, "running");
}

#[test]
fn a_failed_job_fails_the_run_and_can_be_retried_with_its_session() {
    let w = world();
    let run = w.start();
    let (job, _) = w.last_start();
    w.host(json!({"type": "job.started", "jobId": job, "sdkSessionId": "sess-1"}));
    w.host(json!({"type": "job.ended", "jobId": job, "result": "error", "output": null, "costUsd": 0.1, "error": "error_max_turns"}));
    let view = w.run(&run.id);
    assert_eq!(view.state, "failed");
    assert_eq!(view.jobs[0].error.as_deref(), Some("error_max_turns"));

    w.engine.retry_job(&job).unwrap();
    let resumed = w.fake.sent.lock().unwrap().iter().any(|m| {
        matches!(m, CoreMessage::JobResume { sdk_session_id, .. } if sdk_session_id == "sess-1")
    });
    assert!(resumed);
    assert_eq!(w.run(&run.id).state, "running");
}

#[test]
fn the_host_going_away_interrupts_agents_but_keeps_gates() {
    let w = world();
    let waiting = w.start();
    let (plan_job, _) = w.last_start();
    w.end(&plan_job, "plan");
    let running = w.start();
    let (_, _) = w.last_start();
    w.host(json!({"type": "tool.ask", "askId": "a", "jobId": w.last_start().0, "toolName": "Write", "input": {}}));

    w.engine.on_host(HostEvent::Exited);
    let gate_run = w.run(&waiting.id);
    assert_eq!((gate_run.state.as_str(), gate_run.approvals.len()), ("waiting", 1));
    let agent_run = w.run(&running.id);
    assert_eq!(agent_run.state, "interrupted");
    assert!(agent_run.approvals.is_empty(), "tool asks expire with their job");
    assert_eq!(agent_run.jobs[0].state, "interrupted");
}

#[test]
fn starting_checks_the_ticket_the_folder_and_the_limit() {
    let w = world();
    let bad = |key: &str, folder: &str| {
        w.engine
            .start_run(StartRun {
                workflow: "fix-a-bug".into(),
                ticket: TicketRef { key: key.into(), title: "t".into(), url: "u".into() },
                folder: folder.into(),
                started_by: "Luc".into(),
            })
            .unwrap_err()
            .kind
    };
    assert_eq!(bad("not a key", &w.folder), "invalid");
    assert_eq!(bad("SIS-1", "C:/no/such/folder"), "invalid");
    w.start();
    w.start();
    // fix-a-bug allows two runs at once
    assert_eq!(bad("SIS-1", &w.folder), "busy");
}

#[test]
fn without_a_host_the_run_is_interrupted_not_lost() {
    let w = world();
    w.fake.down.store(true, Ordering::SeqCst);
    let run = w.start();
    assert_eq!(run.state, "interrupted");
    assert!(w.fake.sent.lock().unwrap().is_empty());
    w.fake.down.store(false, Ordering::SeqCst);
    w.engine.retry_job(&run.jobs[0].id).unwrap();
    assert_eq!(w.run(&run.id).state, "running");
}

#[test]
fn cancelling_stops_the_agent_and_expires_approvals() {
    let w = world();
    let run = w.start();
    let (job, _) = w.last_start();
    w.engine.cancel_run(&run.id, "Luc").unwrap();
    let stopped = w.fake.sent.lock().unwrap().iter().any(|m| matches!(m, CoreMessage::JobStop { job_id } if *job_id == job));
    assert!(stopped);
    assert_eq!(w.run(&run.id).state, "cancelled");
    // The host's late answer for the stopped job changes nothing
    w.end(&job, "late");
    assert_eq!(w.run(&run.id).state, "cancelled");
    assert!(w.fake.changes.load(Ordering::SeqCst) > 0);
}

#[test]
fn jobs_keep_a_log_and_runs_feed_the_activity() {
    let w = world();
    let run = w.start();
    let (job, _) = w.last_start();
    w.host(json!({"type": "job.message", "jobId": job, "message": {"type": "system", "subtype": "init", "session_id": "s",
        "mcp_servers": [{"name": "Atlassian2", "status": "connected", "source": "user"}]}}));
    w.host(json!({"type": "tool.ask", "askId": "a", "jobId": job, "toolName": "Bash", "input": {}}));
    let ask = w.run(&run.id).approvals[0].id.clone();
    w.engine.decide(&ask, true, Some("fine".into()), "Luc").unwrap();
    w.end(&job, "plan");

    let view = w.run(&run.id);
    let log: Vec<_> = view.jobs[0].log.iter().map(|l| l.what.as_str()).collect();
    assert_eq!(log, ["job.started", "tool.asked", "tool.approved", "job.done"]);
    assert_eq!(view.jobs[0].log[2].detail.as_deref(), Some("Luc: fine"));

    let feed = w.engine.activity(50).unwrap();
    assert_eq!(feed[0].what, "gate.opened");
    assert_eq!(feed.last().unwrap().what, "run.started");
    assert_eq!(feed[0].ticket_key.as_deref(), Some("SIS-244"));

    let mcp = w.engine.mcp_servers().unwrap();
    assert!(mcp.seen_at.is_some());
    assert_eq!((mcp.servers[0].name.as_str(), mcp.servers[0].status.as_str()), ("Atlassian2", "connected"));
}
