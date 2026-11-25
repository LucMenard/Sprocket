//! Mirror of agent-host/src/protocol.ts: one JSON object per line over the host's stdio.
//! Change both together and bump PROTOCOL.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::workflow::AskPolicy;

pub const PROTOCOL: u32 = 4;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentSpec {
    pub model: String,
    pub ask: AskPolicy,
    pub tools: Vec<String>,
    /// MCP servers allowed; empty removes every MCP tool from the session
    pub mcp: Vec<String>,
    pub instructions: Option<String>,
    pub board: BoardAccess,
    /// The agent types it may ask with `ask_agent`; none: no such tool
    pub colleagues: Vec<Colleague>,
}

/// An agent type another one may ask, as the `ask_agent` tool describes it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Colleague {
    pub id: String,
    pub description: Option<String>,
}

/// What an agent can do with its run's board.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BoardAccess {
    /// The workflow has no board: no board tools at all
    Off,
    Read,
    Post,
}

/// Core → host.
#[derive(Serialize, Debug)]
#[serde(tag = "type")]
pub enum CoreMessage {
    #[serde(rename = "job.start", rename_all = "camelCase")]
    JobStart { job_id: String, cwd: String, prompt: String, agent: AgentSpec, output_schema: Option<Value> },
    #[serde(rename = "job.resume", rename_all = "camelCase")]
    JobResume { job_id: String, cwd: String, sdk_session_id: String, prompt: String, agent: AgentSpec, output_schema: Option<Value> },
    #[serde(rename = "job.stop", rename_all = "camelCase")]
    JobStop { job_id: String },
    #[serde(rename = "tool.answer", rename_all = "camelCase")]
    ToolAnswer { ask_id: String, behavior: Behavior, message: Option<String> },
    /// The answer to a board request: text for the agent, or why it failed
    #[serde(rename = "board.answer", rename_all = "camelCase")]
    BoardAnswer { request_id: String, text: String, error: bool },
    /// The answer to `ask_agent`: the colleague's answer, or why there is none
    #[serde(rename = "colleague.answer", rename_all = "camelCase")]
    ColleagueAnswer { request_id: String, text: String, error: bool },
}

#[derive(Serialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Behavior {
    Allow,
    Deny,
}

/// Host → core.
#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
pub enum HostMessage {
    #[serde(rename = "host.ready", rename_all = "camelCase")]
    Ready { protocol: u32, sdk: String, claude_code: String },
    #[serde(rename = "host.error")]
    Error { message: String },
    #[serde(rename = "job.started", rename_all = "camelCase")]
    JobStarted { job_id: String, sdk_session_id: String },
    #[serde(rename = "job.message", rename_all = "camelCase")]
    JobMessage { job_id: String, message: Value },
    #[serde(rename = "job.ended", rename_all = "camelCase")]
    JobEnded {
        job_id: String,
        result: String,
        output: Option<String>,
        cost_usd: Option<f64>,
        error: Option<String>,
        /// The JSON output, when the step asked for one
        #[serde(default)]
        data: Option<Value>,
    },
    #[serde(rename = "tool.ask", rename_all = "camelCase")]
    ToolAsk { ask_id: String, job_id: String, tool_name: String, input: Value },
    /// An agent posts a note to its run's board, for everyone or for someone (`to`)
    #[serde(rename = "board.post", rename_all = "camelCase")]
    BoardPost { request_id: String, job_id: String, message: String, to: Option<String> },
    /// An agent reads its run's board, from after note `since`
    #[serde(rename = "board.read", rename_all = "camelCase")]
    BoardRead { request_id: String, job_id: String, since: Option<i64> },
    /// An agent asks another agent type a question; the answer comes back as colleague.answer
    #[serde(rename = "colleague.ask", rename_all = "camelCase")]
    ColleagueAsk { request_id: String, job_id: String, agent_type: String, question: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_hosts_lines() {
        let ready: HostMessage =
            serde_json::from_str(r#"{"type":"host.ready","protocol":1,"sdk":"0.3.331","claudeCode":"2.1.331"}"#).unwrap();
        assert!(matches!(ready, HostMessage::Ready { protocol: 1, .. }));
        let ask: HostMessage = serde_json::from_str(
            r#"{"type":"tool.ask","askId":"a","jobId":"j","toolName":"Write","input":{"file_path":"x"}}"#,
        )
        .unwrap();
        assert!(matches!(ask, HostMessage::ToolAsk { ref tool_name, .. } if tool_name == "Write"));
    }

    #[test]
    fn writes_what_the_host_expects() {
        let answer = CoreMessage::ToolAnswer { ask_id: "a".into(), behavior: Behavior::Deny, message: None };
        assert_eq!(
            serde_json::to_string(&answer).unwrap(),
            r#"{"type":"tool.answer","askId":"a","behavior":"deny","message":null}"#
        );
        let spec = AgentSpec { model: "m".into(), ask: AskPolicy::BeforeWrites, tools: vec![], mcp: vec![], instructions: None, board: BoardAccess::Post, colleagues: vec![] };
        let start = CoreMessage::JobStart { job_id: "j".into(), cwd: "c".into(), prompt: "p".into(), agent: spec, output_schema: None };
        assert!(serde_json::to_string(&start).unwrap().contains(r#""ask":"before-writes""#));
    }
}
