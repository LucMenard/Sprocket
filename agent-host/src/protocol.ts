// The messages between the Rust core and this host: one JSON object per line, core → host on stdin,
// host → core on stdout. Nothing else may be written to stdout; logs go to stderr.
// The Rust side mirrors these in src-tauri/src/host/protocol.rs. Bump PROTOCOL when either changes.

export const PROTOCOL = 3

export type AskPolicy = 'never' | 'before-writes' | 'every-action'

export interface AgentSpec {
  model: string
  ask: AskPolicy
  /** Tools the agent may use; empty means Claude Code's defaults */
  tools: string[]
  /** MCP servers the agent may use, by their name in Claude Code; empty removes every MCP tool */
  mcp: string[]
  instructions: string | null
  /** The run's board: no tools, reading only, or reading and posting */
  board: BoardAccess
}

export type BoardAccess = 'off' | 'read' | 'post'

/** The JSON schema of a step's output, when it asks for one */
export type JsonSchema = Record<string, unknown>

export type CoreMessage =
  | { type: 'job.start'; jobId: string; cwd: string; prompt: string; agent: AgentSpec; outputSchema: JsonSchema | null }
  | { type: 'job.resume'; jobId: string; cwd: string; sdkSessionId: string; prompt: string; agent: AgentSpec; outputSchema: JsonSchema | null }
  | { type: 'job.stop'; jobId: string }
  | { type: 'tool.answer'; askId: string; behavior: 'allow' | 'deny'; message?: string }
  | { type: 'board.answer'; requestId: string; text: string; error: boolean }

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
      /** The structured output, when the job had an output schema */
      data?: unknown
    }
  | { type: 'tool.ask'; askId: string; jobId: string; toolName: string; input: unknown }
  | { type: 'board.post'; requestId: string; jobId: string; message: string; to?: string }
  | { type: 'board.read'; requestId: string; jobId: string; since?: number }
