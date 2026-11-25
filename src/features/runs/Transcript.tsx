import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'

interface Block {
  type: string
  text?: string
  name?: string
  input?: Record<string, unknown>
  content?: unknown
  is_error?: boolean
}

interface SdkMessage {
  type: string
  subtype?: string
  message?: { content?: Block[] | string }
  result?: string
  model?: string
}

function short(input: Record<string, unknown> | undefined): string {
  if (!input) return ''
  for (const key of ['command', 'file_path', 'pattern', 'url', 'query', 'description']) {
    if (typeof input[key] === 'string') return input[key] as string
  }
  return JSON.stringify(input).slice(0, 160)
}

function resultText(content: unknown): string {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) return content.map((c) => (typeof c?.text === 'string' ? c.text : '')).join('\n')
  return ''
}

/** The agent session, simplified: what it said, the tools it used and what they returned. */
export function Transcript({ jobId, live }: { jobId: string; live: boolean }) {
  // Messages aren't announced one by one: a running job's transcript polls for them
  const { data } = useQuery({
    queryKey: ['job-messages', jobId],
    queryFn: () => api.jobMessages(jobId),
    refetchInterval: live ? 2000 : false,
  })
  const messages = (data ?? []) as SdkMessage[]
  if (messages.length === 0) return <p className="text-sm text-muted-foreground">Nothing yet.</p>

  const rows: { key: string; kind: 'text' | 'tool' | 'result' | 'meta'; text: string; error?: boolean }[] = []
  messages.forEach((m, i) => {
    if (m.type === 'system' && m.subtype === 'init') rows.push({ key: `${i}`, kind: 'meta', text: `Session started · ${m.model ?? ''}` })
    const content = Array.isArray(m.message?.content) ? m.message.content : []
    content.forEach((b, j) => {
      const key = `${i}.${j}`
      if (m.type === 'assistant' && b.type === 'text' && b.text?.trim()) rows.push({ key, kind: 'text', text: b.text })
      if (m.type === 'assistant' && b.type === 'tool_use') rows.push({ key, kind: 'tool', text: `${b.name}  ${short(b.input)}` })
      if (m.type === 'user' && b.type === 'tool_result') {
        const text = resultText(b.content).trim()
        if (text) rows.push({ key, kind: 'result', text: text.length > 600 ? `${text.slice(0, 600)}…` : text, error: b.is_error })
      }
    })
    if (m.type === 'result') rows.push({ key: `${i}`, kind: 'meta', text: `Finished: ${m.subtype}` })
  })

  return (
    <ol className="flex flex-col gap-2 text-sm">
      {rows.map((r) => (
        <li key={r.key}>
          {r.kind === 'text' && <p className="whitespace-pre-wrap">{r.text}</p>}
          {r.kind === 'tool' && <p className="truncate font-mono text-xs text-state-running">▸ {r.text}</p>}
          {r.kind === 'result' && (
            <pre
              className={`max-h-40 overflow-auto rounded-md bg-muted p-2 text-xs whitespace-pre-wrap ${r.error ? 'text-destructive' : 'text-muted-foreground'}`}
            >
              {r.text}
            </pre>
          )}
          {r.kind === 'meta' && <p className="text-xs text-muted-foreground">{r.text}</p>}
        </li>
      ))}
    </ol>
  )
}
