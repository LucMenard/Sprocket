//! The engine's flows, with a fake agent host that records what it was sent.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::host::protocol::{AgentSpec, BoardAccess};
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

// ---- Conditions, loops, actions and JSON outputs ----

const RICH: &str = r#"
name: Rich
steps:
- id: triage
  agent: planner
  output: { files: list, risky: yes/no }
- id: route
  if: steps.triage.output.risky
  then:
  - id: check
    approval: { summary: "Risky: {{ steps.triage.output.files }}" }
- id: each
  forEach: steps.triage.output.files
  as: file
  parallel: 2
  steps:
  - id: fix
    agent: builder
    instructions: Fix {{ file }}
- id: tell
  agent: planner
  action: { do: comment, text: "Fixed {{ steps.triage.output.files }}" }
- id: report
  agent: planner
  input: steps.each.output
"#;

impl World {
    fn with(&self, id: &str, yaml: &str) {
        std::fs::write(std::path::Path::new(&self.folder).join("workflows").join(format!("{id}.yaml")), yaml).unwrap();
    }

    fn start_wf(&self, id: &str) -> RunView {
        self.engine
            .start_run(StartRun {
                workflow: id.into(),
                ticket: TicketRef { key: "SIS-245".into(), title: "Many files".into(), url: "u".into() },
                folder: self.folder.clone(),
                started_by: "Luc".into(),
            })
            .unwrap()
    }

    /// Every job.start sent: id, prompt, output schema.
    fn starts(&self) -> Vec<(String, String, Option<serde_json::Value>)> {
        self.fake
            .sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                CoreMessage::JobStart { job_id, prompt, output_schema, .. } => Some((job_id.clone(), prompt.clone(), output_schema.clone())),
                _ => None,
            })
            .collect()
    }

    fn end_with(&self, job_id: &str, data: serde_json::Value) {
        self.host(json!({"type": "job.ended", "jobId": job_id, "result": "success", "output": "", "costUsd": 0.1, "data": data}));
    }

    fn fail(&self, job_id: &str) {
        self.host(json!({"type": "job.ended", "jobId": job_id, "result": "error", "output": null, "costUsd": 0.1, "error": "boom"}));
    }

    fn current(&self, run: &str) -> Vec<(String, String, String)> {
        self.run(run).jobs.iter().filter(|j| !j.superseded).map(|j| (j.step_id.clone(), j.scope.clone(), j.state.clone())).collect()
    }
}

#[test]
fn a_loop_runs_its_items_two_at_a_time_and_gives_their_results() {
    let w = world();
    w.with("rich", RICH);
    let run = w.start_wf("rich");
    let starts = w.starts();
    let (triage, _, schema) = &starts[0];
    assert_eq!(schema.as_ref().unwrap()["required"], json!(["files", "risky"]));

    // Not risky: the condition has no else, so the loop comes next, two items at a time
    w.end_with(triage, json!({"files": ["a.rs", "b.rs", "c.rs"], "risky": false}));
    let starts = w.starts();
    assert_eq!(starts.len(), 3);
    assert!(starts[1].1.contains("Fix a.rs") && starts[2].1.contains("Fix b.rs"));
    assert!(starts[1].1.contains("Your step: fix (each 1)"));
    assert_eq!(w.run(&run.id).state, "running");

    w.end(&starts[1].0, "A done");
    let starts = w.starts();
    assert!(starts[3].1.contains("Fix c.rs"));
    w.end(&starts[2].0, "B done");
    w.end(&starts[3].0, "C done");

    // The action is an agent told to do one thing
    let starts = w.starts();
    assert!(starts[4].1.contains("Add this comment to the Jira ticket SIS-245, exactly as written:\n\nFixed [\n  \"a.rs\""));
    w.end(&starts[4].0, "Commented");
    let starts = w.starts();
    assert!(starts[5].1.contains("## Input from earlier steps\n\n[\n  \"A done\",\n  \"B done\",\n  \"C done\"\n]"));
    w.end(&starts[5].0, "Report");

    let view = w.run(&run.id);
    assert_eq!(view.state, "passed");
    let scopes: Vec<_> = view.jobs.iter().map(|j| (j.step_id.as_str(), j.scope.as_str(), j.kind.as_str())).collect();
    assert_eq!(
        scopes,
        [
            ("triage", "", "agent"),
            ("route", "", "condition"),
            ("each", "", "loop"),
            ("fix", "each[0]/", "agent"),
            ("fix", "each[1]/", "agent"),
            ("fix", "each[2]/", "agent"),
            ("tell", "", "action"),
            ("report", "", "agent"),
        ]
    );
    assert_eq!(view.jobs[0].data, Some(json!({"files": ["a.rs", "b.rs", "c.rs"], "risky": false})));
}

#[test]
fn a_condition_opens_its_branch() {
    let w = world();
    w.with("rich", RICH);
    let run = w.start_wf("rich");
    w.end_with(&w.starts()[0].0, json!({"files": ["a.rs"], "risky": true}));
    let view = w.run(&run.id);
    assert_eq!(view.state, "waiting");
    assert_eq!(view.approvals[0].summary, "Risky: [\n  \"a.rs\"\n]");
    w.engine.decide(&view.approvals[0].id, true, None, "Luc").unwrap();
    assert!(w.starts()[1].1.contains("Fix a.rs"));
}

const REVIEWED: &str = r#"
name: Reviewed
steps:
- id: triage
  agent: planner
  output: { files: list }
- id: each
  forEach: steps.triage.output.files
  onItemFailure: stop
  steps:
  - id: fix
    agent: builder
    instructions: Fix {{ item }}
  - id: review
    approval: { summary: steps.fix.output }
    onReject: fix
"#;

#[test]
fn a_rejection_in_a_loop_redoes_that_item_only() {
    let w = world();
    w.with("reviewed", REVIEWED);
    let run = w.start_wf("reviewed");
    w.end_with(&w.starts()[0].0, json!({"files": ["a.rs", "b.rs"]}));
    let fix_a = w.starts()[1].0.clone();
    w.end(&fix_a, "patch A");
    let gate = w.run(&run.id).approvals[0].clone();
    assert_eq!(gate.summary, "patch A");

    w.engine.decide(&gate.id, false, Some("Smaller".into()), "Luc").unwrap();
    let (again, prompt, _) = w.starts()[2].clone();
    assert!(prompt.contains("Fix a.rs") && prompt.contains("rejected\n\nSmaller"));
    w.end(&again, "patch A2");
    let gate = w.run(&run.id).approvals[0].clone();
    assert_eq!(gate.summary, "patch A2");
    w.engine.decide(&gate.id, true, None, "Luc").unwrap();
    // Parallel 1 by default: b starts only now
    assert!(w.starts()[3].1.contains("Fix b.rs"));
    let view = w.run(&run.id);
    let a: Vec<_> = view.jobs.iter().filter(|j| j.scope == "each[0]/").map(|j| (j.step_id.as_str(), j.state.as_str(), j.superseded)).collect();
    assert_eq!(a, [("fix", "done", true), ("review", "rejected", true), ("fix", "done", false), ("review", "done", false)]);
}

#[test]
fn a_failed_item_stops_the_loop_and_retrying_it_goes_on() {
    let w = world();
    w.with("reviewed", REVIEWED);
    let run = w.start_wf("reviewed");
    w.end_with(&w.starts()[0].0, json!({"files": ["a.rs", "b.rs"]}));
    let fix_a = w.starts()[1].0.clone();
    w.fail(&fix_a);
    let view = w.run(&run.id);
    assert_eq!(view.state, "failed");
    assert_eq!(view.jobs.iter().find(|j| j.kind == "loop").unwrap().error.as_deref(), Some("Item 1 failed"));

    w.engine.retry_job(&fix_a).unwrap();
    assert_eq!(w.run(&run.id).state, "running");
    let again = w.starts()[2].0.clone();
    w.end(&again, "ok");
    let current = w.current(&run.id);
    assert!(current.contains(&("each".into(), "".into(), "running".into())));
    assert!(current.contains(&("review".into(), "each[0]/".into(), "waiting".into())));
}

#[test]
fn with_continue_a_failed_item_lets_the_others_finish() {
    let w = world();
    let yaml = REVIEWED
        .replace("onItemFailure: stop", "onItemFailure: continue")
        .replace("  - id: review\n    approval: { summary: steps.fix.output }\n    onReject: fix\n", "");
    w.with("reviewed", &yaml);
    let run = w.start_wf("reviewed");
    w.end_with(&w.starts()[0].0, json!({"files": ["a.rs", "b.rs"]}));
    w.fail(&w.starts()[1].0);
    w.end(&w.starts()[2].0, "B");
    let view = w.run(&run.id);
    assert_eq!(view.state, "passed");
    let lp = view.jobs.iter().find(|j| j.kind == "loop").unwrap();
    assert_eq!(lp.output.as_deref(), Some(r#"[null,"B"]"#));
}

#[test]
fn an_empty_list_skips_the_loop() {
    let w = world();
    w.with("reviewed", REVIEWED);
    let run = w.start_wf("reviewed");
    w.end_with(&w.starts()[0].0, json!({"files": []}));
    assert_eq!(w.run(&run.id).state, "passed");
}

#[test]
fn the_host_going_away_mid_loop_and_a_retry_resume_every_item() {
    let w = world();
    w.with("rich", RICH);
    let run = w.start_wf("rich");
    w.end_with(&w.starts()[0].0, json!({"files": ["a.rs", "b.rs", "c.rs"], "risky": false}));
    let starts = w.starts();
    w.host(json!({"type": "job.started", "jobId": starts[1].0, "sdkSessionId": "s-a"}));
    w.host(json!({"type": "job.started", "jobId": starts[2].0, "sdkSessionId": "s-b"}));
    w.engine.on_host(HostEvent::Exited);
    assert_eq!(w.run(&run.id).state, "interrupted");

    w.engine.retry_job(&starts[1].0).unwrap();
    let resumed: Vec<String> = w
        .fake
        .sent
        .lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            CoreMessage::JobResume { sdk_session_id, .. } => Some(sdk_session_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(resumed, ["s-a", "s-b"]);
    assert_eq!(w.run(&run.id).state, "running");
}

#[test]
fn a_gate_waits_while_its_run_is_interrupted_and_decides_once_resumed() {
    let w = world();
    w.with("reviewed", &REVIEWED.replace("onItemFailure: stop", "parallel: 2"));
    let run = w.start_wf("reviewed");
    w.end_with(&w.starts()[0].0, json!({"files": ["a.rs", "b.rs"]}));
    // Item 1 reaches its gate while item 2's agent is still working; then the host goes away
    w.end(&w.starts()[1].0, "patch A");
    let gate = w.run(&run.id).approvals[0].id.clone();
    w.engine.on_host(HostEvent::Exited);
    assert_eq!(w.run(&run.id).state, "interrupted");
    assert_eq!(w.engine.decide(&gate, false, Some("No".into()), "Luc").unwrap_err().kind, "conflict");
    assert_eq!(w.run(&run.id).approvals.len(), 1, "the refused decision changed nothing");

    let b = w.run(&run.id).jobs.iter().find(|j| j.scope == "each[1]/").unwrap().id.clone();
    w.engine.retry_job(&b).unwrap();
    assert_eq!(w.run(&run.id).state, "waiting");
    w.engine.decide(&gate, true, None, "Luc").unwrap();
    assert_eq!(w.run(&run.id).state, "running");
}

// ---- The board ----

impl World {
    /// The board answer sent for a request.
    fn board_answer(&self, request: &str) -> (String, bool) {
        self.fake
            .sent
            .lock()
            .unwrap()
            .iter()
            .find_map(|m| match m {
                CoreMessage::BoardAnswer { request_id, text, error } if request_id == request => Some((text.clone(), *error)),
                _ => None,
            })
            .expect("a board answer")
    }

    fn spec_of(&self, job: &str) -> AgentSpec {
        self.fake
            .sent
            .lock()
            .unwrap()
            .iter()
            .find_map(|m| match m {
                CoreMessage::JobStart { job_id, agent, .. } if job_id == job => Some(agent.clone()),
                _ => None,
            })
            .expect("a job.start")
    }
}

#[test]
fn agents_post_and_read_the_board_and_people_join_in() {
    let w = world();
    let run = w.start();
    let (plan, prompt) = w.last_start();
    assert!(prompt.contains("read_notes") && prompt.contains("post_note"));
    assert_eq!(w.spec_of(&plan).board, BoardAccess::Post);

    w.host(json!({"type": "board.post", "requestId": "r1", "jobId": plan, "message": "The flaky test is unrelated", "to": "build"}));
    assert_eq!(w.board_answer("r1"), ("Posted as note #1.".into(), false));
    w.engine.post_note(&run.id, "Luc", "Keep the API as is", None).unwrap();
    assert!(w.engine.post_note(&run.id, "Luc", "   ", None).is_err());

    w.end(&plan, "the plan");
    w.engine.decide(&w.run(&run.id).approvals[0].id, true, None, "Luc").unwrap();
    let (build, _) = w.last_start();
    w.host(json!({"type": "board.read", "requestId": "r2", "jobId": build}));
    let (text, error) = w.board_answer("r2");
    assert!(!error);
    assert!(text.contains("plan (planner) → build, that is you\nThe flaky test is unrelated"), "{text}");
    assert!(text.contains("Luc (a person)\nKeep the API as is"));
    w.host(json!({"type": "board.read", "requestId": "r3", "jobId": build, "since": 1}));
    assert!(!w.board_answer("r3").0.contains("flaky"));

    let notes = w.run(&run.id).notes;
    assert_eq!(notes.iter().map(|n| n.author.as_str()).collect::<Vec<_>>(), ["plan", "Luc"]);
    assert_eq!((notes[0].agent.as_deref(), notes[0].to.as_deref()), (Some("planner"), Some("build")));
}

#[test]
fn a_rejection_reason_goes_on_the_board_for_the_step_that_redoes_it() {
    let w = world();
    let run = w.start();
    let (plan, _) = w.last_start();
    w.end(&plan, "plan A");
    w.engine.decide(&w.run(&run.id).approvals[0].id, false, Some("Also the docs".into()), "Luc").unwrap();
    let note = w.run(&run.id).notes.pop().unwrap();
    assert_eq!((note.author.as_str(), note.to.as_deref(), note.body.as_str()), ("Luc", Some("plan"), "Rejected at approve: Also the docs"));
}

#[test]
fn without_a_board_or_the_right_to_post_the_tools_say_no() {
    let w = world();
    w.with("quiet", "name: Quiet\nboard: false\nsteps:\n- id: plan\n  agent: planner\n");
    let run = w.start_wf("quiet");
    let (job, prompt, _) = w.starts()[0].clone();
    assert!(!prompt.contains("read_notes"));
    assert_eq!(w.spec_of(&job).board, BoardAccess::Off);
    w.host(json!({"type": "board.read", "requestId": "q1", "jobId": job}));
    assert!(w.board_answer("q1").1);
    assert_eq!(w.engine.post_note(&run.id, "Luc", "hi", None).unwrap_err().kind, "conflict");

    let reader = "name: Reader\nagents:\n  shy:\n    model: m\n    ask: never\n    board: false\nsteps:\n- id: look\n  agent: shy\n";
    w.with("reader", reader);
    w.start_wf("reader");
    let (job, prompt, _) = w.starts()[1].clone();
    assert!(prompt.contains("read_notes") && !prompt.contains("post_note"));
    assert_eq!(w.spec_of(&job).board, BoardAccess::Read);
    w.host(json!({"type": "board.post", "requestId": "q2", "jobId": job, "message": "hello"}));
    assert_eq!(w.board_answer("q2"), ("Your agent type may read the board, not post to it".into(), true));
}

#[test]
fn parallel_branches_start_together_and_the_next_step_waits_for_all() {
    let w = world();
    w.with("together", "name: Together\nsteps:\n- id: both\n  branches:\n  - - id: code\n      agent: builder\n  - - id: docs\n      agent: planner\n- id: merge\n  agent: builder\n");
    let run = w.start_wf("together");
    let starts = w.starts();
    assert_eq!(starts.len(), 2, "both branches start at once");
    w.end(&starts[0].0, "code done");
    assert_eq!(w.starts().len(), 2, "merge waits for docs");
    w.end(&starts[1].0, "docs done");
    let merge = w.starts()[2].0.clone();
    w.end(&merge, "merged");
    assert_eq!(w.run(&run.id).state, "passed");

    // A failed branch fails the run once the other has ended
    let run = w.start_wf("together");
    let starts = w.starts();
    w.fail(&starts[3].0);
    assert_eq!(w.run(&run.id).state, "running");
    w.end(&starts[4].0, "docs");
    assert_eq!(w.run(&run.id).state, "failed");
}
