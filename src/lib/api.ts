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
  /** May post to its run's board (it can always read it) */
  board: boolean
  /** May ask other agent types a question (ask_agent) */
  colleagues: boolean
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
export type AgentDraft = Pick<AgentType, 'description' | 'model' | 'ask' | 'tools' | 'mcp' | 'instructions' | 'board' | 'colleagues'>

/** What the workflow editor saves; the workflow's own agent types are kept from its file */
export interface WorkflowDraft {
  name: string
  description: string
  offerInTracket: boolean
  /** Runs what someone asks in words (Tracket's "Ask an agent"), read as `{{ request }}` */
  forRequests: boolean
  maxParallelRuns: number
  /** A board its runs' agents and people share */
  board: boolean
  steps: WorkflowStep[]
  /** Nodes placed by hand in the editor, by step id (and `$trigger`, `$end`) */
  layout: Record<string, Point>
  /** Who the steps may ask with ask_agent, and how many times a step may */
  colleagues: Colleagues
  /** The most a team step may use */
  teamLimits: TeamLimits
  /**
   * The editor's own: chains of nodes no link reaches, after a link was deleted. Never saved: the editor won't
   * save while there are any.
   */
  loose: WorkflowStep[][]
  /** The editor's own: links deleted that only followed from the others (`from>to`); never saved either */
  cut: string[]
  /** The editor's own: links drawn that the workflow can't hold (`from>to`); saving waits until they go */
  extra: string[]
}

export interface Colleagues {
  canAsk: string[]
  maxAsks: number
}

export interface Point {
  x: number
  y: number
}

export type StepKind = 'agent' | 'approval' | 'action' | 'condition' | 'forEach' | 'parallel' | 'team'
export type OutputType = 'text' | 'number' | 'yes/no' | 'list'
export type ActionKind = 'comment' | 'status' | 'slack' | 'other'

export interface StepAction {
  do: ActionKind
  /** The message (or, for `other`, what to do); `{{ … }}` is filled in */
  text?: string
  /** The status, or the Slack channel */
  to?: string | null
}

/**
 * A step of a workflow. Which fields count depends on `kind`; the others are left empty.
 * Conditions hold their branches (`then`, `else`) and loops their steps (`steps`).
 */
export interface WorkflowStep {
  id: string
  kind: StepKind
  /** The agent type that does an agent step or an action */
  agent: string | null
  instructions: string | null
  /** For agent steps, what the agent gets (`steps.plan.output`); for approvals, the summary shown */
  input: string | null
  onReject: string | null
  /** An agent step's JSON output fields; none: it answers in text */
  output: { name: string; type: OutputType }[]
  action: StepAction | null
  if: string | null
  then: WorkflowStep[]
  else: WorkflowStep[]
  forEach: string | null
  /** The name of a loop's item, `item` by default */
  as: string | null
  parallel: number
  onItemFailure: 'stop' | 'continue'
  steps: WorkflowStep[]
  /** A parallel group's branches: they run at the same time, and the next step waits for all of them */
  branches: WorkflowStep[][]
  /** A team step: agent types working on it together over the board */
  team: Team | null
}

export interface TeamLimits {
  /** Turns of each member's session */
  maxTurns: number
  /** Dollars each member may spend */
  budgetUsd: number
  /** Minutes each member may work; waiting for a person doesn't count */
  minutes: number
}

export interface Team {
  members: string[]
  /** The member that ends the step, with finish(summary) */
  lead: string
  /** The one member that may change files; null: nobody does */
  writer: string | null
  goal: string
  limits: TeamLimits
}

export interface Workflow {
  id: string
  name: string
  description: string
  offerInTracket: boolean
  forRequests: boolean
  maxParallelRuns: number
  board: boolean
  layout: Record<string, Point>
  colleagues: Colleagues
  teamLimits: TeamLimits
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
  /** `ask`: another agent type answering a question a step asked (its `parentJob`) */
  kind: 'agent' | 'approval' | 'action' | 'condition' | 'loop' | 'ask' | 'team' | 'member'
  /** `""` in the workflow itself; `each[2]/` for the third item of the loop `each` */
  scope: string
  /** Redone since (after a rejection or a retry): history only */
  superseded: boolean
  /** A JSON output; a loop's items */
  data: unknown
  /** For an ask: the job that asked, and its question */
  parentJob: string | null
  question: string | null
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
  /** What someone asked for, on a run started from a request */
  request: string | null
  state: RunState
  startedAt: string
  endedAt: string | null
  steps: WorkflowStep[]
  /** Whether its workflow has a board */
  board: boolean
  /** The board, oldest first */
  notes: Note[]
  jobs: Job[]
  /** Pending only */
  approvals: Approval[]
  costUsd: number
}

/** A note on a run's board, by one of its agents or by a person. */
export interface Note {
  id: number
  at: string
  /** The step that posted it (`fix (each 2)`), or the person */
  author: string
  /** The agent type; null for a person */
  agent: string | null
  jobId: string | null
  /** A step or an agent type it is for; everyone when null */
  to: string | null
  body: string
}

export interface StartRunRequest {
  workflow: string
  ticket: Ticket
  folder: string
  startedBy: string
  /** What you want done, for a workflow that runs requests */
  request?: string
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
  postNote: (runId: string, body: string, to: string | null) => call<Note>('post_note', { runId, body, to }),
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
