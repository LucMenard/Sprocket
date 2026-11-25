// Sprocket's own tools for an agent, served by an in-process MCP server named `sprocket`: the run's board
// (read_notes, post_note) and asking another agent type (ask_agent). The core owns all of it: every call is a
// request to it, answered by a board.answer or colleague.answer line.
import { randomUUID } from 'node:crypto'
import { createSdkMcpServer, tool, type McpSdkServerConfigWithInstance } from '@anthropic-ai/claude-agent-sdk'
import { z } from 'zod'
import type { AgentSpec, HostMessage } from './protocol.ts'

/** The server's name, as tools show it: `mcp__sprocket__post_note` */
export const BOARD_SERVER = 'sprocket'

/** Tools of our own server: never refused by the MCP filter, never sent to a human to approve. */
export const isBoardTool = (toolName: string) => toolName.startsWith(`mcp__${BOARD_SERVER}__`)

type Answer = { text: string; error: boolean }

/** Requests waiting for the core's answer, by request id. */
export class BoardRequests {
  private readonly waiting = new Map<string, (answer: Answer) => void>()

  constructor(private readonly send: (message: HostMessage) => void) {}

  ask(
    message:
      | { type: 'board.post'; jobId: string; message: string; to?: string }
      | { type: 'board.read'; jobId: string; since?: number }
      | { type: 'colleague.ask'; jobId: string; agentType: string; question: string },
  ): Promise<Answer> {
    const requestId = randomUUID()
    return new Promise((resolve) => {
      this.waiting.set(requestId, resolve)
      this.send({ ...message, requestId })
    })
  }

  answer(requestId: string, answer: Answer) {
    this.waiting.get(requestId)?.(answer)
    this.waiting.delete(requestId)
  }

  /** The core went away: nobody will answer */
  failAll() {
    for (const resolve of this.waiting.values()) resolve({ text: 'Sprocket is not answering', error: true })
    this.waiting.clear()
  }
}

const result = (a: Answer) => ({ content: [{ type: 'text' as const, text: a.text }], isError: a.error })

/** Whether a job gets the server at all: a board to use, or someone to ask */
export const needsServer = (agent: AgentSpec) => agent.board !== 'off' || agent.colleagues.length > 0

/**
 * The tools one job gets: the board's when its workflow has one (posting when its agent type may), and
 * ask_agent when it may ask someone.
 */
export function boardServer(jobId: string, agent: AgentSpec, requests: BoardRequests): McpSdkServerConfigWithInstance {
  const read = tool(
    'read_notes',
    "Reads the run's board: notes from earlier steps, from people following the run, and why a result was rejected. " +
      'Pass `since` (a note number) to get only newer notes.',
    { since: z.number().int().optional().describe('Only notes after this note number') },
    async ({ since }) => result(await requests.ask({ type: 'board.read', jobId, ...(since !== undefined ? { since } : {}) })),
    { alwaysLoad: true, annotations: { readOnlyHint: true } },
  )
  const post = tool(
    'post_note',
    "Posts a note on the run's board, for the next steps and the people following the run. Use it for what they should " +
      "know that your result doesn't say. `to` names a step or an agent type it is for; everyone when left out.",
    {
      message: z.string().min(1).max(4000).describe('The note'),
      to: z.string().optional().describe('A step or agent type it is for'),
    },
    async ({ message, to }) => result(await requests.ask({ type: 'board.post', jobId, message, ...(to ? { to } : {}) })),
    { alwaysLoad: true },
  )
  const who = agent.colleagues.map((c) => (c.description ? `${c.id} (${c.description})` : c.id)).join('; ')
  const ask = () =>
    tool(
    'ask_agent',
    `Asks another agent type of this workflow a question and waits for its answer. It works in the same folder and ` +
      `can read the ticket and the code; it can't see your conversation, so put what it needs in the question. ` +
      `You may ask: ${who}.`,
    {
      agentType: z.enum(agent.colleagues.map((c) => c.id) as [string, ...string[]]).describe('Who to ask'),
      question: z.string().min(1).max(8000).describe('The question, with what it needs to answer it'),
    },
    async ({ agentType, question }) => result(await requests.ask({ type: 'colleague.ask', jobId, agentType, question })),
    { alwaysLoad: true },
  )
  const tools = [
    ...(agent.board === 'off' ? [] : agent.board === 'post' ? [read, post] : [read]),
    ...(agent.colleagues.length ? [ask()] : []),
  ]
  return createSdkMcpServer({ name: BOARD_SERVER, version: '1.0.0', tools })
}
