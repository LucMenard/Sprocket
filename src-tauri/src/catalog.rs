//! Editing workflows and agent types: the editors send what they show, this checks it with the same rules
//! as loading, then writes the YAML file. A file is replaced in one step (written beside it, then moved),
//! and nothing is written when the check fails.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::agent::{self, AgentType, AskPolicy, FileAgent};
use crate::error::{AppError, Result};
use crate::workflow::{self, FileApproval, FileStep, FileWorkflow, Step, Workflow};

/// Where the two kinds of file live.
pub struct Dirs {
    pub workflows: PathBuf,
    pub agents: PathBuf,
}

impl Dirs {
    pub fn new(workflows: &Path) -> Self {
        Self { workflows: workflows.to_path_buf(), agents: agent::dir_next_to(workflows) }
    }
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new("invalid", message)
}

fn write_file(path: &Path, text: &str) -> Result<()> {
    let temp = path.with_extension(format!("yaml.{}.tmp", std::process::id()));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

fn yaml<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_yaml::to_string(value).map_err(|e| AppError::new("io", e.to_string()))
}

fn shared(dirs: &Dirs) -> Result<std::collections::BTreeMap<String, AgentType>> {
    Ok(agent::load_all(&dirs.agents)?.by_id())
}

/// `My workflow!` → `my-workflow`, made unique in `dir` with `-2`, `-3`…
fn new_id(name: &str, dir: &Path) -> Result<String> {
    let mut base: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    base.truncate(48);
    if base.is_empty() {
        return Err(invalid("Give it a name with letters or digits"));
    }
    let mut id = base.clone();
    let mut n = 2;
    while dir.join(format!("{id}.yaml")).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    Ok(id)
}

// ---- Workflows ----

/// A workflow as its editor shows it. Its own agent types (if any) are kept from the file.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDraft {
    pub name: String,
    pub description: String,
    pub offer_in_tracket: bool,
    pub max_parallel_runs: u32,
    #[serde(default = "yes")]
    pub board: bool,
    /// The steps as the editor has them; checked again before anything is written
    pub steps: Vec<Step>,
    #[serde(default)]
    pub layout: std::collections::BTreeMap<String, workflow::Point>,
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

fn workflow_path(dirs: &Dirs, id: &str) -> Result<PathBuf> {
    if !agent::valid_id(id) {
        return Err(invalid("Not a workflow name"));
    }
    Ok(dirs.workflows.join(format!("{id}.yaml")))
}

fn read_workflow_file(path: &Path) -> Result<FileWorkflow> {
    let text = std::fs::read_to_string(path).map_err(|_| AppError::new("notFound", "No such workflow"))?;
    serde_yaml::from_str(&text).map_err(|e| invalid(format!("The file can't be read: {e}")))
}

fn to_file(draft: WorkflowDraft, agents: std::collections::BTreeMap<String, FileAgent>) -> FileWorkflow {
    let steps = draft.steps.iter().map(Step::to_file).collect();
    FileWorkflow {
        name: draft.name.trim().to_string(),
        description: draft.description.trim().to_string(),
        offer_in_tracket: draft.offer_in_tracket,
        max_parallel_runs: draft.max_parallel_runs.max(1),
        board: draft.board,
        agents,
        steps,
        layout: draft.layout,
    }
}

/// Checks and writes `file` as workflow `id`.
fn put_workflow(dirs: &Dirs, id: &str, file: FileWorkflow) -> Result<Workflow> {
    let path = workflow_path(dirs, id)?;
    let text = yaml(&file)?;
    let checked = workflow::parse(id, &path.display().to_string(), &text, &shared(dirs)?).map_err(invalid)?;
    write_file(&path, &text)?;
    Ok(checked)
}

pub fn save_workflow(dirs: &Dirs, id: &str, draft: WorkflowDraft) -> Result<Workflow> {
    let current = read_workflow_file(&workflow_path(dirs, id)?)?;
    put_workflow(dirs, id, to_file(draft, current.agents))
}

/// A new workflow with one step: the first shared agent type's, or an approval when there is none.
pub fn create_workflow(dirs: &Dirs, name: &str) -> Result<Workflow> {
    std::fs::create_dir_all(&dirs.workflows)?;
    let id = new_id(name, &dirs.workflows)?;
    let step = match shared(dirs)?.keys().next() {
        Some(agent) => FileStep { id: "work".into(), agent: Some(agent.clone()), ..Default::default() },
        None => FileStep { id: "approve".into(), approval: Some(FileApproval::default()), ..Default::default() },
    };
    let file = FileWorkflow {
        name: name.trim().to_string(),
        description: String::new(),
        offer_in_tracket: false,
        max_parallel_runs: 1,
        board: true,
        layout: Default::default(),
        agents: Default::default(),
        steps: vec![step],
    };
    put_workflow(dirs, &id, file)
}

pub fn delete_workflow(dirs: &Dirs, id: &str) -> Result<()> {
    std::fs::remove_file(workflow_path(dirs, id)?).map_err(|_| AppError::new("notFound", "No such workflow"))
}

// ---- Agent types ----

/// An agent type as its editor shows it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDraft {
    pub description: Option<String>,
    pub model: String,
    pub ask: AskPolicy,
    pub tools: Vec<String>,
    pub mcp: Vec<String>,
    pub instructions: Option<String>,
    #[serde(default = "yes")]
    pub board: bool,
}

fn yes() -> bool {
    true
}

fn agent_path(dirs: &Dirs, id: &str) -> Result<PathBuf> {
    if !agent::valid_id(id) {
        return Err(invalid("An agent type's name is lowercase letters, digits, - or _"));
    }
    Ok(dirs.agents.join(format!("{id}.yaml")))
}

fn put_agent(dirs: &Dirs, id: &str, file: &FileAgent) -> Result<AgentType> {
    let path = agent_path(dirs, id)?;
    let text = yaml(file)?;
    let checked = agent::parse(id, &text).map_err(invalid)?;
    std::fs::create_dir_all(&dirs.agents)?;
    write_file(&path, &text)?;
    Ok(checked)
}

fn draft_file(draft: AgentDraft) -> FileAgent {
    let clean = |list: Vec<String>| {
        let mut list: Vec<String> = list.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        list.dedup();
        list
    };
    FileAgent {
        description: non_empty(draft.description),
        model: draft.model.trim().to_string(),
        ask: draft.ask,
        tools: clean(draft.tools),
        mcp: clean(draft.mcp),
        instructions: non_empty(draft.instructions),
        board: draft.board,
    }
}

/// Writes a shared agent type; `create` refuses to replace one that exists.
pub fn save_agent(dirs: &Dirs, id: &str, draft: AgentDraft, create: bool) -> Result<AgentType> {
    if create && agent_path(dirs, id)?.exists() {
        return Err(AppError::new("conflict", format!("An agent type called `{id}` exists already")));
    }
    if !create && !agent_path(dirs, id)?.exists() {
        return Err(AppError::new("notFound", "No such agent type"));
    }
    put_agent(dirs, id, &draft_file(draft))
}

/// The workflows whose steps use the shared agent type `id` (and don't define their own).
pub fn workflows_using(dirs: &Dirs, id: &str) -> Result<Vec<String>> {
    let list = workflow::load_all(&dirs.workflows)?;
    Ok(list
        .workflows
        .into_iter()
        .filter(|w| w.agents.iter().any(|a| a.id == id && a.shared))
        .map(|w| w.name)
        .collect())
}

pub fn delete_agent(dirs: &Dirs, id: &str) -> Result<()> {
    let users = workflows_using(dirs, id)?;
    if !users.is_empty() {
        return Err(AppError::new("conflict", format!("Used by {}: change their steps first", users.join(", "))));
    }
    std::fs::remove_file(agent_path(dirs, id)?).map_err(|_| AppError::new("notFound", "No such agent type"))
}

/// Moves an agent type defined in a workflow file to the shared ones. When a shared one of that name
/// exists already, it must do the same thing (the description aside); the workflow then uses it.
pub fn share_agent(dirs: &Dirs, workflow_id: &str, id: &str) -> Result<AgentType> {
    let mut file = read_workflow_file(&workflow_path(dirs, workflow_id)?)?;
    let own = file.agents.remove(id).ok_or_else(|| AppError::new("notFound", "The workflow doesn't define that agent type"))?;
    let existing = shared(dirs)?.get(id).cloned();
    let result = match existing {
        Some(found) => {
            let same = FileAgent { description: None, ..found.to_file() } == FileAgent { description: None, ..own.clone() };
            if !same {
                return Err(AppError::new(
                    "conflict",
                    format!("A shared agent type called `{id}` exists and works differently: rename one of them first"),
                ));
            }
            found
        }
        None => put_agent(dirs, id, &own)?,
    };
    put_workflow(dirs, workflow_id, file)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::StepKind;

    fn dirs() -> (tempfile::TempDir, Dirs) {
        let dir = tempfile::tempdir().unwrap();
        let dirs = Dirs::new(&dir.path().join("workflows"));
        workflow::load_all(&dirs.workflows).unwrap(); // the examples
        (dir, dirs)
    }

    fn step(id: &str, kind: StepKind, agent: Option<&str>, input: Option<&str>, on_reject: Option<&str>) -> Step {
        let yaml = serde_json::json!({
            "id": id, "kind": kind, "agent": agent, "instructions": "  ", "input": input, "onReject": on_reject,
        });
        serde_json::from_value(yaml).unwrap()
    }

    #[test]
    fn saves_a_workflow_and_refuses_a_broken_one() {
        let (_d, dirs) = dirs();
        let draft = WorkflowDraft {
            name: "Fix a bug".into(),
            description: "Shorter".into(),
            offer_in_tracket: false,
            max_parallel_runs: 3,
            board: true,
            layout: Default::default(),
            steps: vec![
                step("plan", StepKind::Agent, Some("planner"), None, None),
                step("review", StepKind::Approval, None, Some("steps.plan.output"), Some("plan")),
            ],
        };
        let saved = save_workflow(&dirs, "fix-a-bug", draft).unwrap();
        assert_eq!((saved.steps.len(), saved.max_parallel_runs, saved.offer_in_tracket), (2, 3, false));
        // Blank instructions are left out of the file
        assert!(saved.steps[0].instructions.is_none());

        let broken = WorkflowDraft {
            name: "Fix a bug".into(),
            description: String::new(),
            offer_in_tracket: false,
            max_parallel_runs: 1,
            board: true,
            layout: Default::default(),
            steps: vec![step("plan", StepKind::Agent, Some("nobody"), None, None)],
        };
        assert!(save_workflow(&dirs, "fix-a-bug", broken).unwrap_err().message.contains("doesn't exist"));
        // Nothing was written: the file is still the saved one
        let again = workflow::load_all(&dirs.workflows).unwrap();
        assert_eq!(again.workflows[0].steps.len(), 2);
    }

    #[test]
    fn saves_conditions_loops_and_actions_as_the_editor_sends_them() {
        let (_d, dirs) = dirs();
        let blank = |id: &str, kind: &str| {
            serde_json::json!({
                "id": id, "kind": kind, "agent": null, "instructions": null, "input": null, "onReject": null,
                "output": [], "action": null, "if": null, "then": [], "else": [], "forEach": null, "as": null,
                "parallel": 1, "onItemFailure": "stop", "steps": [],
            })
        };
        let mut triage = blank("triage", "agent");
        triage["agent"] = "planner".into();
        triage["output"] = serde_json::json!([{ "name": "files", "type": "list" }]);
        let mut work = blank("work", "agent");
        work["agent"] = "builder".into();
        let mut check = blank("check", "condition");
        check["if"] = "true".into();
        check["then"] = serde_json::json!([work]);
        let mut each = blank("each", "forEach");
        each["forEach"] = "'one item'".into();
        each["as"] = "item".into();
        let mut inner = blank("work-2", "agent");
        inner["agent"] = "builder".into();
        inner["instructions"] = "Fix {{ item }}".into();
        each["steps"] = serde_json::json!([inner]);
        let mut tell = blank("tell", "action");
        tell["agent"] = "notifier".into();
        tell["action"] = serde_json::json!({ "do": "slack", "text": "{{ steps.triage.output.files }}", "to": "#team" });
        let draft: WorkflowDraft = serde_json::from_value(serde_json::json!({
            "name": "Rich", "description": "", "offerInTracket": false, "maxParallelRuns": 1,
            "steps": [triage, check, each, tell],
        }))
        .unwrap();
        let saved = save_workflow(&dirs, "fix-a-bug", draft).unwrap();
        let kinds: Vec<_> = saved.steps.iter().map(|s| s.kind).collect();
        assert_eq!(kinds, [StepKind::Agent, StepKind::Condition, StepKind::ForEach, StepKind::Action]);
        // The file keeps only what each kind uses: `as: item` and `parallel: 1` are the defaults
        let text = std::fs::read_to_string(dirs.workflows.join("fix-a-bug.yaml")).unwrap();
        assert!(!text.contains("as:") && !text.contains("parallel:") && text.contains("to: '#team'"));
        assert_eq!(saved.steps[2].item.as_deref(), Some("item"));
    }

    #[test]
    fn creates_named_workflows_and_agent_types_and_deletes_only_unused_ones() {
        let (_d, dirs) = dirs();
        let a = create_workflow(&dirs, "Review a PR!").unwrap();
        let b = create_workflow(&dirs, "Review a PR").unwrap();
        assert_eq!((a.id.as_str(), b.id.as_str()), ("review-a-pr", "review-a-pr-2"));

        let draft = || AgentDraft { description: None, model: "claude-haiku-5-5".into(), ask: AskPolicy::EveryAction, tools: vec!["Read".into()], mcp: vec![], instructions: None, board: true };
        save_agent(&dirs, "reviewer", draft(), true).unwrap();
        assert_eq!(save_agent(&dirs, "reviewer", draft(), true).unwrap_err().kind, "conflict");
        assert_eq!(save_agent(&dirs, "Bad Name", draft(), true).unwrap_err().kind, "invalid");

        // The new workflows use the first shared agent type, builder
        assert_eq!(delete_agent(&dirs, "builder").unwrap_err().kind, "conflict");
        delete_agent(&dirs, "reviewer").unwrap();
        delete_workflow(&dirs, "review-a-pr").unwrap();
    }

    #[test]
    fn moves_a_workflows_own_agent_to_the_shared_ones() {
        let (_d, dirs) = dirs();
        let yaml = "name: Own\nagents:\n  planner:\n    model: claude-opus-5-5\n    ask: never\n    tools: [Read, Grep, Glob, WebSearch]\n    mcp: [Atlassian2]\n    instructions: You plan changes. You never edit files.\n  helper:\n    model: m\n    ask: never\nsteps:\n- id: a\n  agent: planner\n- id: b\n  agent: helper\n";
        std::fs::write(dirs.workflows.join("own.yaml"), yaml).unwrap();
        // Same as the shared example but for the description: folded into it
        assert!(share_agent(&dirs, "own", "planner").unwrap().shared);
        // New: becomes a shared file
        share_agent(&dirs, "own", "helper").unwrap();
        assert!(dirs.agents.join("helper.yaml").exists());
        let own = workflow::load_all(&dirs.workflows).unwrap().workflows.into_iter().find(|w| w.id == "own").unwrap();
        assert!(own.agents.iter().all(|a| a.shared));
    }
}
