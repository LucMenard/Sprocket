// The messages between the Rust core and this host: one JSON object per line, core → host on stdin,
// host → core on stdout. Nothing else may be written to stdout; logs go to stderr.
// The Rust side mirrors these in src-tauri/src/host/protocol.rs. Bump PROTOCOL when either changes.

export const PROTOCOL = 1

export type AskPolicy = 'never' | 'before-writes' | 'every-action'

export interface AgentSpec {
  model: string
  ask: AskPolicy
  /** Tools the agent may use; empty means Claude Code's defaults */
  tools: string[]
  /** MCP servers the agent may use, by their name in Claude Code; empty removes every MCP tool */
  mcp: string[]
  instructions: string | null
}

export type CoreMessage =
  | { type: 'job.start'; jobId: string; cwd: string; prompt: string; agent: AgentSpec }
  | { type: 'job.resume'; jobId: string; cwd: string; sdkSessionId: string; prompt: string; agent: AgentSpec }
  | { type: 'job.stop'; jobId: string }
  | { type: 'tool.answer'; askId: string; behavior: 'allow' | 'deny'; message?: string }

export type HostMessage =
  | { type: 'host.ready'; protocol: number; sdk: string; claudeCode: string }
  | { type: 'host.error'; message: string }
  | { type: 'job.started'; jobId: string; sdkSessionId: string }
  | { type: 'job.message'; jobId: string; message: unknown }
  | {
      type: 'job.ended'
      jobId: string
      result: 'success' | 'error' | 'stopped'
      output: string | null
      costUsd: number | null
      error?: string
    }
  | { type: 'tool.ask'; askId: string; jobId: string; toolName: string; input: unknown }
