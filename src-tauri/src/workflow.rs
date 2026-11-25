//! Workflows are YAML files in `<data dir>/workflows`. See workflows/fix-a-bug.yaml for the format.
//! A file that can't be read is reported with why, and the others still load.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The example copied into an empty workflows folder.
pub const EXAMPLE: (&str, &str) = ("fix-a-bug.yaml", include_str!("../../workflows/fix-a-bug.yaml"));

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AskPolicy {
    Never,
    BeforeWrites,
    EveryAction,
}

// ---- The file as written ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileWorkflow {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    offer_in_tracket: bool,
    #[serde(default = "one")]
    max_parallel_runs: u32,
    #[serde(default)]
    agents: BTreeMap<String, FileAgent>,
    steps: Vec<FileStep>,
}

fn one() -> u32 {
    1
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileAgent {
    model: String,
    ask: AskPolicy,
    #[serde(default)]
    tools: Vec<String>,
    /// MCP servers the agent may use, by their name in Claude Code; none when left out
    #[serde(default)]
    mcp: Vec<String>,
    instructions: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileStep {
    id: String,
    agent: Option<String>,
    approval: Option<FileApproval>,
    instructions: Option<String>,
    input: Option<String>,
    on_reject: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileApproval {
    summary: Option<String>,
}

// ---- What the rest of the app uses ----

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentType {
    pub id: String,
    pub model: String,
    pub ask: AskPolicy,
    pub tools: Vec<String>,
    #[serde(default)]
    pub mcp: Vec<String>,
    pub instructions: Option<String>,
}

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
    pub agents: Vec<AgentType>,
    pub steps: Vec<Step>,
}

#[derive(Serialize, Debug)]
pub struct Problem {
    pub file: String,
    pub message: String,
}

#[derive(Serialize, Default, Debug)]
pub struct WorkflowList {
    pub workflows: Vec<Workflow>,
    pub problems: Vec<Problem>,
}

pub fn parse(id: &str, file: &str, text: &str) -> Result<Workflow, String> {
    let w: FileWorkflow = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    if w.name.trim().is_empty() {
        return Err("`name` is empty".into());
    }
    if w.steps.is_empty() {
        return Err("A workflow needs at least one step".into());
    }

    let mut seen = HashSet::new();
    let mut steps = Vec::with_capacity(w.steps.len());
    for s in w.steps {
        if !seen.insert(s.id.clone()) {
            return Err(format!("Two steps are called `{}`", s.id));
        }
        let (kind, input) = match (&s.agent, &s.approval) {
            (Some(agent), None) => {
                if !w.agents.contains_key(agent) {
                    return Err(format!("Step `{}` uses agent `{agent}`, which `agents` doesn't define", s.id));
                }
                (StepKind::Agent, s.input)
            }
            (None, Some(approval)) => (StepKind::Approval, approval.summary.clone()),
            _ => return Err(format!("Step `{}` needs either `agent` or `approval`", s.id)),
        };
        if let Some(target) = &s.on_reject {
            // Only backwards: a rejection sends the run back to redo an earlier step
            if target == &s.id || !seen.contains(target) {
                return Err(format!("Step `{}`: `onReject` must name an earlier step", s.id));
            }
        }
        steps.push(Step {
            id: s.id,
            kind,
            agent: s.agent,
            instructions: s.instructions,
            input,
            on_reject: s.on_reject,
        });
    }

    let agents = w
        .agents
        .into_iter()
        .map(|(id, a)| AgentType { id, model: a.model, ask: a.ask, tools: a.tools, mcp: a.mcp, instructions: a.instructions })
        .collect();

    Ok(Workflow {
        id: id.to_string(),
        name: w.name,
        description: w.description,
        offer_in_tracket: w.offer_in_tracket,
        max_parallel_runs: w.max_parallel_runs.max(1),
        file: file.to_string(),
        agents,
        steps,
    })
}

/// Reads every `*.yaml` in `dir`, sorted by name. The folder is made, with the example in it, when missing.
pub fn load_all(dir: &Path) -> std::io::Result<WorkflowList> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(EXAMPLE.0), EXAMPLE.1)?;
    }
    let mut paths: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yaml" || x == "yml"))
        .collect();
    paths.sort();

    let mut list = WorkflowList::default();
    for path in paths {
        let file = path.display().to_string();
        let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| parse(&id, &file, &text));
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

    #[test]
    fn the_example_parses() {
        let w = parse("fix-a-bug", "x", EXAMPLE.1).unwrap();
        assert_eq!(w.name, "Fix a bug");
        assert!(w.offer_in_tracket);
        let kinds: Vec<_> = w.steps.iter().map(|s| (s.id.as_str(), s.kind)).collect();
        assert_eq!(kinds, [("plan", StepKind::Agent), ("approve", StepKind::Approval), ("build", StepKind::Agent)]);
        assert_eq!(w.steps[1].on_reject.as_deref(), Some("plan"));
        assert_eq!(w.agents.iter().find(|a| a.id == "builder").unwrap().ask, AskPolicy::BeforeWrites);
        assert_eq!(w.agents.iter().find(|a| a.id == "planner").unwrap().mcp, ["Atlassian2"]);
        assert!(w.agents.iter().find(|a| a.id == "builder").unwrap().mcp.is_empty());
    }

    fn err(yaml: &str) -> String {
        parse("t", "t.yaml", yaml).unwrap_err()
    }

    #[test]
    fn rejects_broken_workflows() {
        assert!(err("name: x\nsteps: []").contains("at least one step"));
        assert!(err("name: x\nsteps:\n  - id: a\n    agent: ghost").contains("doesn't define"));
        assert!(err("name: x\nsteps:\n  - id: a").contains("either `agent` or `approval`"));
        assert!(err("name: x\nsteps:\n  - id: a\n    approval: {}\n  - id: a\n    approval: {}").contains("Two steps"));
        assert!(err("name: x\nsteps:\n  - id: a\n    approval: {}\n    onReject: b\n  - id: b\n    approval: {}")
            .contains("earlier step"));
        assert!(err("name: x\nsurprise: 1\nsteps:\n  - id: a\n    approval: {}").contains("unknown field"));
    }

    #[test]
    fn an_empty_folder_gets_the_example_and_bad_files_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("workflows");
        let first = load_all(&folder).unwrap();
        assert_eq!(first.workflows.len(), 1);

        std::fs::write(folder.join("broken.yaml"), "name: [").unwrap();
        let second = load_all(&folder).unwrap();
        assert_eq!((second.workflows.len(), second.problems.len()), (1, 1));
        assert!(second.problems[0].file.ends_with("broken.yaml"));
    }
}
