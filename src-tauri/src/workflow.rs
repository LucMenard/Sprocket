//! Workflows are YAML files in `<data dir>/workflows`. See workflows/fix-a-bug.yaml for the format.
//! Their steps use shared agent types (`<data dir>/agents`) or ones the file defines itself.
//! A file that can't be read is reported with why, and the others still load.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use crate::agent::AskPolicy;
use crate::agent::{self, AgentType, FileAgent, Problem};
use crate::expr;

/// The example copied into an empty workflows folder.
pub const EXAMPLE: (&str, &str) = ("fix-a-bug.yaml", include_str!("../../workflows/fix-a-bug.yaml"));
/// The workflow Tracket's "Ask an agent" uses, also written into an empty folder.
pub const ASK_EXAMPLE: (&str, &str) = ("ask-an-agent.yaml", include_str!("../../workflows/ask-an-agent.yaml"));

// ---- The file as written ----

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileWorkflow {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default)]
    pub offer_in_tracket: bool,
    /// Runs what someone asks in words (Tracket's "Ask an agent"): its steps read it as `{{ request }}`
    #[serde(default, skip_serializing_if = "is_no")]
    pub for_requests: bool,
    #[serde(default = "one")]
    pub max_parallel_runs: u32,
    /// A board the run's agents and people share; on when left out
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub board: bool,
    /// Agent types of this workflow only; the shared ones are in `<data dir>/agents`
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agents: BTreeMap<String, FileAgent>,
    pub steps: Vec<FileStep>,
    /// Asking other agent types (`ask_agent`): who may be asked, and how often per step
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colleagues: Option<FileColleagues>,
    /// The most a team step may use; a team step can only ask for less
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_limits: Option<FileTeamLimits>,
    /// Where the editor shows the nodes someone placed by hand, by step (and `$trigger`, `$end`).
    /// Only the editor reads it; the others are laid out automatically
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layout: BTreeMap<String, Point>,
}

/// A team step as written.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileTeam {
    /// Agent types working together, two or more
    pub members: Vec<String>,
    /// The member that ends the step, with `finish`
    pub lead: String,
    /// The one member that may change files; none: nobody does
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writer: Option<String>,
    pub goal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes: Option<u32>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileTeamLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes: Option<u32>,
}

/// A team member's stop rules: turns of its session, its budget, and the team's time.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TeamLimits {
    /// Turns of each member's session
    pub max_turns: u32,
    /// Dollars each member may spend
    pub budget_usd: f64,
    /// Minutes each member may work (time waiting for a person doesn't count)
    pub minutes: u32,
}

impl Default for TeamLimits {
    fn default() -> Self {
        Self { max_turns: 40, budget_usd: 5.0, minutes: 60 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub members: Vec<String>,
    pub lead: String,
    pub writer: Option<String>,
    pub goal: String,
    pub limits: TeamLimits,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileColleagues {
    /// The agent types a step may ask; when left out, those the steps use. Empty: no asking
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_ask: Option<Vec<String>>,
    /// Asks a step may make; 3 when left out
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_asks: Option<u32>,
}

/// Asking other agent types, checked.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Colleagues {
    pub can_ask: Vec<String>,
    pub max_asks: u32,
}

pub const DEFAULT_ASKS: u32 = 3;
pub const MAX_ASKS: u32 = 20;

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

fn one() -> u32 {
    1
}

fn yes() -> bool {
    true
}

fn is_yes(b: &bool) -> bool {
    *b
}

fn is_no(b: &bool) -> bool {
    !*b
}

/// A step as written. Which fields it has says what it is:
/// - `agent` (and nothing below): an agent does the work; `output` asks it for a JSON result
/// - `approval`: waits for someone to approve; `onReject` sends the run back to an earlier step
/// - `action` (with the `agent` that does it): one thing done in another tool, such as a Jira comment
/// - `if` with `then` and `else`: takes one branch or the other
/// - `forEach` with `steps`: does those steps for each item of a list, `parallel` at a time
/// - `branches`: lists of steps that run at the same time; the next step waits for all of them
/// - `team`: several agent types working on the step together over the board; the lead finishes it
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileStep {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<FileApproval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    #[serde(default, rename = "if", skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub then: Option<Vec<FileStep>>,
    #[serde(default, rename = "else", skip_serializing_if = "Option::is_none")]
    pub otherwise: Option<Vec<FileStep>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub for_each: Option<String>,
    #[serde(default, rename = "as", skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_item_failure: Option<ItemFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<FileStep>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<Vec<Vec<FileStep>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<FileTeam>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<BTreeMap<String, OutputType>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_reject: Option<String>,
}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FileApproval {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// The type of one field of an agent's JSON output.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
pub enum OutputType {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "number")]
    Number,
    #[serde(rename = "yes/no")]
    YesNo,
    #[serde(rename = "list")]
    List,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    /// A comment on the run's Jira ticket
    #[default]
    Comment,
    /// Moves the ticket to the status `to`
    Status,
    /// A message to the Slack channel `to`
    Slack,
    /// Anything else, said in `text`
    Other,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Action {
    #[serde(rename = "do")]
    pub kind: ActionKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

/// What a loop does when one item fails.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ItemFailure {
    /// Starts no other item, and fails once the running ones end
    #[default]
    Stop,
    /// Goes on with the other items
    Continue,
}

/// Loops are capped: each item can mean an agent session.
pub const MAX_ITEMS: usize = 50;
pub const MAX_PARALLEL: u32 = 10;

// ---- What the rest of the app uses ----

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StepKind {
    Agent,
    Approval,
    Action,
    Condition,
    ForEach,
    /// Branches that run at the same time
    Parallel,
    /// Agent types working on one step together
    Team,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OutputField {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: OutputType,
}

/// A step, checked. Also what the editor sends back, and what a run keeps of its workflow (older runs
/// have only the first fields, hence the defaults).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: String,
    pub kind: StepKind,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub instructions: Option<String>,
    /// For agent steps, what the agent gets (`steps.plan.output`); for approvals, the summary shown
    #[serde(default)]
    pub input: Option<String>,
    #[serde(default)]
    pub on_reject: Option<String>,
    /// The fields of an agent step's JSON output; none for a text answer
    #[serde(default)]
    pub output: Vec<OutputField>,
    #[serde(default)]
    pub action: Option<Action>,
    #[serde(default, rename = "if")]
    pub condition: Option<String>,
    #[serde(default)]
    pub then: Vec<Step>,
    #[serde(default, rename = "else")]
    pub otherwise: Vec<Step>,
    #[serde(default)]
    pub for_each: Option<String>,
    #[serde(default, rename = "as")]
    pub item: Option<String>,
    #[serde(default = "one")]
    pub parallel: u32,
    #[serde(default)]
    pub on_item_failure: ItemFailure,
    /// A loop's steps
    #[serde(default)]
    pub steps: Vec<Step>,
    /// A parallel group's branches
    #[serde(default)]
    pub branches: Vec<Vec<Step>>,
    /// A team step's members, lead, writer, goal and limits
    #[serde(default)]
    pub team: Option<Team>,
}

impl Step {
    /// The steps inside this one: both branches of a condition, a loop's steps.
    pub fn children(&self) -> impl Iterator<Item = &Step> {
        self.then.iter().chain(&self.otherwise).chain(&self.steps).chain(self.branches.iter().flatten())
    }

    /// The JSON schema an agent step's output must follow.
    pub fn output_schema(&self) -> Option<serde_json::Value> {
        if self.output.is_empty() {
            return None;
        }
        let properties: serde_json::Map<String, serde_json::Value> = self
            .output
            .iter()
            .map(|f| {
                let schema = match f.kind {
                    OutputType::Text => serde_json::json!({ "type": "string" }),
                    OutputType::Number => serde_json::json!({ "type": "number" }),
                    OutputType::YesNo => serde_json::json!({ "type": "boolean" }),
                    OutputType::List => serde_json::json!({ "type": "array", "items": { "type": "string" } }),
                };
                (f.name.clone(), schema)
            })
            .collect();
        let required: Vec<&str> = self.output.iter().map(|f| f.name.as_str()).collect();
        Some(serde_json::json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false }))
    }

    /// Back to the file's form.
    pub fn to_file(&self) -> FileStep {
        let text = |t: &Option<String>| t.as_ref().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        let steps = |list: &[Step]| Some(list.iter().map(Step::to_file).collect::<Vec<_>>());
        let mut f = FileStep { id: self.id.trim().to_string(), ..Default::default() };
        match self.kind {
            StepKind::Agent => {
                f.agent = self.agent.clone();
                f.instructions = text(&self.instructions);
                f.input = text(&self.input);
                if !self.output.is_empty() {
                    f.output = Some(self.output.iter().map(|o| (o.name.trim().to_string(), o.kind)).collect());
                }
            }
            StepKind::Approval => {
                f.approval = Some(FileApproval { summary: text(&self.input) });
                f.on_reject = text(&self.on_reject);
            }
            StepKind::Action => {
                f.agent = self.agent.clone();
                let a = self.action.clone().unwrap_or_default();
                f.action = Some(Action { kind: a.kind, text: a.text.trim().to_string(), to: text(&a.to) });
            }
            StepKind::Condition => {
                f.condition = text(&self.condition);
                f.then = steps(&self.then);
                f.otherwise = steps(&self.otherwise);
            }
            StepKind::ForEach => {
                f.for_each = text(&self.for_each);
                f.item = text(&self.item).filter(|i| i != "item");
                f.parallel = Some(self.parallel).filter(|&p| p > 1);
                f.on_item_failure = Some(self.on_item_failure).filter(|&p| p != ItemFailure::Stop);
                f.steps = steps(&self.steps);
            }
            StepKind::Parallel => {
                f.branches = Some(self.branches.iter().map(|b| b.iter().map(Step::to_file).collect()).collect());
            }
            StepKind::Team => {
                let t = self.team.clone().unwrap_or_else(|| Team { members: vec![], lead: String::new(), writer: None, goal: String::new(), limits: TeamLimits::default() });
                f.team = Some(FileTeam {
                    members: t.members,
                    lead: t.lead,
                    writer: t.writer.filter(|w| !w.is_empty()),
                    goal: t.goal.trim().to_string(),
                    max_turns: Some(t.limits.max_turns),
                    budget_usd: Some(t.limits.budget_usd),
                    minutes: Some(t.limits.minutes),
                });
            }
        }
        f
    }
}

/// Where a step is in its workflow.
pub struct Place<'a> {
    pub step: &'a Step,
    /// The steps of its block (its branch, its loop, or the workflow's), and where it is in them
    pub block: &'a [Step],
    pub index: usize,
    /// The loops around it, outermost first
    pub loops: Vec<&'a Step>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    /// The file name without `.yaml`
    pub id: String,
    pub name: String,
    pub description: String,
    pub offer_in_tracket: bool,
    /// Runs what someone asks in words
    #[serde(default)]
    pub for_requests: bool,
    pub max_parallel_runs: u32,
    /// Runs have a board (older runs' snapshots: yes)
    #[serde(default = "yes")]
    pub board: bool,
    pub file: String,
    /// The agent types its steps use, resolved: its own first, then the shared ones
    pub agents: Vec<AgentType>,
    pub steps: Vec<Step>,
    /// Nodes placed by hand in the editor
    #[serde(default)]
    pub layout: BTreeMap<String, Point>,
    /// Who the steps may ask, and how often (older runs: nobody)
    #[serde(default)]
    pub colleagues: Colleagues,
    /// The most a team step may use
    #[serde(default)]
    pub team_limits: TeamLimits,
}

impl Workflow {
    /// Every step, nested ones included, in the order they are written.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn all_steps(&self) -> Vec<&Step> {
        fn walk<'a>(steps: &'a [Step], out: &mut Vec<&'a Step>) {
            for s in steps {
                out.push(s);
                walk(&s.then, out);
                walk(&s.otherwise, out);
                walk(&s.steps, out);
                for b in &s.branches {
                    walk(b, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.steps, &mut out);
        out
    }

    pub fn find(&self, id: &str) -> Option<Place<'_>> {
        fn search<'a>(block: &'a [Step], id: &str, loops: &mut Vec<&'a Step>) -> Option<Place<'a>> {
            for (index, step) in block.iter().enumerate() {
                if step.id == id {
                    return Some(Place { step, block, index, loops: loops.clone() });
                }
                for branch in [&step.then, &step.otherwise].into_iter().chain(&step.branches) {
                    if let Some(found) = search(branch, id, loops) {
                        return Some(found);
                    }
                }
                loops.push(step);
                let found = search(&step.steps, id, loops);
                loops.pop();
                if found.is_some() {
                    return found;
                }
            }
            None
        }
        search(&self.steps, id, &mut Vec::new())
    }
}

#[derive(Serialize, Default, Debug)]
pub struct WorkflowList {
    pub workflows: Vec<Workflow>,
    pub problems: Vec<Problem>,
}

/// An `input` or approval `summary` is a template; a bare `steps.plan.output` reads as `{{ steps.plan.output }}`.
pub fn as_template(text: &str) -> std::borrow::Cow<'_, str> {
    let t = text.trim();
    if !t.contains("{{") && t.starts_with("steps.") && !t.contains(char::is_whitespace) {
        return format!("{{{{ {t} }}}}").into();
    }
    text.into()
}

/// What a step can read at its place: earlier steps (with the fields of their JSON output), and the
/// items of the loops around it.
#[derive(Clone, Default)]
struct Visible {
    /// None: the step gives text (or, for a loop, a list); only `steps.<id>.output` as a whole
    steps: BTreeMap<String, Option<Vec<String>>>,
    items: Vec<String>,
}

impl Visible {
    fn check(&self, text: &str, what: &str) -> Result<(), String> {
        let e = expr::parse(text).map_err(|e| format!("{what}: {e}"))?;
        self.paths(&e, what)
    }

    fn check_template(&self, text: &str, what: &str) -> Result<(), String> {
        for (_, e) in expr::template_parts(&as_template(text)).map_err(|e| format!("{what}: {e}"))? {
            self.paths(&e, what)?;
        }
        Ok(())
    }

    fn paths(&self, e: &expr::Expr, what: &str) -> Result<(), String> {
        for path in e.paths() {
            let name = &path[0];
            // What someone asked for, on a run started from a request (null otherwise)
            if name == "request" {
                continue;
            }
            if name != "steps" {
                if !self.items.contains(name) {
                    return Err(format!(
                        "{what}: `{name}` is unknown. Read a step with steps.<step>.output, or the item of a loop around it"
                    ));
                }
                continue;
            }
            let (Some(step), Some("output")) = (path.get(1), path.get(2).map(String::as_str)) else {
                return Err(format!("{what}: write steps.<step>.output"));
            };
            let Some(fields) = self.steps.get(step) else {
                return Err(format!(
                    "{what}: no earlier step `{step}` to read here (steps inside a loop or another branch can't be read from outside)"
                ));
            };
            if let Some(field) = path.get(3) {
                match fields {
                    Some(fields) if fields.contains(field) => {}
                    Some(_) => return Err(format!("{what}: the output of `{step}` has no field `{field}`")),
                    None => return Err(format!("{what}: `{step}` gives text, not JSON fields like `{field}`")),
                }
            }
        }
        Ok(())
    }
}

struct Checker<'a> {
    own: &'a BTreeMap<String, FileAgent>,
    shared: &'a BTreeMap<String, AgentType>,
    seen: HashSet<String>,
    used: Vec<AgentType>,
    /// The workflow has a board (teams need it)
    board: bool,
    team_limits: TeamLimits,
}

impl Checker<'_> {
    fn agent(&mut self, step: &str, name: &str) -> Result<(), String> {
        let found = match self.own.get(name) {
            Some(own) => AgentType::from_file(name, own.clone(), false),
            None => self
                .shared
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Step `{step}` uses the agent type `{name}`, which doesn't exist"))?,
        };
        if !self.used.iter().any(|a| a.id == found.id) {
            self.used.push(found);
        }
        Ok(())
    }

    fn block(&mut self, steps: Vec<FileStep>, vis: &mut Visible) -> Result<Vec<Step>, String> {
        let mut out: Vec<Step> = Vec::with_capacity(steps.len());
        for s in steps {
            let step = self.step(s, vis, &out)?;
            out.push(step);
        }
        Ok(out)
    }

    fn step(&mut self, s: FileStep, vis: &mut Visible, before: &[Step]) -> Result<Step, String> {
        let id = s.id.clone();
        if !agent::valid_id(&id) {
            return Err(format!("Step `{id}`: a step name is lowercase letters, digits, - or _"));
        }
        if !self.seen.insert(id.clone()) {
            return Err(format!("Two steps are called `{id}`"));
        }
        let at = |field: &str| format!("Step `{id}`, {field}");
        let kinds = [s.approval.is_some(), s.action.is_some(), s.condition.is_some(), s.for_each.is_some(), s.branches.is_some(), s.team.is_some()];
        let kind = match kinds {
            [false, false, false, false, false, false] if s.agent.is_some() => StepKind::Agent,
            [true, false, false, false, false, false] => StepKind::Approval,
            [false, true, false, false, false, false] => StepKind::Action,
            [false, false, true, false, false, false] => StepKind::Condition,
            [false, false, false, true, false, false] => StepKind::ForEach,
            [false, false, false, false, true, false] => StepKind::Parallel,
            [false, false, false, false, false, true] => StepKind::Team,
            [false, false, false, false, false, false] => {
                return Err(format!("Step `{id}` needs one of `agent`, `approval`, `action`, `if`, `forEach`, `branches` or `team`"))
            }
            _ => return Err(format!("Step `{id}` can be only one of an approval, an action, an `if`, a `forEach`, `branches` or a `team`")),
        };
        // Fields that belong to another kind of step are a mistake, not something to ignore
        let stray = [
            ("agent", s.agent.is_some() && !matches!(kind, StepKind::Agent | StepKind::Action)),
            ("instructions", s.instructions.is_some() && kind != StepKind::Agent),
            ("input", s.input.is_some() && kind != StepKind::Agent),
            ("output", s.output.is_some() && kind != StepKind::Agent),
            ("onReject", s.on_reject.is_some() && kind != StepKind::Approval),
            ("then", s.then.is_some() && kind != StepKind::Condition),
            ("else", s.otherwise.is_some() && kind != StepKind::Condition),
            ("steps", s.steps.is_some() && kind != StepKind::ForEach),
            ("as", s.item.is_some() && kind != StepKind::ForEach),
            ("parallel", s.parallel.is_some() && kind != StepKind::ForEach),
            ("onItemFailure", s.on_item_failure.is_some() && kind != StepKind::ForEach),
            ("branches", s.branches.is_some() && kind != StepKind::Parallel),
            ("team", s.team.is_some() && kind != StepKind::Team),
        ];
        if let Some((field, _)) = stray.iter().find(|(_, bad)| *bad) {
            return Err(format!("Step `{id}`: `{field}` doesn't belong on this kind of step"));
        }

        let mut step = Step {
            id: id.clone(),
            kind,
            agent: s.agent.clone(),
            instructions: s.instructions.clone(),
            input: s.input.clone(),
            on_reject: s.on_reject.clone(),
            output: Vec::new(),
            action: None,
            condition: s.condition.clone(),
            then: Vec::new(),
            otherwise: Vec::new(),
            for_each: s.for_each.clone(),
            item: None,
            parallel: 1,
            on_item_failure: ItemFailure::Stop,
            steps: Vec::new(),
            branches: Vec::new(),
            team: None,
        };
        let mut gives: Option<Vec<String>> = None;
        match kind {
            StepKind::Agent => {
                self.agent(&id, s.agent.as_deref().unwrap_or_default())?;
                if let Some(t) = &s.instructions {
                    vis.check_template(t, &at("instructions"))?;
                }
                if let Some(t) = &s.input {
                    vis.check_template(t, &at("input"))?;
                }
                for (name, kind) in s.output.unwrap_or_default() {
                    if !agent::valid_id(&name) {
                        return Err(format!("Step `{id}`: the output field `{name}` isn't lowercase letters, digits, - or _"));
                    }
                    step.output.push(OutputField { name, kind });
                }
                if !step.output.is_empty() {
                    gives = Some(step.output.iter().map(|f| f.name.clone()).collect());
                }
            }
            StepKind::Approval => {
                let summary = s.approval.and_then(|a| a.summary);
                if let Some(t) = &summary {
                    vis.check_template(t, &at("summary"))?;
                }
                step.input = summary;
                if let Some(target) = &s.on_reject {
                    // Only back, and only in the same block: a rejection redoes the steps from there
                    if !before.iter().any(|b| &b.id == target) {
                        return Err(format!("Step `{id}`: `onReject` must name an earlier step next to it"));
                    }
                }
            }
            StepKind::Action => {
                let Some(name) = s.agent.as_deref() else {
                    return Err(format!("Step `{id}`: an action needs the `agent` that does it"));
                };
                self.agent(&id, name)?;
                let action = s.action.unwrap_or_default();
                let needs_to = matches!(action.kind, ActionKind::Status | ActionKind::Slack);
                if needs_to && action.to.as_deref().is_none_or(|t| t.trim().is_empty()) {
                    return Err(format!("Step `{id}`: say where in `to` (the status, or the Slack channel)"));
                }
                if action.kind != ActionKind::Status && action.text.trim().is_empty() {
                    return Err(format!("Step `{id}`: the action's `text` is empty"));
                }
                vis.check_template(&action.text, &at("text"))?;
                if let Some(to) = &action.to {
                    vis.check_template(to, &at("to"))?;
                }
                step.action = Some(action);
            }
            StepKind::Condition => {
                vis.check(s.condition.as_deref().unwrap_or_default(), &at("if"))?;
                let mut then_vis = vis.clone();
                let mut else_vis = vis.clone();
                step.then = self.block(s.then.unwrap_or_default(), &mut then_vis)?;
                step.otherwise = self.block(s.otherwise.unwrap_or_default(), &mut else_vis)?;
                if step.then.is_empty() && step.otherwise.is_empty() {
                    return Err(format!("Step `{id}`: put steps in `then` or `else`"));
                }
                // After the condition, the steps of either branch can be read (null when not taken)
                vis.steps.extend(then_vis.steps);
                vis.steps.extend(else_vis.steps);
                return Ok(step);
            }
            StepKind::Team => {
                let t = s.team.clone().unwrap_or_default();
                if !self.board {
                    return Err(format!("Step `{id}`: a team talks on the run's board: turn the board on"));
                }
                if t.members.len() < 2 {
                    return Err(format!("Step `{id}`: a team has two members or more"));
                }
                for (i, m) in t.members.iter().enumerate() {
                    if t.members[..i].contains(m) {
                        return Err(format!("Step `{id}`: `{m}` is in the team twice"));
                    }
                    self.agent(&id, m)?;
                    if self.used.iter().find(|a| &a.id == m).is_some_and(|a| !a.board) {
                        return Err(format!("Step `{id}`: `{m}` may not post to the board, so it can't work in a team"));
                    }
                }
                if !t.members.contains(&t.lead) {
                    return Err(format!("Step `{id}`: the lead, `{}`, isn't a member", t.lead));
                }
                if let Some(w) = t.writer.as_ref().filter(|w| !t.members.contains(w)) {
                    return Err(format!("Step `{id}`: the writer, `{w}`, isn't a member"));
                }
                if t.goal.trim().is_empty() {
                    return Err(format!("Step `{id}`: say the team's goal"));
                }
                vis.check_template(&t.goal, &at("goal"))?;
                // A step can ask for less than the workflow's limits, not more
                let most = self.team_limits;
                let limits = TeamLimits {
                    max_turns: t.max_turns.unwrap_or(most.max_turns),
                    budget_usd: t.budget_usd.unwrap_or(most.budget_usd),
                    minutes: t.minutes.unwrap_or(most.minutes),
                };
                if limits.max_turns == 0 || limits.max_turns > most.max_turns {
                    return Err(format!("Step `{id}`: `maxTurns` is from 1 to {}, the workflow's limit", most.max_turns));
                }
                if !(limits.budget_usd > 0.0 && limits.budget_usd <= most.budget_usd) {
                    return Err(format!("Step `{id}`: `budgetUsd` is more than 0 and at most {}, the workflow's limit", most.budget_usd));
                }
                if limits.minutes == 0 || limits.minutes > most.minutes {
                    return Err(format!("Step `{id}`: `minutes` is from 1 to {}, the workflow's limit", most.minutes));
                }
                step.team = Some(Team { members: t.members, lead: t.lead, writer: t.writer.filter(|w| !w.is_empty()), goal: t.goal, limits });
            }
            StepKind::Parallel => {
                let lanes = s.branches.unwrap_or_default();
                if lanes.len() < 2 || lanes.iter().any(Vec::is_empty) {
                    return Err(format!("Step `{id}`: `branches` needs two or more lists of steps, none empty"));
                }
                // The branches run at the same time: one can't read another; the steps after read them all
                let mut after = vis.clone();
                for lane in lanes {
                    let mut lane_vis = vis.clone();
                    step.branches.push(self.block(lane, &mut lane_vis)?);
                    after.steps.extend(lane_vis.steps);
                }
                *vis = after;
                return Ok(step);
            }
            StepKind::ForEach => {
                vis.check(s.for_each.as_deref().unwrap_or_default(), &at("forEach"))?;
                let item = s.item.unwrap_or_else(|| "item".into());
                let reserved = ["steps", "request", "and", "or", "not", "contains", "true", "false", "null"];
                if !agent::valid_id(&item) || reserved.contains(&item.as_str()) {
                    return Err(format!("Step `{id}`: `as` is a name of lowercase letters, digits, - or _"));
                }
                step.parallel = s.parallel.unwrap_or(1);
                if !(1..=MAX_PARALLEL).contains(&step.parallel) {
                    return Err(format!("Step `{id}`: `parallel` is from 1 to {MAX_PARALLEL}"));
                }
                step.on_item_failure = s.on_item_failure.unwrap_or_default();
                let mut inner = vis.clone();
                inner.items.push(item.clone());
                step.steps = self.block(s.steps.unwrap_or_default(), &mut inner)?;
                if step.steps.is_empty() {
                    return Err(format!("Step `{id}`: a loop needs steps"));
                }
                step.item = Some(item);
            }
        }
        vis.steps.insert(id, gives);
        Ok(step)
    }
}

fn checker_seen(steps: &[Step], id: &str) -> bool {
    steps.iter().any(|s| s.id == id || checker_seen(&s.children().cloned().collect::<Vec<_>>(), id))
}

/// Checks a workflow file and resolves its agents against the shared ones.
pub fn resolve(id: &str, file: &str, w: FileWorkflow, shared: &BTreeMap<String, AgentType>) -> Result<Workflow, String> {
    if w.name.trim().is_empty() {
        return Err("`name` is empty".into());
    }
    if w.steps.is_empty() {
        return Err("A workflow needs at least one step".into());
    }
    let team_limits = {
        let file = w.team_limits.clone().unwrap_or_default();
        let base = TeamLimits::default();
        TeamLimits {
            max_turns: file.max_turns.unwrap_or(base.max_turns),
            budget_usd: file.budget_usd.unwrap_or(base.budget_usd),
            minutes: file.minutes.unwrap_or(base.minutes),
        }
    };
    if team_limits.max_turns == 0 || team_limits.budget_usd <= 0.0 || team_limits.minutes == 0 {
        return Err("`teamLimits` are more than 0".into());
    }
    let mut checker = Checker { own: &w.agents, shared, seen: HashSet::new(), used: Vec::new(), board: w.board, team_limits };
    let steps = checker.block(w.steps.clone(), &mut Visible::default())?;
    let file_colleagues = w.colleagues.clone().unwrap_or_default();
    let can_ask = match file_colleagues.can_ask {
        Some(names) => {
            for name in &names {
                checker.agent("colleagues", name).map_err(|_| format!("`colleagues.canAsk` names `{name}`, which isn't an agent type"))?;
            }
            names
        }
        None => checker.used.iter().map(|a| a.id.clone()).collect(),
    };
    let max_asks = file_colleagues.max_asks.unwrap_or(DEFAULT_ASKS);
    if !(1..=MAX_ASKS).contains(&max_asks) {
        return Err(format!("`colleagues.maxAsks` is from 1 to {MAX_ASKS}"));
    }
    let used = checker.used;
    Ok(Workflow {
        id: id.to_string(),
        name: w.name,
        description: w.description,
        offer_in_tracket: w.offer_in_tracket,
        for_requests: w.for_requests,
        max_parallel_runs: w.max_parallel_runs.max(1),
        board: w.board,
        file: file.to_string(),
        agents: used,
        // Positions of steps that no longer exist are dropped
        colleagues: Colleagues { can_ask, max_asks },
        team_limits,
        layout: w.layout.into_iter().filter(|(k, _)| k == "$trigger" || k == "$end" || checker_seen(&steps, k)).collect(),
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
        std::fs::write(dir.join(ASK_EXAMPLE.0), ASK_EXAMPLE.1)?;
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
        assert!(err("name: x\nsteps:\n  - id: a").contains("needs one of"));
        assert!(err("name: x\nsteps:\n  - id: a\n    approval: {}\n  - id: a\n    approval: {}").contains("Two steps"));
        assert!(err("name: x\nsteps:\n  - id: a\n    approval: {}\n    onReject: b\n  - id: b\n    approval: {}").contains("earlier step"));
        assert!(err("name: x\nsteps:\n  - id: a\n    approval: {}\n    onReject: a").contains("earlier step"));
        assert!(err("name: x\nsteps:\n  - id: A b\n    approval: {}").contains("lowercase"));
        assert!(err("name: x\nsteps:\n  - id: a\n    approval:\n      summary: steps.z.output").contains("no earlier step `z`"));
        assert!(err("name: x\nsurprise: 1\nsteps:\n  - id: a\n    approval: {}").contains("unknown field"));
    }

    const RICH: &str = r#"
name: Triage
steps:
- id: triage
  agent: planner
  output: { severity: text, files: list, risky: yes/no }
- id: route
  if: steps.triage.output.risky and steps.triage.output.severity == "high"
  then:
  - id: check
    approval: { summary: "Risky: {{ steps.triage.output.files }}" }
  else:
  - id: note
    agent: planner
    action: { do: comment, text: "Low risk ({{ steps.triage.output.severity }})" }
- id: each
  forEach: steps.triage.output.files
  as: file
  parallel: 2
  onItemFailure: continue
  steps:
  - id: fix
    agent: builder
    instructions: Fix {{ file }}
  - id: review
    approval: { summary: steps.fix.output }
    onReject: fix
- id: report
  agent: planner
  input: steps.each.output
"#;

    #[test]
    fn reads_conditions_loops_actions_and_json_outputs() {
        let w = parse("t", "t.yaml", RICH, &shared()).unwrap();
        let kinds: Vec<_> = w.all_steps().iter().map(|s| (s.id.as_str(), s.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("triage", StepKind::Agent),
                ("route", StepKind::Condition),
                ("check", StepKind::Approval),
                ("note", StepKind::Action),
                ("each", StepKind::ForEach),
                ("fix", StepKind::Agent),
                ("review", StepKind::Approval),
                ("report", StepKind::Agent),
            ]
        );
        let each = w.find("review").unwrap();
        assert_eq!((each.index, each.loops.len(), each.loops[0].id.as_str()), (1, 1, "each"));
        let schema = w.steps[0].output_schema().unwrap();
        assert_eq!(schema["properties"]["risky"]["type"], "boolean");
        assert_eq!(w.steps[2].parallel, 2);
        // Writing it back gives the same workflow
        let file = FileWorkflow {
            name: w.name.clone(),
            description: String::new(),
            offer_in_tracket: false,
            for_requests: false,
            max_parallel_runs: 1,
            board: true,
            agents: BTreeMap::new(),
            steps: w.steps.iter().map(Step::to_file).collect(),
            layout: BTreeMap::new(),
            colleagues: None,
            team_limits: None,
        };
        let again = parse("t", "t.yaml", &serde_yaml::to_string(&file).unwrap(), &shared()).unwrap();
        assert_eq!(serde_json::to_value(&again.steps).unwrap(), serde_json::to_value(&w.steps).unwrap());
    }

    #[test]
    fn checks_what_each_place_can_read() {
        let bad = |from: &str, to: &str| err(&RICH.replacen(from, to, 1));
        assert!(bad("steps.triage.output.severity == \"high\"", "nope == 1").contains("`nope` is unknown"));
        assert!(bad("output.risky and", "output.colour and").contains("has no field `colour`"));
        assert!(bad("input: steps.each.output", "input: steps.fix.output").contains("can't be read from outside"));
        assert!(bad("Fix {{ file }}", "Fix {{ steps.report.output }}").contains("no earlier step `report`"));
        assert!(bad("{ do: comment, text: \"Low", "{ do: slack, text: \"Low").contains("`to`"));
        assert!(bad("    agent: planner\n    action", "    action").contains("needs the `agent`"));
        assert!(bad("  parallel: 2", "  parallel: 50").contains("from 1 to"));
        assert!(bad("  onReject: fix", "  onReject: triage").contains("earlier step next to it"));
        assert!(bad("- id: triage\n  agent: planner\n", "- id: triage\n  agent: planner\n  then: []\n").contains("`then` doesn't belong"));
        assert!(bad("  as: file", "  as: file\n  if: x").contains("only one of"));
        assert!(bad("  as: file", "  as: not").contains("`as` is a name"));
    }

    const TOGETHER: &str = "name: T\nsteps:\n- id: plan\n  agent: planner\n- id: both\n  branches:\n  - - id: code\n      agent: builder\n  - - id: docs\n      agent: planner\n    - id: check\n      approval: { summary: steps.docs.output }\n- id: merge\n  agent: builder\n  input: '{{ steps.code.output }} {{ steps.check.output }}'\n";

    #[test]
    fn reads_parallel_branches() {
        let w = parse("t", "t.yaml", TOGETHER, &shared()).unwrap();
        assert_eq!(w.steps[1].kind, StepKind::Parallel);
        assert_eq!(w.steps[1].branches.len(), 2);
        let check = w.find("check").unwrap();
        assert_eq!((check.index, check.block.len()), (1, 2));
        assert_eq!(w.all_steps().len(), 6);
        // One branch can't read the other: they run at the same time
        assert!(err(&TOGETHER.replace("summary: steps.docs.output", "summary: steps.code.output")).contains("no earlier step `code`"));
        assert!(err("name: x\nsteps:\n- id: g\n  branches:\n  - - id: a\n      agent: planner\n").contains("two or more"));
    }

    #[test]
    fn keeps_hand_placed_positions_of_existing_steps_only() {
        let yaml = "name: x
steps:
- id: a
  agent: planner
layout:
  a: { x: 300, y: 120 }
  gone: { x: 1, y: 2 }
  $end: { x: 300, y: 400 }
";
        let w = parse("t", "t.yaml", yaml, &shared()).unwrap();
        assert_eq!(w.layout.keys().map(String::as_str).collect::<Vec<_>>(), ["$end", "a"]);
        assert_eq!(w.layout["a"], Point { x: 300, y: 120 });
    }

    const TEAM: &str = "name: T
teamLimits: { maxTurns: 30, budgetUsd: 4, minutes: 45 }
steps:
- id: design
  team:
    members: [planner, builder]
    lead: planner
    writer: builder
    goal: Agree on the change and make it
    maxTurns: 20
- id: after
  agent: planner
  input: steps.design.output
";

    #[test]
    fn reads_a_team_step_and_its_limits() {
        let w = parse("t", "t.yaml", TEAM, &shared()).unwrap();
        let t = w.steps[0].team.as_ref().unwrap();
        assert_eq!((t.lead.as_str(), t.writer.as_deref()), ("planner", Some("builder")));
        // The step's own limit, the workflow's for the rest
        assert_eq!(t.limits, TeamLimits { max_turns: 20, budget_usd: 4.0, minutes: 45 });
        assert_eq!(w.team_limits.max_turns, 30);
        let bad = |from: &str, to: &str| err(&TEAM.replacen(from, to, 1));
        assert!(bad("maxTurns: 20", "maxTurns: 50").contains("the workflow's limit"));
        assert!(bad("members: [planner, builder]", "members: [planner]").contains("two members"));
        assert!(bad("lead: planner", "lead: nobody").contains("isn't a member"));
        assert!(bad("writer: builder", "writer: nobody").contains("isn't a member"));
        assert!(bad("goal: Agree on the change and make it", "goal: ' '").contains("goal"));
        assert!(bad("name: T", "name: T
board: false").contains("board"));
        assert!(bad("    goal:", "    agent: planner
    goal:").contains("only one of") || bad("    goal:", "    agent: planner
    goal:").contains("unknown field"));
    }

    #[test]
    fn an_empty_folder_gets_the_examples_and_bad_files_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("workflows");
        let first = load_all(&folder).unwrap();
        let ids: Vec<_> = first.workflows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["ask-an-agent", "fix-a-bug"]);
        assert!(first.workflows[0].for_requests);
        assert!(dir.path().join("agents").join("planner.yaml").exists());

        std::fs::write(folder.join("broken.yaml"), "name: [").unwrap();
        let second = load_all(&folder).unwrap();
        assert_eq!((second.workflows.len(), second.problems.len()), (2, 1));
        assert!(second.problems[0].file.ends_with("broken.yaml"));
    }
}
