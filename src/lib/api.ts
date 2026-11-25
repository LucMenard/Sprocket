import { invoke } from '@tauri-apps/api/core'

/** Errors from the Rust side arrive as `{ kind, message }`. */
export interface AppError {
  kind: string
  message: string
}

export type HostState =
  | { state: 'starting' }
  | { state: 'ready'; protocol: number; sdk: string; claudeCode: string }
  | { state: 'failed'; message: string }
  | { state: 'stopped' }

export interface AppStatus {
  version: string
  /** `127.0.0.1:<port>`, or null when the API couldn't start */
  apiAddress: string | null
  apiError: string | null
  host: HostState
  dataDir: string
}

export type AskPolicy = 'never' | 'before-writes' | 'every-action'

export interface AgentType {
  id: string
  description: string | null
  model: string
  ask: AskPolicy
  /** Claude Code's tools it may use; all of them when empty */
  tools: string[]
  /** MCP servers it may use; none when empty */
  mcp: string[]
  instructions: string | null
  /** A shared agent type (agents/<id>.yaml), or one defined in a workflow file */
  shared: boolean
}

export interface SharedAgent extends AgentType {
  file: string
}

export interface AgentList {
  agents: SharedAgent[]
  problems: WorkflowProblem[]
}

/** What the agent type editor saves */
export type AgentDraft = Pick<AgentType, 'description' | 'model' | 'ask' | 'tools' | 'mcp' | 'instructions'>

/** What the workflow editor saves; the workflow's own agent types are kept from its file */
export interface WorkflowDraft {
  name: string
  description: string
  offerInTracket: boolean
  maxParallelRuns: number
  steps: WorkflowStep[]
}

export interface WorkflowStep {
  id: string
  kind: 'agent' | 'approval'
  /** Agent type id, for agent steps */
  agent: string | null
  instructions: string | null
  input: string | null
  onReject: string | null
}

export interface Workflow {
  id: string
  name: string
  description: string
  offerInTracket: boolean
  maxParallelRuns: number
  file: string
  agents: AgentType[]
  steps: WorkflowStep[]
}

/** A workflow file that couldn't be read, with why. */
export interface WorkflowProblem {
  file: string
  message: string
}

export interface WorkflowList {
  workflows: Workflow[]
  problems: WorkflowProblem[]
}

export type RunState = 'running' | 'waiting' | 'passed' | 'failed' | 'cancelled' | 'interrupted'
export type JobState = 'running' | 'waiting' | 'done' | 'failed' | 'rejected' | 'stopped' | 'interrupted'

export interface Ticket {
  key: string
  title: string
  url: string
}

export interface Job {
  id: string
  stepId: string
  kind: 'agent' | 'approval'
  state: JobState
  attempt: number
  output: string | null
  error: string | null
  costUsd: number | null
  startedAt: string | null
  endedAt: string | null
  hasSession: boolean
  /** Oldest first: started, tool asks and answers, done or failed */
  log: LogLine[]
}

export interface LogLine {
  at: string
  what: string
  detail: string | null
}

export interface ActivityItem extends LogLine {
  runId: string | null
  ticketKey: string | null
  workflowName: string | null
  number: number | null
}

export interface McpServers {
  /** When the agent session they were read from started; null until an agent has run */
  seenAt: string | null
  servers: { name: string; status: string; source: string | null }[]
}

export interface ApiCall {
  /** Milliseconds since 1970 */
  at: number
  method: string
  path: string
  status: number
}

export interface Approval {
  id: string
  /** `gate`: an approval step. `tool`: an agent asking before a tool call */
  kind: 'gate' | 'tool'
  jobId: string | null
  summary: string
  toolName: string | null
  input: unknown
  createdAt: string
}

export interface Run {
  id: string
  workflowId: string
  workflowName: string
  number: number
  ticket: Ticket
  folder: string
  startedBy: string
  state: RunState
  startedAt: string
  endedAt: string | null
  steps: WorkflowStep[]
  jobs: Job[]
  /** Pending only */
  approvals: Approval[]
  costUsd: number
}

export interface StartRunRequest {
  workflow: string
  ticket: Ticket
  folder: string
  startedBy: string
}

export const inTauri = '__TAURI_INTERNALS__' in window

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) throw { kind: 'notInApp', message: 'Open this in the Sprocket app' } satisfies AppError
  return invoke<T>(command, args)
}

export const api = {
  status: () => call<AppStatus>('app_status'),
  workflows: () => call<WorkflowList>('list_workflows'),
  /** The bearer token Tracket needs, from the keychain */
  apiToken: () => call<string>('api_token'),
  openDataDir: () => call<void>('open_data_dir'),
  runs: (ticket?: string) => call<Run[]>('list_runs', { ticket: ticket ?? null }),
  startRun: (request: StartRunRequest) => call<Run>('start_run', { request }),
  decide: (id: string, approve: boolean, reason?: string) =>
    call<void>('decide_approval', { id, approve, reason: reason || null }),
  cancelRun: (id: string) => call<void>('cancel_run', { id }),
  retryJob: (id: string) => call<void>('retry_job', { id }),
  /** The agent session's messages, as the Agent SDK sent them */
  jobMessages: (id: string) => call<unknown[]>('job_messages', { id }),
  activity: (limit = 30) => call<ActivityItem[]>('activity', { limit }),
  mcpServers: () => call<McpServers>('mcp_servers'),
  apiCalls: () => call<ApiCall[]>('api_calls'),
  agents: () => call<AgentList>('list_agents'),
  saveWorkflow: (id: string, draft: WorkflowDraft) => call<Workflow>('save_workflow', { id, draft }),
  createWorkflow: (name: string) => call<Workflow>('create_workflow', { name }),
  deleteWorkflow: (id: string) => call<void>('delete_workflow', { id }),
  saveAgent: (id: string, draft: AgentDraft, create: boolean) => call<AgentType>('save_agent', { id, draft, create }),
  deleteAgent: (id: string) => call<void>('delete_agent', { id }),
  /** Moves an agent type defined in a workflow file to the shared ones */
  shareAgent: (workflow: string, id: string) => call<AgentType>('share_agent', { workflow, id }),
  /** A workflow's YAML file, in the default editor */
  openWorkflowFile: (file: string) => call<void>('open_workflow_file', { file }),
}
