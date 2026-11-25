//! Workflows are YAML files in `<data dir>/workflows`. See workflows/fix-a-bug.yaml for the format.
//! Their steps use shared agent types (`<data dir>/agents`) or ones the file defines itself.
//! A file that can't be read is reported with why, and the others still load.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use crate::agent::AskPolicy;
use crate::agent::{self, AgentType, FileAgent, Problem};

/// The example copied into an empty workflows folder.
pub const EXAMPLE: (&str, &str) = ("fix-a-bug.yaml", include_str!("../../workflows/fix-a-bug.yaml"));

// ---- The file as written ----

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileWorkflow {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default)]
    pub offer_in_tracket: bool,
    #[serde(default = "one")]
    pub max_parallel_runs: u32,
    /// Agent types of this workflow only; the shared ones are in `<data dir>/agents`
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agents: BTreeMap<String, FileAgent>,
    pub steps: Vec<FileStep>,
}

fn one() -> u32 {
    1
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileStep {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<FileApproval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_reject: Option<String>,
}

#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileApproval {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

// ---- What the rest of the app uses ----

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StepKind {
    Agent,
    Approval,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: String,
    pub kind: StepKind,
    pub agent: Option<String>,
    pub instructions: Option<String>,
    /// For agent steps, what the agent gets (`steps.plan.output`); for approvals, the summary shown
    pub input: Option<String>,
    pub on_reject: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    /// The file name without `.yaml`
    pub id: String,
    pub name: String,
    pub description: String,
    pub offer_in_tracket: bool,
    pub max_parallel_runs: u32,
    pub file: String,
    /// The agent types its steps use, resolved: its own first, then the shared ones
    pub agents: Vec<AgentType>,
    pub steps: Vec<Step>,
}

#[derive(Serialize, Default, Debug)]
pub struct WorkflowList {
    pub workflows: Vec<Workflow>,
    pub problems: Vec<Problem>,
}

/// `steps.<id>.output` → `<id>`
fn referenced_step(expr: &str) -> Option<&str> {
    expr.strip_prefix("steps.").and_then(|s| s.strip_suffix(".output"))
}

/// Checks a workflow file and resolves its agents against the shared ones.
pub fn resolve(id: &str, file: &str, w: FileWorkflow, shared: &BTreeMap<String, AgentType>) -> Result<Workflow, String> {
    if w.name.trim().is_empty() {
        return Err("`name` is empty".into());
    }
    if w.steps.is_empty() {
        return Err("A workflow needs at least one step".into());
    }

    let mut seen = HashSet::new();
    let mut used = Vec::<AgentType>::new();
    let mut steps = Vec::with_capacity(w.steps.len());
    for s in w.steps {
        if !agent::valid_id(&s.id) {
            return Err(format!("Step `{}`: a step name is lowercase letters, digits, - or _", s.id));
        }
        if seen.contains(&s.id) {
            return Err(format!("Two steps are called `{}`", s.id));
        }
        let (kind, input) = match (&s.agent, &s.approval) {
            (Some(name), None) => {
                let found = match w.agents.get(name) {
                    Some(own) => AgentType::from_file(name, own.clone(), false),
                    None => shared
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Step `{}` uses the agent type `{name}`, which doesn't exist", s.id))?,
                };
                if !used.iter().any(|a| a.id == found.id) {
                    used.push(found);
                }
                (StepKind::Agent, s.input)
            }
            (None, Some(approval)) => (StepKind::Approval, approval.summary.clone()),
            _ => return Err(format!("Step `{}` needs either `agent` or `approval`", s.id)),
        };
        if let Some(step) = input.as_deref().and_then(referenced_step) {
            if !seen.contains(step) {
                return Err(format!("Step `{}` reads the output of `{step}`, which isn't an earlier step", s.id));
            }
        }
        if let Some(target) = &s.on_reject {
            // Only backwards: a rejection sends the run back to redo an earlier step
            if !seen.contains(target) {
                return Err(format!("Step `{}`: `onReject` must name an earlier step", s.id));
            }
        }
        seen.insert(s.id.clone());
        steps.push(Step { id: s.id, kind, agent: s.agent, instructions: s.instructions, input, on_reject: s.on_reject });
    }

    Ok(Workflow {
        id: id.to_string(),
        name: w.name,
        description: w.description,
        offer_in_tracket: w.offer_in_tracket,
        max_parallel_runs: w.max_parallel_runs.max(1),
        file: file.to_string(),
        agents: used,
        steps,
    })
}

pub fn parse(id: &str, file: &str, text: &str, shared: &BTreeMap<String, AgentType>) -> Result<Workflow, String> {
    let w: FileWorkflow = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    resolve(id, file, w, shared)
}

/// The workflow files in `dir`, sorted by name.
pub fn files(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yaml" || x == "yml"))
        .collect();
    paths.sort();
    Ok(paths)
}

/// Reads every workflow in `dir`, with the shared agent types next to it. The folders are made, with the
/// examples in them, when missing.
pub fn load_all(dir: &Path) -> std::io::Result<WorkflowList> {
    let shared = agent::load_all(&agent::dir_next_to(dir))?.by_id();
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(EXAMPLE.0), EXAMPLE.1)?;
    }
    let mut list = WorkflowList::default();
    for path in files(dir)? {
        let file = path.display().to_string();
        let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| parse(&id, &file, &text, &shared));
        match parsed {
            Ok(w) => list.workflows.push(w),
            Err(message) => list.problems.push(Problem { file, message }),
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> BTreeMap<String, AgentType> {
        let dir = tempfile::tempdir().unwrap();
        agent::load_all(&dir.path().join("agents")).unwrap().by_id()
    }

    #[test]
    fn the_example_parses_with_the_shared_agents() {
        let w = parse("fix-a-bug", "x", EXAMPLE.1, &shared()).unwrap();
        assert_eq!(w.name, "Fix a bug");
        assert!(w.offer_in_tracket);
        let kinds: Vec<_> = w.steps.iter().map(|s| (s.id.as_str(), s.kind)).collect();
        assert_eq!(kinds, [("plan", StepKind::Agent), ("approve", StepKind::Approval), ("build", StepKind::Agent)]);
        assert_eq!(w.steps[1].on_reject.as_deref(), Some("plan"));
        let builder = w.agents.iter().find(|a| a.id == "builder").unwrap();
        assert_eq!((builder.ask, builder.shared), (AskPolicy::BeforeWrites, true));
        assert_eq!(w.agents.iter().find(|a| a.id == "planner").unwrap().mcp, ["Atlassian2"]);
    }

    #[test]
    fn a_workflows_own_agent_wins_over_a_shared_one() {
        let yaml = "name: x\nagents:\n  planner:\n    model: own\n    ask: never\nsteps:\n- id: a\n  agent: planner\n";
        let w = parse("t", "t.yaml", yaml, &shared()).unwrap();
        assert_eq!((w.agents[0].model.as_str(), w.agents[0].shared), ("own", false));
    }

    fn err(yaml: &str) -> String {
        parse("t", "t.yaml", yaml, &shared()).unwrap_err()
    }

    #[test]
    fn rejects_broken_workflows() {
        assert!(err("name: x\nsteps: []").contains("at least one step"));
        assert!(err("name: x\nsteps:\n  - id: a\n    agent: ghost").contains("doesn't exist"));
        assert!(err("name: x\nsteps:\n  - id: a").contains("either `agent` or `approval`"));
        assert!(err("name: x\nsteps:\n  - id: a\n    approval: {}\n  - id: a\n    approval: {}").contains("Two steps"));
        assert!(err("name: x\nsteps:\n  - id: a\n    approval: {}\n    onReject: b\n  - id: b\n    approval: {}").contains("earlier step"));
        assert!(err("name: x\nsteps:\n  - id: a\n    approval: {}\n    onReject: a").contains("earlier step"));
        assert!(err("name: x\nsteps:\n  - id: A b\n    approval: {}").contains("lowercase"));
        assert!(err("name: x\nsteps:\n  - id: a\n    approval:\n      summary: steps.z.output").contains("isn't an earlier step"));
        assert!(err("name: x\nsurprise: 1\nsteps:\n  - id: a\n    approval: {}").contains("unknown field"));
    }

    #[test]
    fn an_empty_folder_gets_the_examples_and_bad_files_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("workflows");
        let first = load_all(&folder).unwrap();
        assert_eq!(first.workflows.len(), 1);
        assert!(dir.path().join("agents").join("planner.yaml").exists());

        std::fs::write(folder.join("broken.yaml"), "name: [").unwrap();
        let second = load_all(&folder).unwrap();
        assert_eq!((second.workflows.len(), second.problems.len()), (1, 1));
        assert!(second.problems[0].file.ends_with("broken.yaml"));
    }
}
