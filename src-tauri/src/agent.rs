//! Agent types: who does an agent step (model, tools, MCP servers, when to ask a human, instructions).
//! Shared ones are files in `<data dir>/agents/<id>.yaml`, used by any workflow. A workflow file may also
//! define its own under `agents:`; those win over a shared one of the same name, for that workflow only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The examples written into a new agents folder, the same agents as the example workflow's.
const EXAMPLES: [(&str, &str); 3] = [
    ("planner.yaml", include_str!("../../agents/planner.yaml")),
    ("builder.yaml", include_str!("../../agents/builder.yaml")),
    ("notifier.yaml", include_str!("../../agents/notifier.yaml")),
];

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AskPolicy {
    Never,
    BeforeWrites,
    EveryAction,
}

/// An agent type as written in a file (shared, or under a workflow's `agents:`).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileAgent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub model: String,
    pub ask: AskPolicy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// MCP servers the agent may use, by their name in Claude Code; none when left out
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Whether it may post to its run's board; it can always read it. Yes when left out
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub board: bool,
    /// Whether it may ask other agent types a question (`ask_agent`). Yes when left out
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub colleagues: bool,
}

fn yes() -> bool {
    true
}

fn is_yes(b: &bool) -> bool {
    *b
}

/// An agent type as the rest of the app sees it.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentType {
    pub id: String,
    #[serde(default)]
    pub description: Option<String>,
    pub model: String,
    pub ask: AskPolicy,
    pub tools: Vec<String>,
    #[serde(default)]
    pub mcp: Vec<String>,
    pub instructions: Option<String>,
    /// May post to the run's board (it can always read it)
    #[serde(default = "yes")]
    pub board: bool,
    /// May ask other agent types a question
    #[serde(default = "yes")]
    pub colleagues: bool,
    /// True for a shared agent type (agents/<id>.yaml), false for one defined in a workflow file
    #[serde(default)]
    pub shared: bool,
}

impl AgentType {
    pub fn from_file(id: &str, a: FileAgent, shared: bool) -> Self {
        Self {
            id: id.to_string(),
            description: a.description,
            model: a.model,
            ask: a.ask,
            tools: a.tools,
            mcp: a.mcp,
            instructions: a.instructions,
            board: a.board,
            colleagues: a.colleagues,
            shared,
        }
    }

    pub fn to_file(&self) -> FileAgent {
        FileAgent {
            description: self.description.clone().filter(|d| !d.trim().is_empty()),
            model: self.model.clone(),
            ask: self.ask,
            tools: self.tools.clone(),
            mcp: self.mcp.clone(),
            instructions: self.instructions.clone().filter(|i| !i.trim().is_empty()),
            board: self.board,
            colleagues: self.colleagues,
        }
    }
}

#[derive(Serialize, Debug)]
pub struct Problem {
    pub file: String,
    pub message: String,
}

/// A shared agent type and its file.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SharedAgent {
    #[serde(flatten)]
    pub agent: AgentType,
    pub file: String,
}

#[derive(Serialize, Default, Debug)]
pub struct AgentList {
    pub agents: Vec<SharedAgent>,
    pub problems: Vec<Problem>,
}

impl AgentList {
    pub fn by_id(&self) -> BTreeMap<String, AgentType> {
        self.agents.iter().map(|a| (a.agent.id.clone(), a.agent.clone())).collect()
    }
}

/// `<data dir>/agents`, next to `<data dir>/workflows`.
pub fn dir_next_to(workflows_dir: &Path) -> PathBuf {
    workflows_dir.with_file_name("agents")
}

/// Names of files, steps and agent types: lowercase letters, digits, `-` and `_`, starting with a letter or digit.
pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && id.len() <= 64
}

pub fn parse(id: &str, text: &str) -> Result<AgentType, String> {
    let a: FileAgent = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    if a.model.trim().is_empty() {
        return Err("`model` is empty".into());
    }
    Ok(AgentType::from_file(id, a, true))
}

/// Reads every `*.yaml` in `dir`. The folder is made, with the examples in it, when missing.
pub fn load_all(dir: &Path) -> std::io::Result<AgentList> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
        for (name, text) in EXAMPLES {
            std::fs::write(dir.join(name), text)?;
        }
    }
    let mut paths: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yaml" || x == "yml"))
        .collect();
    paths.sort();

    let mut list = AgentList::default();
    for path in paths {
        let file = path.display().to_string();
        let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let parsed = if valid_id(&id) {
            std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| parse(&id, &text))
        } else {
            Err("The file name must be lowercase letters, digits, - or _".into())
        };
        match parsed {
            Ok(agent) => list.agents.push(SharedAgent { agent, file }),
            Err(message) => list.problems.push(Problem { file, message }),
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_examples_parse_and_ids_are_checked() {
        for (name, text) in EXAMPLES {
            parse(name.trim_end_matches(".yaml"), text).unwrap();
        }
        assert!(valid_id("planner") && valid_id("code-review_2"));
        assert!(!valid_id("Planner") && !valid_id("-x") && !valid_id("a b") && !valid_id(""));
        assert!(parse("x", "model: m\nask: sometimes").is_err());
        assert!(parse("x", "model: ''\nask: never").unwrap_err().contains("empty"));
    }

    #[test]
    fn writes_back_what_it_read() {
        let a = parse("planner", EXAMPLES[0].1).unwrap();
        let again = serde_yaml::to_string(&a.to_file()).unwrap();
        assert_eq!(parse("planner", &again).unwrap().to_file(), a.to_file());
    }
}
