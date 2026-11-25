// One Agent SDK session per job. The policy of the job's agent type decides which tool calls are
// sent to the core as tool asks.
import { randomUUID } from 'node:crypto'
import { query, type CanUseTool, type HookCallback, type Options, type PermissionResult } from '@anthropic-ai/claude-agent-sdk'
import type { AgentSpec, CoreMessage, HostMessage } from './protocol.ts'

/** Tools that only read: "before-writes" lets them through without asking. */
const READ_ONLY = new Set(['Read', 'Grep', 'Glob', 'LS', 'WebSearch', 'WebFetch', 'ToolSearch', 'TodoWrite'])
/** MCP tools whose name says they only read: `mcp__Atlassian2__getJiraIssue`, `…__searchConfluence` */
const MCP_READ = /^(get|list|search|read|fetch|find|query|lookup|discover|view|describe)|UserInfo$/i

/** `mcp__<server>__<tool>`: the server's name as Claude Code writes it (spaces and dots become `_`). */
function mcpParts(toolName: string): { server: string; tool: string } | null {
  const match = /^mcp__(.+?)__(.+)$/.exec(toolName)
  return match ? { server: match[1], tool: match[2] } : null
}

const normalize = (server: string) => server.replace(/[^A-Za-z0-9_-]/g, '_')

export function isReadOnly(toolName: string): boolean {
  const mcp = mcpParts(toolName)
  return mcp ? MCP_READ.test(mcp.tool) : READ_ONLY.has(toolName)
}

export function mcpAllowed(toolName: string, servers: string[]): boolean {
  const mcp = mcpParts(toolName)
  return !mcp || servers.some((s) => normalize(s) === mcp.server)
}

export interface HostConfig {
  /** Always set, so the dev build and the bundled exe run the same Claude Code */
  claudePath: string
  settingSources: Array<'user' | 'project' | 'local'>
}

type Send = (message: HostMessage) => void

export class Jobs {
  private readonly running = new Map<string, AbortController>()
  private readonly asks = new Map<string, (result: PermissionResult) => void>()

  constructor(
    private readonly config: HostConfig,
    private readonly send: Send,
  ) {}

  handle(message: CoreMessage) {
    switch (message.type) {
      case 'job.start':
        return void this.run(message.jobId, message.cwd, message.prompt, message.agent)
      case 'job.resume':
        return void this.run(message.jobId, message.cwd, message.prompt, message.agent, message.sdkSessionId)
      case 'job.stop':
        return this.running.get(message.jobId)?.abort()
      case 'tool.answer': {
        const resolve = this.asks.get(message.askId)
        this.asks.delete(message.askId)
        return resolve?.(
          message.behavior === 'allow'
            ? { behavior: 'allow' }
            : { behavior: 'deny', message: message.message ?? 'Denied in Sprocket' },
        )
      }
    }
  }

  /** Stops every job, for when the core goes away. */
  stopAll() {
    for (const controller of this.running.values()) controller.abort()
  }

  private askCore(jobId: string, toolName: string, input: unknown, signal: AbortSignal): Promise<PermissionResult> {
    const askId = randomUUID()
    return new Promise((resolve) => {
      this.asks.set(askId, resolve)
      signal.addEventListener('abort', () => {
        this.asks.delete(askId)
        resolve({ behavior: 'deny', message: 'The job was stopped' })
      })
      this.send({ type: 'tool.ask', askId, jobId, toolName, input })
    })
  }

  private options(jobId: string, cwd: string, agent: AgentSpec, abortController: AbortController, resume?: string): Options {
    const canUseTool: CanUseTool = async (toolName, input, { signal }) => {
      if (agent.ask === 'never') return { behavior: 'allow', updatedInput: input }
      if (agent.ask === 'before-writes' && isReadOnly(toolName)) return { behavior: 'allow', updatedInput: input }
      return this.askCore(jobId, toolName, input, signal)
    }
    // Runs before Claude Code's own permission rules. It refuses MCP servers the agent type doesn't
    // list (allow rules in the user's settings would otherwise let them through), and for
    // "every-action" turns every call into a prompt, since canUseTool only hears about calls that prompt.
    const preToolUse: HookCallback = async (hookInput) => {
      const toolName = 'tool_name' in hookInput ? hookInput.tool_name : ''
      if (!mcpAllowed(toolName, agent.mcp)) {
        return {
          hookSpecificOutput: {
            hookEventName: 'PreToolUse',
            permissionDecision: 'deny',
            permissionDecisionReason: 'This agent may not use that MCP server',
          },
        }
      }
      if (agent.ask === 'every-action') {
        return { hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'ask' } }
      }
      return {}
    }

    return {
      cwd,
      abortController,
      pathToClaudeCodeExecutable: this.config.claudePath,
      mcpServersDelivery: 'initialize',
      settingSources: this.config.settingSources,
      permissionMode: 'default',
      model: agent.model,
      canUseTool,
      ...(agent.tools.length ? { tools: agent.tools } : {}),
      // No MCP server listed: none of their tools is even loaded, which also saves their context
      ...(agent.mcp.length ? {} : { disallowedTools: ['mcp__*'] }),
      ...(agent.instructions
        ? { systemPrompt: { type: 'preset', preset: 'claude_code', append: agent.instructions } }
        : {}),
      hooks: { PreToolUse: [{ hooks: [preToolUse] }] },
      ...(resume ? { resume } : {}),
      stderr: (line) => process.stderr.write(`[${jobId}] ${line}\n`),
    }
  }

  private async run(jobId: string, cwd: string, prompt: string, agent: AgentSpec, resume?: string) {
    if (this.running.has(jobId)) return
    const abortController = new AbortController()
    this.running.set(jobId, abortController)
    let started = false
    let ended = false
    try {
      for await (const message of query({ prompt, options: this.options(jobId, cwd, agent, abortController, resume) })) {
        if (!started && message.type === 'system' && message.subtype === 'init') {
          started = true
          this.send({ type: 'job.started', jobId, sdkSessionId: message.session_id })
        }
        this.send({ type: 'job.message', jobId, message })
        if (message.type === 'result') {
          ended = true
          const ok = message.subtype === 'success'
          this.send({
            type: 'job.ended',
            jobId,
            result: ok ? 'success' : 'error',
            output: ok ? message.result : null,
            costUsd: message.total_cost_usd,
            ...(ok ? {} : { error: message.subtype }),
          })
        }
      }
      // The session ended without a result: the job must still end, or it would hold its run forever
      if (!ended) {
        const stopped = abortController.signal.aborted
        this.send({ type: 'job.ended', jobId, result: stopped ? 'stopped' : 'error', output: null, costUsd: null,
          ...(stopped ? {} : { error: 'The agent session ended without a result' }) })
      }
    } catch (error) {
      const stopped = abortController.signal.aborted
      this.send({
        type: 'job.ended',
        jobId,
        result: stopped ? 'stopped' : 'error',
        output: null,
        costUsd: null,
        ...(stopped ? {} : { error: String(error) }),
      })
    } finally {
      this.running.delete(jobId)
    }
  }
}
