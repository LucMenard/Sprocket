//! Mirror of agent-host/src/protocol.ts: one JSON object per line over the host's stdio.
//! Change both together and bump PROTOCOL.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::workflow::AskPolicy;

pub const PROTOCOL: u32 = 1;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentSpec {
    pub model: String,
    pub ask: AskPolicy,
    pub tools: Vec<String>,
    /// MCP servers allowed; empty removes every MCP tool from the session
    pub mcp: Vec<String>,
    pub instructions: Option<String>,
}

/// Core → host.
#[derive(Serialize, Debug)]
#[serde(tag = "type")]
pub enum CoreMessage {
    #[serde(rename = "job.start", rename_all = "camelCase")]
    JobStart { job_id: String, cwd: String, prompt: String, agent: AgentSpec },
    #[serde(rename = "job.resume", rename_all = "camelCase")]
    JobResume { job_id: String, cwd: String, sdk_session_id: String, prompt: String, agent: AgentSpec },
    #[serde(rename = "job.stop", rename_all = "camelCase")]
    JobStop { job_id: String },
    #[serde(rename = "tool.answer", rename_all = "camelCase")]
    ToolAnswer { ask_id: String, behavior: Behavior, message: Option<String> },
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
    },
    #[serde(rename = "tool.ask", rename_all = "camelCase")]
    ToolAsk { ask_id: String, job_id: String, tool_name: String, input: Value },
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
        let spec = AgentSpec { model: "m".into(), ask: AskPolicy::BeforeWrites, tools: vec![], mcp: vec![], instructions: None };
        let start = CoreMessage::JobStart { job_id: "j".into(), cwd: "c".into(), prompt: "p".into(), agent: spec };
        assert!(serde_json::to_string(&start).unwrap().contains(r#""ask":"before-writes""#));
    }
}
