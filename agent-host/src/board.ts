// The run's board, as tools of an in-process MCP server named `sprocket`. The core owns the notes: every
// call is a request to it, answered by a board.answer line.
import { randomUUID } from 'node:crypto'
import { createSdkMcpServer, tool, type McpSdkServerConfigWithInstance } from '@anthropic-ai/claude-agent-sdk'
import { z } from 'zod'
import type { BoardAccess, HostMessage } from './protocol.ts'

/** The server's name, as tools show it: `mcp__sprocket__post_note` */
export const BOARD_SERVER = 'sprocket'

/** Tools of our own server: never refused by the MCP filter, never sent to a human to approve. */
export const isBoardTool = (toolName: string) => toolName.startsWith(`mcp__${BOARD_SERVER}__`)

type Answer = { text: string; error: boolean }

/** Requests waiting for the core's answer, by request id. */
export class BoardRequests {
  private readonly waiting = new Map<string, (answer: Answer) => void>()

  constructor(private readonly send: (message: HostMessage) => void) {}

  ask(message: { type: 'board.post'; jobId: string; message: string; to?: string } | { type: 'board.read'; jobId: string; since?: number }): Promise<Answer> {
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

/** The board tools one job gets: reading always, posting when its agent type may. */
export function boardServer(jobId: string, access: BoardAccess, requests: BoardRequests): McpSdkServerConfigWithInstance {
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
  return createSdkMcpServer({ name: BOARD_SERVER, version: '1.0.0', tools: access === 'post' ? [read, post] : [read] })
}
