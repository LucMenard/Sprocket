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

const PLAN_APPROVE_BUILD: &str = "name: Fix a bug
offerInTracket: true
maxParallelRuns: 2
steps:
- id: plan
  agent: planner
  instructions: Read the ticket and the code it concerns, then write a short plan. Do not change any file.
- id: approve
  approval:
    summary: steps.plan.output
  onReject: plan
- id: build
  agent: builder
  input: steps.plan.output
  instructions: Implement the plan, run the tests, push a branch named after the ticket and open a draft PR.
";

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let workflows = dir.path().join("workflows");
    workflow::load_all(&workflows).unwrap(); // writes the examples and the shared agents
    // The tests drive a fixed plan → approve → build, whatever the shipped example becomes
    std::fs::write(workflows.join("fix-a-bug.yaml"), PLAN_APPROVE_BUILD).unwrap();
    let fake = Arc::new(Fake::default());
    let db = Db::open(&dir.path().join("sprocket.sqlite")).unwrap();
    let engine = Engine::new(db, Box::new(FakeSink(fake.clone())), workflows);
    // Agents only work in a git working tree
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    World { engine, fake, folder: dir.path().display().to_string(), _dir: dir }
}

impl World {
    fn start(&self) -> RunView {
        self.engine
            .start_run(StartRun {
                workflow: "fix-a-bug".into(),
                ticket: Some(TicketRef { key: "SIS-244".into(), title: "Fix it".into(), url: "https://x/browse/SIS-244".into() }),
                folder: self.folder.clone(),
                started_by: "Luc".into(),
                request: None,
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
                ticket: Some(TicketRef { key: key.into(), title: "t".into(), url: "u".into() }),
                folder: folder.into(),
                started_by: "Luc".into(),
                request: None,
            })
            .unwrap_err()
            .kind
    };
    assert_eq!(bad("not a key", &w.folder), "invalid");
    assert_eq!(bad("SIS-1", "C:/no/such/folder"), "invalid");
    // Relative, not a git working tree, or out of one with `..`: refused
    assert_eq!(bad("SIS-1", "workflows"), "invalid");
    let plain = tempfile::tempdir().unwrap();
    assert_eq!(bad("SIS-1", &plain.path().display().to_string()), "invalid");
    assert_eq!(bad("SIS-1", &format!("{}/..", w.folder)), "invalid");
    assert_eq!(bad("SIS-1", r"\\server\share\repo"), "invalid");
    w.start();
    w.start();
    // fix-a-bug allows two runs at once
    assert_eq!(bad("SIS-1", &w.folder), "busy");
}

#[test]
fn the_worktree_folder_is_kept_resolved() {
    let w = world();
    let run = w
        .engine
        .start_run(StartRun {
            workflow: "fix-a-bug".into(),
            ticket: Some(TicketRef { key: "SIS-9".into(), title: "t".into(), url: "u".into() }),
            folder: format!("{}/workflows/..", w.folder),
            started_by: "Luc".into(),
            request: None,
        })
        .unwrap();
    assert!(!run.folder.contains("..") && !run.folder.starts_with(r"\\"), "{}", run.folder);
    assert!(std::path::Path::new(&run.folder).join(".git").exists());
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
    // A later session of an agent with no MCP server sees only Sprocket's own: the list keeps the user's
    w.host(json!({"type": "job.message", "jobId": job, "message": {"type": "system", "subtype": "init", "session_id": "s",
        "mcp_servers": [{"name": "sprocket", "status": "connected"}]}}));
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
    let names: Vec<_> = mcp.servers.iter().map(|s| (s.name.as_str(), s.status.as_str())).collect();
    assert_eq!(names, [("Atlassian2", "connected")]);
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
                ticket: Some(TicketRef { key: "SIS-245".into(), title: "Many files".into(), url: "u".into() }),
                folder: self.folder.clone(),
                started_by: "Luc".into(),
                request: None,
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
    assert!(starts[4].1.contains("Add this comment to the Jira ticket SIS-245, exactly as written, with its last line:\n\nFixed [\n  \"a.rs\""));
    // Signed, so Tracket shows it came from Sprocket
    assert!(starts[4].1.contains("\n\n— Posted by Sprocket · Rich #1\n"), "{}", starts[4].1);
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

// ---- Asking a colleague ----

impl World {
    /// The colleague answer sent for a request.
    fn colleague_answer(&self, request: &str) -> Option<(String, bool)> {
        self.fake.sent.lock().unwrap().iter().find_map(|m| match m {
            CoreMessage::ColleagueAnswer { request_id, text, error } if request_id == request => Some((text.clone(), *error)),
            _ => None,
        })
    }

    fn ask(&self, request: &str, job: &str, agent: &str, question: &str) {
        self.host(json!({"type": "colleague.ask", "requestId": request, "jobId": job, "agentType": agent, "question": question}));
    }
}

#[test]
fn an_agent_asks_a_colleague_and_gets_its_answer() {
    let w = world();
    let run = w.start();
    let (plan, prompt) = w.last_start();
    assert!(prompt.contains("ask_agent, up to 3 times"));
    let ids: Vec<String> = w.spec_of(&plan).colleagues.iter().map(|c| c.id.clone()).collect();
    assert_eq!(ids, ["planner", "builder"]);

    w.ask("q1", &plan, "builder", "Which test covers the parser?");
    let (ask, prompt) = w.last_start();
    assert_ne!(ask, plan);
    assert!(prompt.contains("The planner agent, working on the step plan, asks you a question") && prompt.contains("Which test covers the parser?"));
    assert!(w.spec_of(&ask).colleagues.is_empty(), "no chains: the asked one can't ask");

    // The asked one can't ask in turn
    w.ask("q2", &ask, "planner", "And you?");
    assert!(w.colleague_answer("q2").unwrap().1);

    w.end(&ask, "tests/parser.rs");
    assert_eq!(w.colleague_answer("q1"), Some(("tests/parser.rs".into(), false)));
    let view = w.run(&run.id);
    assert_eq!(view.state, "running");
    let job = view.jobs.iter().find(|j| j.kind == "ask").unwrap();
    assert_eq!((job.parent_job.as_deref(), job.question.as_deref(), job.state.as_str()), (Some(plan.as_str()), Some("Which test covers the parser?"), "done"));
    assert_eq!(job.step_id, "plan~builder");
    let log: Vec<_> = view.jobs.iter().find(|j| j.id == plan).unwrap().log.iter().map(|l| l.what.as_str()).collect();
    assert_eq!(log, ["job.started", "colleague.asked", "colleague.answered"]);
    // The run goes on as before: the plan's end opens the gate
    w.end(&plan, "the plan");
    assert_eq!(w.run(&run.id).state, "waiting");
}

#[test]
fn asks_are_limited_and_only_to_who_the_workflow_allows() {
    let w = world();
    w.with("limited", "name: L\ncolleagues: { canAsk: [builder], maxAsks: 1 }\nsteps:\n- id: plan\n  agent: planner\n");
    w.start_wf("limited");
    let plan = w.starts()[0].0.clone();
    w.ask("a1", &plan, "planner", "?");
    assert!(w.colleague_answer("a1").unwrap().0.contains("You may ask: builder"));
    w.ask("a2", &plan, "builder", "first");
    assert!(w.colleague_answer("a2").is_none(), "started, not answered yet");
    w.ask("a3", &plan, "builder", "second");
    assert!(w.colleague_answer("a3").unwrap().0.contains("its limit"));

    // The asking job ends: its ask still going is stopped
    let ask = w.starts()[1].0.clone();
    w.end(&plan, "done");
    assert!(w.fake.sent.lock().unwrap().iter().any(|m| matches!(m, CoreMessage::JobStop { job_id } if *job_id == ask)));

    // An agent type that may not ask, or a workflow that lets nobody be asked
    w.with("shy", "name: S\nagents:\n  loner:\n    model: m\n    ask: never\n    colleagues: false\nsteps:\n- id: solo\n  agent: loner\n");
    w.start_wf("shy");
    let solo = w.starts()[2].0.clone();
    assert!(w.spec_of(&solo).colleagues.is_empty());
    w.ask("s1", &solo, "loner", "?");
    assert!(w.colleague_answer("s1").unwrap().0.contains("may not ask"));
}

// ---- Team steps ----

const TEAMWORK: &str = "name: Teamwork\nsteps:\n- id: design\n  team:\n    members: [planner, builder]\n    lead: planner\n    writer: builder\n    goal: Agree on the change and make it\n    maxTurns: 10\n- id: after\n  agent: planner\n  input: steps.design.output\n";

impl World {
    /// The member jobs started, by agent type: (job id, prompt, spec).
    fn members(&self) -> Vec<(String, String, AgentSpec)> {
        self.fake
            .sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                CoreMessage::JobStart { job_id, prompt, agent, .. } | CoreMessage::JobResume { job_id, prompt, agent, .. } if agent.team.is_some() => {
                    Some((job_id.clone(), prompt.clone(), agent.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn board_reply(&self, request: &str) -> Option<String> {
        self.fake.sent.lock().unwrap().iter().find_map(|m| match m {
            CoreMessage::BoardAnswer { request_id, text, .. } if request_id == request => Some(text.clone()),
            _ => None,
        })
    }

    fn stopped(&self, job: &str) -> bool {
        self.fake.sent.lock().unwrap().iter().any(|m| matches!(m, CoreMessage::JobStop { job_id } if job_id == job))
    }

    fn team_start(&self) -> (RunView, String, String) {
        self.with("teamwork", TEAMWORK);
        let before = self.members().len();
        let run = self.start_wf("teamwork");
        let m = self.members().split_off(before);
        let lead = m.iter().find(|x| x.2.team.as_ref().unwrap().lead).unwrap().0.clone();
        let other = m.iter().find(|x| !x.2.team.as_ref().unwrap().lead).unwrap().0.clone();
        (run, lead, other)
    }
}

#[test]
fn a_team_starts_together_talks_and_the_lead_finishes() {
    let w = world();
    let (run, lead, builder) = w.team_start();
    let m = w.members();
    assert_eq!(m.len(), 2, "both members start at once");
    let lead_spec = &m.iter().find(|x| x.0 == lead).unwrap().2;
    let builder_spec = &m.iter().find(|x| x.0 == builder).unwrap().2;
    assert!(lead_spec.read_only && !builder_spec.read_only, "only the writer changes files");
    assert_eq!(lead_spec.limits.unwrap().max_turns, 10);
    assert!(lead_spec.colleagues.is_empty());
    assert!(m.iter().find(|x| x.0 == lead).unwrap().1.contains("You lead the team"));

    // The lead waits; the builder posts; the wait answers with the note
    w.host(json!({"type": "board.wait", "requestId": "w1", "jobId": lead}));
    assert!(w.board_reply("w1").is_none(), "nothing new yet: it waits");
    w.host(json!({"type": "board.post", "requestId": "p1", "jobId": builder, "message": "Patch ready", "to": "planner"}));
    assert!(w.board_reply("w1").unwrap().contains("builder in design (builder) → planner, that is you\nPatch ready"));
    // Read already: a new wait doesn't get it again
    w.host(json!({"type": "board.wait", "requestId": "w2", "jobId": lead}));
    assert!(w.board_reply("w2").is_none());

    // Only the lead finishes
    w.host(json!({"type": "team.finish", "requestId": "f0", "jobId": builder, "summary": "mine"}));
    assert!(w.board_reply("f0").unwrap().contains("only it finishes"));
    w.host(json!({"type": "team.finish", "requestId": "f1", "jobId": lead, "summary": "Changed the parser; tests pass"}));
    assert!(w.board_reply("f1").unwrap().contains("The step is done"));
    assert!(w.stopped(&builder));
    let view = w.run(&run.id);
    let team = view.jobs.iter().find(|j| j.kind == "team").unwrap();
    assert_eq!((team.state.as_str(), team.output.as_deref()), ("done", Some("Changed the parser; tests pass")));
    // The next step has started, with the team's result as input
    assert!(w.last_start().1.contains("## Input from earlier steps\n\nChanged the parser; tests pass"));
    // The lead's session ending later changes nothing
    w.end(&lead, "bye");
    assert_eq!(w.run(&run.id).jobs.iter().find(|j| j.kind == "team").unwrap().state, "done");
}

#[test]
fn a_lead_that_stops_early_is_sent_back_and_ending_last_finishes() {
    let w = world();
    let (run, lead, builder) = w.team_start();
    w.host(json!({"type": "job.started", "jobId": lead, "sdkSessionId": "s-lead"}));
    w.end(&lead, "I'm done thinking");
    // The builder still works: the lead resumes, told to wait for it
    let resumed = w.fake.sent.lock().unwrap().iter().any(|m| matches!(m, CoreMessage::JobResume { sdk_session_id, prompt, .. } if sdk_session_id == "s-lead" && prompt.contains("still working (builder)")));
    assert!(resumed);
    let lead2 = w.members().last().unwrap().0.clone();
    w.host(json!({"type": "board.wait", "requestId": "w1", "jobId": lead2}));
    // The builder ends its turn: the waiting lead hears of it
    w.end(&builder, "patched");
    assert!(w.board_reply("w1").unwrap().contains("builder ended its turn"));
    // Now the lead is the last one working: ending its turn finishes the step with its words
    w.end(&lead2, "Summary: patched and tested");
    let team = w.run(&run.id).jobs.into_iter().find(|j| j.kind == "team").unwrap();
    assert_eq!(team.output.as_deref(), Some("Summary: patched and tested"));
}

#[test]
fn a_stop_rule_asks_whether_to_go_on_and_approving_resumes_the_members() {
    let w = world();
    let (run, lead, builder) = w.team_start();
    w.host(json!({"type": "job.started", "jobId": lead, "sdkSessionId": "s-lead"}));
    w.host(json!({"type": "job.started", "jobId": builder, "sdkSessionId": "s-builder"}));
    // The lead was asking a person something when the builder hit its turn limit
    w.host(json!({"type": "tool.ask", "askId": "a1", "jobId": lead, "toolName": "Read", "input": {}}));
    w.host(json!({"type": "job.ended", "jobId": builder, "result": "error", "output": null, "costUsd": 0.3, "error": "error_max_turns"}));
    assert!(w.stopped(&lead));
    let view = w.run(&run.id);
    assert_eq!(view.state, "waiting");
    assert_eq!(view.approvals.len(), 1, "the lead's tool ask expired; only the team's question is left");
    assert!(view.approvals[0].summary.contains("builder reached its turn limit"));

    w.engine.decide(&view.approvals[0].id, true, None, "Luc").unwrap();
    let resumed: Vec<String> = w.fake.sent.lock().unwrap().iter().filter_map(|m| match m {
        CoreMessage::JobResume { sdk_session_id, .. } => Some(sdk_session_id.clone()),
        _ => None,
    }).collect();
    assert_eq!(resumed.len(), 2);
    assert!(resumed.contains(&"s-lead".to_string()) && resumed.contains(&"s-builder".to_string()));
    assert_eq!(w.run(&run.id).state, "running");
}

#[test]
fn rejecting_the_stop_rule_fails_and_the_host_going_away_resumes_on_retry() {
    let w = world();
    let (run, _lead, builder) = w.team_start();
    w.host(json!({"type": "job.ended", "jobId": builder, "result": "error", "output": null, "costUsd": 0.3, "error": "error_max_budget_usd"}));
    let gate = w.run(&run.id).approvals[0].clone();
    assert!(gate.summary.contains("budget"));
    w.engine.decide(&gate.id, false, None, "Luc").unwrap();
    assert_eq!(w.run(&run.id).state, "failed");

    let (run, lead, builder) = w.team_start();
    w.host(json!({"type": "job.started", "jobId": lead, "sdkSessionId": "s-l"}));
    w.host(json!({"type": "job.started", "jobId": builder, "sdkSessionId": "s-b"}));
    w.engine.on_host(HostEvent::Exited);
    let view = w.run(&run.id);
    assert_eq!(view.state, "interrupted");
    let team = view.jobs.iter().find(|j| j.kind == "team").unwrap();
    assert_eq!(team.state, "interrupted");
    w.engine.retry_job(&team.id).unwrap();
    assert_eq!(w.run(&run.id).state, "running");
    let resumed = w.fake.sent.lock().unwrap().iter().filter(|m| matches!(m, CoreMessage::JobResume { sdk_session_id, .. } if sdk_session_id == "s-l" || sdk_session_id == "s-b")).count();
    assert_eq!(resumed, 2);
}

// ---- Live: a team with the real agent host ----

struct HostSink(Arc<crate::host::Host>);

impl Sink for HostSink {
    fn send(&self, message: CoreMessage) -> bool {
        self.0.send(message)
    }
    fn host_ready(&self) -> bool {
        self.0.is_ready()
    }
    fn changed(&self) {}
}

/// Two Haiku agents in a team, through the real agent host and Claude Code: the lead has to get a word only
/// the helper knows, over the board, and finish with it. Costs a few cents:
/// `cargo test --lib live_team -- --ignored --nocapture`
#[test]
fn a_request_reaches_the_prompts_and_templates_and_is_required() {
    let w = world();
    // Written next to fix-a-bug on first start
    let start = |request: Option<&str>| {
        w.engine.start_run(StartRun {
            workflow: "ask-an-agent".into(),
            ticket: Some(TicketRef { key: "SIS-7".into(), title: "Slow page".into(), url: "u".into() }),
            folder: w.folder.clone(),
            started_by: "Luc".into(),
            request: request.map(String::from),
        })
    };
    assert_eq!(start(None).unwrap_err().kind, "invalid");
    assert_eq!(start(Some("  ")).unwrap_err().kind, "invalid");

    let run = start(Some("Cache the list")).unwrap();
    assert_eq!(run.request.as_deref(), Some("Cache the list"));
    let (plan, prompt) = w.last_start();
    assert!(prompt.contains("## What was asked\n\nCache the list"), "{prompt}");
    w.end(&plan, "1. add a cache");
    let view = w.run(&run.id);
    w.engine.decide(&view.approvals[0].id, true, None, "Luc").unwrap();
    assert!(w.last_start().1.contains("## What was asked\n\nCache the list"));

    w.with("echo", "name: Echo\nforRequests: true\nsteps:\n- id: say\n  agent: builder\n  instructions: 'Do: {{ request }}'\n");
    let run = w.engine.start_run(StartRun {
        workflow: "echo".into(),
        ticket: Some(TicketRef { key: "SIS-8".into(), title: "t".into(), url: "u".into() }),
        folder: w.folder.clone(),
        started_by: "Luc".into(),
        request: Some("rename foo".into()),
    });
    assert!(run.is_ok(), "{:?}", run.err());
    assert!(w.last_start().1.contains("Do: rename foo"));
    // A workflow without forRequests doesn't need one
    assert!(w.start().request.is_none());
}

#[test]
fn tickets_sum_up_their_latest_run() {
    let w = world();
    let first = w.start();
    let (plan, _) = w.last_start();
    w.end(&plan, "the plan");
    let other = w.start_wf("fix-a-bug");
    w.fail(&w.last_start().0);

    let tickets = w.engine.tickets().unwrap();
    assert_eq!(tickets.len(), 2);
    let waiting = tickets.iter().find(|t| t.ticket_key == "SIS-244").unwrap();
    assert_eq!((waiting.run_id.as_str(), waiting.state.as_str()), (first.id.as_str(), "waiting"));
    assert_eq!(waiting.approval_summary.as_deref(), Some("the plan"));
    assert!(waiting.approval_id.is_some() && waiting.failed_job_id.is_none());
    assert_eq!(waiting.approval_kind.as_deref(), Some("gate"));
    let failed = tickets.iter().find(|t| t.ticket_key == "SIS-245").unwrap();
    assert_eq!((failed.run_id.as_str(), failed.state.as_str()), (other.id.as_str(), "failed"));
    assert_eq!(failed.failed_job_id.as_deref(), Some(other.jobs[0].id.as_str()));
    // Closing a failed run takes its "failed" away
    w.engine.cancel_run(&other.id, "Luc").unwrap();
    let closed = w.engine.tickets().unwrap();
    assert_eq!(closed.iter().find(|t| t.ticket_key == "SIS-245").unwrap().state, "cancelled");

    // A newer run on the ticket takes its place
    std::thread::sleep(std::time::Duration::from_millis(20));
    let newer = w.start();
    let tickets = w.engine.tickets().unwrap();
    let latest = tickets.iter().find(|t| t.ticket_key == "SIS-244").unwrap();
    assert_eq!((latest.run_id.as_str(), latest.working), (newer.id.as_str(), 1));
}

#[test]
#[ignore]
fn live_team() {
    let dir = tempfile::tempdir().unwrap();
    let workflows = dir.path().join("workflows");
    workflow::load_all(&workflows).unwrap();
    std::fs::write(
        workflows.join("pair.yaml"),
        "name: Pair\nagents:\n  lead:\n    model: claude-haiku-5-5\n    ask: never\n    tools: [Read]\n  helper:\n    model: claude-haiku-5-5\n    ask: never\n    tools: [Read]\n    instructions: The secret word is TEAL. Tell it to whoever asks you for it on the board.\nsteps:\n- id: pair\n  team:\n    members: [lead, helper]\n    lead: lead\n    goal: The lead needs the secret word, which only the helper knows. Get it over the board, then finish with the word as the summary.\n    maxTurns: 15\n    budgetUsd: 0.3\n    minutes: 5\n",
    )
    .unwrap();
    let host = crate::host::Host::new();
    let engine = Arc::new(Engine::new(Db::open(&dir.path().join("s.sqlite")).unwrap(), Box::new(HostSink(host.clone())), workflows));
    let handler = engine.clone();
    host.set_handler(Arc::new(move |event| handler.on_host(event)));
    let claude = crate::host::find_claude().expect("claude.exe");
    host.clone().supervise(crate::host::dev_launch(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."), &claude));
    let wait = |what: &str, done: &dyn Fn() -> bool, secs: u64| {
        for _ in 0..secs * 2 {
            if done() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        panic!("timed out waiting for {what}");
    };
    wait("the agent host", &|| host.is_ready(), 60);
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();

    let run = engine
        .start_run(StartRun {
            workflow: "pair".into(),
            ticket: Some(TicketRef { key: "SIS-1".into(), title: "Live team".into(), url: "u".into() }),
            folder: dir.path().display().to_string(),
            started_by: "test".into(),
            request: None,
        })
        .unwrap();
    wait("the run to end", &|| !matches!(engine.run(&run.id).unwrap().state.as_str(), "running" | "waiting"), 300);
    let view = engine.run(&run.id).unwrap();
    for n in &view.notes {
        println!("NOTE {} → {:?}: {}", n.author, n.to, n.body);
    }
    for j in &view.jobs {
        println!("JOB {} {} {} {:?} {:?} ${:?}", j.kind, j.step_id, j.state, j.output.as_deref().map(|o| &o[..o.len().min(80)]), j.error, j.cost_usd);
    }
    host.shutdown();
    assert_eq!(view.state, "passed");
    let team = view.jobs.iter().find(|j| j.kind == "team").unwrap();
    assert!(team.output.as_deref().unwrap_or_default().to_uppercase().contains("TEAL"));
}

// ---- Launching another workflow ----

const OUTER: &str = "name: Outer
steps:
- id: plan
  agent: planner
- id: go
  launch: inner
  input: 'Do {{ steps.plan.output }}'
- id: after
  agent: builder
  input: steps.go.output
";

const INNER: &str = "name: Inner
steps:
- id: work
  agent: builder
  instructions: 'Work on: {{ request }}'
";

impl World {
    /// Starts Outer and ends its plan, so Inner runs: (outer run, inner run, inner's job)
    fn launched(&self) -> (RunView, RunView, String) {
        self.with("outer", OUTER);
        self.with("inner", INNER);
        let run = self.start_wf("outer");
        let (plan, _) = self.last_start();
        self.end(&plan, "the thing");
        let (work, _) = self.last_start();
        let child = self.run(&run.id).jobs.iter().find(|j| j.kind == "launch").unwrap().launched_run.clone().unwrap();
        (self.run(&run.id), self.run(&child), work)
    }

    fn launch_job(&self, run: &str) -> JobViewLite {
        let j = self.run(run).jobs.into_iter().rev().find(|j| j.kind == "launch").unwrap();
        JobViewLite { id: j.id, state: j.state, error: j.error, output: j.output }
    }
}

struct JobViewLite {
    id: String,
    state: String,
    error: Option<String>,
    output: Option<String>,
}

#[test]
fn a_launch_step_runs_another_workflow_on_the_ticket_and_gets_its_result() {
    let w = world();
    let (run, child, work) = w.launched();
    let (_, prompt) = w.last_start();
    assert!(prompt.contains("Work on: Do the thing") && prompt.contains("## What was asked\n\nDo the thing"));
    assert_eq!((child.workflow_id.as_str(), child.state.as_str(), child.ticket.as_ref().unwrap().key.as_str()), ("inner", "running", "SIS-245"));
    assert_eq!((child.parent_run.as_deref(), child.started_by.as_str()), (Some(run.id.as_str()), "Outer #1"));
    assert_eq!(w.launch_job(&run.id).state, "running");

    // Tracket sees the outer run, with the inner run's work
    let tickets = w.engine.tickets().unwrap();
    assert_eq!((tickets.len(), tickets[0].run_id.as_str(), tickets[0].working), (1, run.id.as_str(), 1));

    w.end(&work, "done it");
    assert_eq!(w.run(&child.id).state, "passed");
    assert_eq!(w.launch_job(&run.id).output.as_deref(), Some("done it"));
    let (after, prompt) = w.last_start();
    assert!(prompt.contains("## Input from earlier steps\n\ndone it"));
    w.end(&after, "ok");
    assert_eq!(w.run(&run.id).state, "passed");
}

#[test]
fn a_failed_launched_run_fails_its_step_and_a_retry_goes_on_with_that_run() {
    let w = world();
    let (run, child, work) = w.launched();
    w.fail(&work);
    assert_eq!((w.run(&child.id).state.as_str(), w.run(&run.id).state.as_str()), ("failed", "failed"));
    let go = w.launch_job(&run.id);
    assert_eq!((go.state.as_str(), go.error.as_deref()), ("failed", Some("Inner #1 failed")));
    // Tracket offers the step that failed down the chain
    assert_eq!(w.engine.tickets().unwrap()[0].failed_job_id.as_deref(), Some(work.as_str()));

    w.engine.retry_job(&go.id).unwrap();
    assert_eq!((w.run(&child.id).state.as_str(), w.run(&run.id).state.as_str()), ("running", "running"));
    assert_eq!(w.launch_job(&run.id).state, "running");
    assert_eq!(w.engine.runs(None).unwrap().len(), 2, "no second Inner run");
    let (again, _) = w.last_start();
    assert_ne!(again, work);
    w.end(&again, "fixed");
    let (_, prompt) = w.last_start();
    assert!(prompt.contains("## Input from earlier steps\n\nfixed"));
    assert_eq!(w.launch_job(&run.id).state, "done");
}

#[test]
fn cancelling_stops_the_launched_run_and_a_launched_run_cancelled_alone_fails_its_step() {
    let w = world();
    let (run, child, work) = w.launched();
    w.engine.cancel_run(&run.id, "Luc").unwrap();
    assert_eq!((w.run(&child.id).state.as_str(), w.run(&run.id).state.as_str()), ("cancelled", "cancelled"));
    assert!(w.stopped(&work));

    let w = world();
    let (run, child, _) = w.launched();
    w.engine.cancel_run(&child.id, "Luc").unwrap();
    assert_eq!(w.run(&run.id).state, "failed");
    let go = w.launch_job(&run.id);
    assert_eq!(go.error.as_deref(), Some("Inner #1 was cancelled"));
    // Nothing to go on with: a retry launches it again
    w.engine.retry_job(&go.id).unwrap();
    let runs = w.engine.runs(None).unwrap();
    assert_eq!(runs.len(), 3);
    assert_eq!((runs[0].workflow_id.as_str(), runs[0].number, runs[0].state.as_str()), ("inner", 2, "running"));
}

#[test]
fn the_host_going_away_interrupts_the_launched_run_and_the_one_waiting_on_it() {
    let w = world();
    let (run, child, work) = w.launched();
    w.engine.on_host(HostEvent::Exited);
    assert_eq!((w.run(&child.id).state.as_str(), w.run(&run.id).state.as_str()), ("interrupted", "interrupted"));
    assert_eq!(w.launch_job(&run.id).state, "interrupted");

    w.engine.retry_job(&work).unwrap();
    assert_eq!((w.run(&child.id).state.as_str(), w.run(&run.id).state.as_str()), ("running", "running"));
    let (again, _) = w.last_start();
    w.end(&again, "done");
    assert_eq!(w.run(&child.id).state, "passed");
    assert_eq!(w.launch_job(&run.id).state, "done");
}

#[test]
fn a_workflow_can_t_launch_itself_or_one_already_up_its_chain() {
    let own = crate::workflow::parse("selfish", "x", "name: S\nsteps:\n- id: a\n  launch: selfish\n", &Default::default());
    assert!(own.unwrap_err().contains("can't launch itself"));

    let w = world();
    w.with("ping", "name: Ping\nsteps:\n- id: go\n  launch: pong\n");
    w.with("pong", "name: Pong\nsteps:\n- id: back\n  launch: ping\n");
    let run = w.start_wf("ping");
    assert_eq!(w.run(&run.id).state, "failed");
    let runs = w.engine.runs(None).unwrap();
    let pong = runs.iter().find(|r| r.workflow_id == "pong").unwrap();
    let back = pong.jobs.iter().find(|j| j.kind == "launch").unwrap();
    assert_eq!(back.error.as_deref(), Some("Ping is already running further up this chain: it would launch itself"));
    assert_eq!(w.launch_job(&run.id).error.as_deref(), Some("Pong #1 failed"));

    w.with("lost", "name: Lost\nsteps:\n- id: go\n  launch: nowhere\n");
    let run = w.start_wf("lost");
    assert_eq!(w.launch_job(&run.id).error.as_deref(), Some("There is no workflow `nowhere`, or its file has a problem"));
}

// ---- Launching on another ticket, and waiting for room ----

impl World {
    fn start_in(&self, id: &str, folder: &str) -> RunView {
        self.engine
            .start_run(StartRun {
                workflow: id.into(),
                ticket: Some(TicketRef { key: "SIS-300".into(), title: "The story".into(), url: "https://x/browse/SIS-300".into() }),
                folder: folder.into(),
                started_by: "Luc".into(),
                request: None,
            })
            .unwrap()
    }
}

const SUBTASKS: &str = "name: Subtasks
steps:
- id: list
  agent: planner
  output: { subtasks: list }
- id: each
  forEach: steps.list.output.subtasks
  as: subtask
  parallel: 2
  steps:
  - id: do
    launch: inner
    ticket: '{{ subtask }}'
    input: 'Do {{ subtask }}'
";

#[test]
fn a_launch_step_on_another_ticket_runs_in_that_tickets_worktree() {
    let w = world();
    w.with("subtasks", SUBTASKS);
    w.with("inner", &format!("{INNER}maxParallelRuns: 5\n"));
    let repo = crate::worktree::cloned_repo(std::path::Path::new(&w.folder));
    let run = w.start_in("subtasks", &repo.to_string_lossy());
    let (list, _) = w.last_start();
    w.end_with(&list, json!({"subtasks": ["SIS-301 Add the field", "SIS-302"]}));

    let runs = w.engine.runs(None).unwrap();
    let sub = runs.iter().find(|r| r.ticket.as_ref().unwrap().key == "SIS-301").expect("a run on SIS-301");
    assert_eq!((sub.ticket.as_ref().unwrap().title.as_str(), sub.ticket.as_ref().unwrap().url.as_str()), ("Add the field", "https://x/browse/SIS-301"));
    assert!(std::path::Path::new(&sub.folder).ends_with(std::path::Path::new(".claude/worktrees/SIS-301")));
    assert_eq!(sub.request.as_deref(), Some("Do SIS-301 Add the field"));
    let other = runs.iter().find(|r| r.ticket.as_ref().unwrap().key == "SIS-302").unwrap();
    assert_eq!(other.ticket.as_ref().unwrap().title, "SIS-302");

    // Each subtask is its own row in Tracket, besides the story's
    let mut keys: Vec<_> = w.engine.tickets().unwrap().into_iter().map(|t| t.ticket_key).collect();
    keys.sort();
    assert_eq!(keys, ["SIS-300", "SIS-301", "SIS-302"]);

    for r in [sub, other] {
        let job = r.jobs.iter().find(|j| j.kind == "agent").unwrap().id.clone();
        w.end(&job, "done");
    }
    assert_eq!(w.run(&run.id).state, "passed");
}

#[test]
fn a_launch_step_on_a_bad_ticket_or_without_a_repository_fails() {
    let w = world();
    w.with("one", "name: One\nsteps:\n- id: go\n  launch: inner\n  ticket: 'not a key'\n");
    w.with("inner", INNER);
    let run = w.start_wf("one");
    assert_eq!(w.launch_job(&run.id).error.as_deref(), Some("The ticket \"not a key\" doesn't start with a key like ABC-123"));

    // The test folder has a `.git` folder, but isn't a repository: no worktree can be made
    w.with("two", "name: Two\nsteps:\n- id: go\n  launch: inner\n  ticket: SIS-999\n");
    let run = w.start_wf("two");
    assert!(w.launch_job(&run.id).error.unwrap().starts_with("The ticket's worktree:"));
    assert_eq!(w.run(&run.id).state, "failed");
}

#[test]
fn a_launch_step_waits_for_room_at_its_workflows_limit() {
    let w = world();
    w.with("inner", INNER); // one run at a time
    w.with("many", "name: Many\nsteps:\n- id: list\n  agent: planner\n  output: { items: list }\n- id: each\n  forEach: steps.list.output.items\n  parallel: 2\n  steps:\n  - id: do\n    launch: inner\n    input: '{{ item }}'\n");
    let run = w.start_wf("many");
    let (list, _) = w.last_start();
    w.end_with(&list, json!({"items": ["a", "b"]}));
    let states: Vec<_> = w.run(&run.id).jobs.iter().filter(|j| j.kind == "launch").map(|j| j.state.clone()).collect();
    assert_eq!(states, ["running", "queued"]);
    assert_eq!(w.run(&run.id).state, "running");

    let (first, prompt) = w.last_start();
    assert!(prompt.contains("Work on: a"));
    w.end(&first, "a done");
    // Room again: the second one starts
    let (second, prompt) = w.last_start();
    assert!(prompt.contains("Work on: b"));
    w.end(&second, "b done");
    let view = w.run(&run.id);
    assert_eq!(view.state, "passed");
    let lp = view.jobs.iter().find(|j| j.kind == "loop").unwrap();
    assert_eq!(lp.output.as_deref(), Some(r#"["a done","b done"]"#));

    // Cancelled while it waits: nothing starts later
    let run = w.start_wf("many");
    let (list, _) = w.last_start();
    w.end_with(&list, json!({"items": ["c", "d"]}));
    w.engine.cancel_run(&run.id, "Luc").unwrap();
    let states: Vec<_> = w.run(&run.id).jobs.iter().filter(|j| j.kind == "launch").map(|j| j.state.clone()).collect();
    assert_eq!(states, ["stopped", "stopped"]);
}

// ---- Triggers, and runs with no ticket ----

const NIGHTLY: &str = "name: Nightly
triggers:
- schedule: 0 2 * * *
  folder: FOLDER
  request: Check the dependencies
steps:
- id: check
  agent: planner
- id: tell
  agent: notifier
  action:
    do: comment
    text: Done
";

const FOLLOW: &str = "name: Follow
triggers:
- after: fix-a-bug
  when: ended
- webhook: true
  folder: FOLDER
steps:
- id: look
  agent: planner
";

impl World {
    fn write(&self, id: &str, text: &str) {
        // A plain YAML scalar keeps its backslashes
        let text = text.replace("FOLDER", &self.folder);
        std::fs::write(std::path::Path::new(&self.folder).join("workflows").join(format!("{id}.yaml")), text).unwrap();
    }

    fn runs_of(&self, workflow: &str) -> Vec<RunView> {
        self.engine.runs(None).unwrap().into_iter().filter(|r| r.workflow_id == workflow).collect()
    }
}

fn local(h: u32, m: u32, s: u32) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local.with_ymd_and_hms(2026, 11, 27, h, m, s).unwrap()
}

#[test]
fn a_schedule_starts_a_run_on_its_folder_once_with_no_ticket() {
    let w = world();
    w.write("nightly", NIGHTLY);
    // First look: it counts from now
    w.engine.tick_schedules(local(1, 59, 30)).unwrap();
    assert!(w.runs_of("nightly").is_empty());
    w.engine.tick_schedules(local(2, 0, 5)).unwrap();
    w.engine.tick_schedules(local(2, 0, 20)).unwrap();
    let runs = w.runs_of("nightly");
    assert_eq!(runs.len(), 1, "started once");
    let run = &runs[0];
    assert_eq!((run.ticket.is_none(), run.started_by.as_str(), run.request.as_deref()), (true, "Schedule", Some("Check the dependencies")));
    let (job, prompt) = w.last_start();
    assert!(prompt.starts_with(&format!("The repository at {}. This run has no Jira ticket.", run.folder)), "{prompt}");
    assert!(!prompt.contains("Jira ticket "));

    // A comment on the ticket has nothing to go to
    w.end(&job, "all fine");
    let run = w.run(&run.id);
    assert_eq!(run.state, "failed");
    assert_eq!(run.jobs.last().unwrap().error.as_deref(), Some("This run has no Jira ticket to comment on or move"));
    // Tracket sees no ticket for it
    assert!(w.engine.tickets().unwrap().is_empty());
    // The editor sees what it started, and when it runs next
    let status = &w.engine.trigger_statuses("nightly").unwrap()[0];
    assert_eq!((status.run_number, status.error.as_deref(), status.next.len()), (Some(1), None, 3));
}

#[test]
fn a_schedule_missed_while_sprocket_was_closed_is_skipped_and_a_bad_folder_is_reported() {
    let w = world();
    w.write("nightly", NIGHTLY);
    w.engine.tick_schedules(local(1, 0, 0)).unwrap();
    w.engine.tick_schedules(local(9, 0, 0)).unwrap();
    assert!(w.runs_of("nightly").is_empty(), "not caught up on");

    w.write("nightly", &NIGHTLY.replace("FOLDER", "C:\\nowhere\\at-all"));
    w.engine.tick_schedules(local(1, 0, 0)).unwrap();
    w.engine.tick_schedules(local(2, 0, 30)).unwrap();
    let status = &w.engine.trigger_statuses("nightly").unwrap()[0];
    assert!(status.error.as_deref().is_some_and(|e| e.contains("doesn't exist")), "{status:?}");
}

#[test]
fn a_workflow_set_to_run_after_another_starts_on_its_ticket_and_folder() {
    let w = world();
    w.write("follow", FOLLOW);
    let run = w.start();
    let (plan, _) = w.last_start();
    // A failed run counts with `when: ended`
    w.host(json!({"type": "job.ended", "jobId": plan, "result": "error", "error": "boom"}));
    assert_eq!(w.run(&run.id).state, "failed");
    let follow = w.runs_of("follow");
    assert_eq!(follow.len(), 1);
    assert_eq!(
        (follow[0].ticket.as_ref().map(|t| t.key.as_str()), follow[0].folder.as_str(), follow[0].started_by.as_str()),
        (Some("SIS-244"), w.run(&run.id).folder.as_str(), "After Fix a bug #1")
    );
    assert!(w.engine.activity(50).unwrap().iter().any(|a| a.what == "trigger.started" && a.detail.as_deref() == Some("Follow #1")));
    // It is now the ticket's latest run
    assert_eq!(w.engine.tickets().unwrap()[0].run_id, follow[0].id);

    // Retried, the run ends again: it doesn't start a second one
    w.engine.retry_job(&w.run(&run.id).jobs[0].id).unwrap();
    let (plan, _) = w.last_start();
    w.end(&plan, "1. change foo");
    let gate = w.run(&run.id).approvals[0].id.clone();
    w.engine.decide(&gate, true, None, "Luc").unwrap();
    let (build, _) = w.last_start();
    w.end(&build, "built");
    assert_eq!((w.run(&run.id).state.as_str(), w.runs_of("follow").len()), ("passed", 1));
}

#[test]
fn after_triggers_never_loop_and_only_follow_the_end_they_name() {
    let w = world();
    let one_step = |name: &str, after: &str| format!("name: {name}
triggers:
- after: {after}
steps:
- id: go
  agent: planner
");
    w.write("ping", &one_step("Ping", "pong"));
    w.write("pong", &one_step("Pong", "ping"));

    // Pong follows a pass only
    w.engine.run_now("ping", &w.folder, None).unwrap();
    let (job, _) = w.last_start();
    w.host(json!({"type": "job.ended", "jobId": job, "result": "error", "error": "boom"}));
    assert!(w.runs_of("pong").is_empty());

    // Ping passes, so pong runs; pong passes, and ping would run again: a loop, skipped
    w.engine.run_now("ping", &w.folder, None).unwrap();
    let (job, _) = w.last_start();
    w.end(&job, "ok");
    let pong = w.runs_of("pong");
    assert_eq!((pong.len(), pong[0].started_by.as_str()), (1, "After Ping #2"));
    let (job, _) = w.last_start();
    w.end(&job, "ok");
    assert_eq!((w.runs_of("ping").len(), w.run(&pong[0].id).state.as_str()), (2, "passed"));
    let skipped = w.engine.activity(100).unwrap().into_iter().find(|a| a.what == "trigger.skipped").expect("skipped");
    assert!(skipped.detail.as_deref().unwrap_or_default().starts_with("Ping: it already ran in this chain"));
}

#[test]
fn a_webhook_starts_only_a_workflow_that_has_one_in_its_folder() {
    let w = world();
    w.write("follow", FOLLOW);
    let run = w.engine.start_from_webhook("follow", Some("Look at the logs"), None).unwrap();
    assert_eq!((run.started_by.as_str(), run.ticket.is_none(), run.request.as_deref()), ("Webhook", true, Some("Look at the logs")));
    let e = w.engine.start_from_webhook("fix-a-bug", None, None).unwrap_err();
    assert_eq!(e.kind, "notFound");
    let long = "x".repeat(20_001);
    assert_eq!(w.engine.start_from_webhook("follow", Some(&long), None).unwrap_err().kind, "invalid");
}

#[test]
fn run_now_starts_a_run_on_a_repository_with_no_ticket() {
    let w = world();
    let run = w.engine.run_now("fix-a-bug", &w.folder, None).unwrap();
    assert_eq!((run.ticket.is_none(), run.started_by.as_str()), (true, "You, in Sprocket"));
    assert_eq!(w.engine.run_now("fix-a-bug", "relative", None).unwrap_err().kind, "invalid");
}
