import { Check } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { useRuns } from '@/features/runs/useRuns'
import { useRunSelection } from '@/features/runs/useRunSelection'
import { useWorkflows } from '@/features/workflows/useWorkflows'
import { api, type AgentType, type AppError, type AskPolicy, type Workflow } from '@/lib/api'
import { useNav } from '@/lib/nav'
import { cn } from '@/lib/utils'

const ASK: { id: AskPolicy; label: string; help: string }[] = [
  { id: 'never', label: 'Never', help: 'Runs on its own' },
  { id: 'before-writes', label: 'Before writes', help: 'Read freely, ask before changing things' },
  { id: 'every-action', label: 'Every action', help: 'Ask before each tool call' },
]

/** An agent type as one workflow file defines it. */
interface Entry {
  key: string
  agent: AgentType
  workflow: Workflow
}

function Card({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section className="ct-card flex flex-col gap-4 p-5">
      {title && <h2 className="text-[15px] font-semibold">{title}</h2>}
      {children}
    </section>
  )
}

function ReadField({ label, children, mono }: { label: string; children: ReactNode; mono?: boolean }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="ct-label">{label}</span>
      <div className={cn('flex min-h-10 items-center rounded-lg border border-ct-control bg-ct-subtle px-3', mono && 'text-[13px] ct-mono')}>{children}</div>
    </div>
  )
}

/** The mockup's toggle chips, shown in their state (read-only). */
function Chips({ items, empty, mono }: { items: string[]; empty: string; mono?: boolean }) {
  if (items.length === 0) return <span className="text-[13px] text-ct-text-3">{empty}</span>
  return (
    <div className="flex flex-wrap gap-2">
      {items.map((t) => (
        <span
          key={t}
          className={cn('inline-flex min-h-9 items-center gap-1.5 rounded-[18px] border px-3.5 text-[13px] font-medium', mono && 'ct-mono')}
          style={{ background: 'var(--ct-accent-soft)', color: 'var(--ct-accent-text)', borderColor: 'var(--ct-accent-ring)' }}
        >
          <Check aria-hidden className="size-3.5" strokeWidth={2.4} />
          {t}
        </span>
      ))}
    </div>
  )
}

export function AgentsView() {
  const { data, error } = useWorkflows()
  const runs = useRuns().data ?? []
  const go = useNav((s) => s.go)
  const select = useRunSelection((s) => s.select)
  const entries: Entry[] = (data?.workflows ?? []).flatMap((workflow) =>
    workflow.agents.map((agent) => ({ key: `${workflow.id}/${agent.id}`, agent, workflow })),
  )
  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const entry = entries.find((e) => e.key === selectedKey) ?? entries[0]

  // Jobs this agent type is running or waiting in now
  const workingNow = (e: Entry) =>
    runs
      .filter((r) => r.workflowId === e.workflow.id)
      .flatMap((run) =>
        run.jobs
          .filter((j) => (j.state === 'running' || j.state === 'waiting') && run.steps.find((s) => s.id === j.stepId)?.agent === e.agent.id)
          .map((job) => ({ run, job })),
      )
  const now = entry ? workingNow(entry) : []
  const usedIn = entry ? entry.workflow.steps.filter((s) => s.agent === entry.agent.id) : []

  return (
    <div className="flex h-full min-h-0 flex-col">
      {error && <p className="p-3 text-ct-failed">{(error as unknown as AppError).message}</p>}
      <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
        <ResizablePanel id="agents-list" defaultSize="24%" minSize="16%">
          <nav aria-label="Agent types" className="flex h-full flex-col gap-1 overflow-y-auto bg-ct-surface px-2.5 py-3.5">
            <div className="flex items-center justify-between px-2 pb-2">
              <span className="ct-eyebrow">Agent types</span>
              <span className="text-xs text-ct-text-3">{entries.length}</span>
            </div>
            {entries.map((e) => {
              const busy = workingNow(e)
              return (
                <button key={e.key} type="button" className="ct-row min-h-[52px]" aria-current={e.key === entry?.key ? 'true' : undefined} onClick={() => setSelectedKey(e.key)}>
                  <span aria-hidden className="size-2 shrink-0 rounded-full" style={{ background: busy.length ? 'var(--ct-accent)' : 'var(--ct-idle)' }} />
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate font-semibold">{e.agent.id}</span>
                    <span className="truncate text-xs text-ct-text-3">
                      {e.agent.model} · {e.workflow.name}
                    </span>
                  </span>
                  <span className="text-xs whitespace-nowrap text-ct-text-2">{busy.length ? `${busy.length} running` : ''}</span>
                </button>
              )
            })}
            {data && entries.length === 0 && <p className="px-2 text-ct-text-3">Agent types are defined in workflow files.</p>}
          </nav>
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="agents-detail" defaultSize="52%" minSize="30%">
          {entry && (
            <main className="flex h-full flex-col gap-[22px] overflow-y-auto px-9 py-7">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div className="flex flex-col gap-1.5">
                  <span className="text-[13px] text-ct-text-3">Agent type</span>
                  <h1 className="text-[26px] leading-tight font-bold tracking-[-0.015em]">{entry.agent.id}</h1>
                </div>
                <button type="button" className="ct-btn ct-btn-primary" onClick={() => void api.openWorkflowFile(entry.workflow.file)}>
                  Open file
                </button>
              </div>
              <Card title="General">
                <div className="grid grid-cols-[repeat(auto-fit,minmax(200px,1fr))] gap-4">
                  <ReadField label="Name">{entry.agent.id}</ReadField>
                  <ReadField label="Model" mono>
                    {entry.agent.model}
                  </ReadField>
                  <ReadField label="Defined in">{entry.workflow.name}</ReadField>
                </div>
              </Card>
              <Card title="Access">
                <div className="flex flex-col gap-2">
                  <span className="ct-label">MCP servers it can use</span>
                  <Chips items={entry.agent.mcp} empty="None: no MCP tool is loaded" />
                </div>
                <div className="flex flex-col gap-2">
                  <span className="ct-label">Tools</span>
                  <Chips items={entry.agent.tools} empty="All of Claude Code's tools" mono />
                </div>
                <fieldset className="flex flex-col gap-2">
                  <legend className="mb-2 ct-label">Ask a human before</legend>
                  <div className="grid grid-cols-[repeat(auto-fit,minmax(180px,1fr))] gap-2">
                    {ASK.map((a) => {
                      const on = a.id === entry.agent.ask
                      return (
                        <label
                          key={a.id}
                          className="flex items-start gap-2.5 rounded-lg border px-3 py-2.5"
                          style={on ? { background: 'var(--ct-accent-soft)', borderColor: 'var(--ct-accent-ring)' } : { borderColor: 'var(--ct-control)', opacity: 0.6 }}
                        >
                          <input type="radio" className="mt-0.5 accent-[var(--ct-accent)]" checked={on} readOnly disabled={!on} />
                          <span className="flex flex-col gap-0.5">
                            <span className="font-medium">{a.label}</span>
                            <span className="text-xs text-ct-text-3">{a.help}</span>
                          </span>
                        </label>
                      )
                    })}
                  </div>
                </fieldset>
              </Card>
              <Card>
                <span className="ct-label">Instructions</span>
                <div className="min-h-24 rounded-lg border border-ct-control bg-ct-subtle px-3 py-2.5 leading-normal whitespace-pre-wrap">
                  {entry.agent.instructions ?? <span className="text-ct-text-3">None besides each step's own.</span>}
                </div>
              </Card>
              <p className="text-xs text-ct-text-3">Read-only for now: "Open file" edits the YAML.</p>
            </main>
          )}
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="agents-side" defaultSize="24%" minSize="18%">
          {entry && (
            <aside aria-label="Details" className="flex h-full flex-col gap-4 overflow-y-auto bg-ct-surface px-4 py-[18px]">
              <span className="ct-eyebrow">Usage</span>
              <div className="flex flex-col gap-1 rounded-[10px] border border-ct-border p-3.5">
                <span className="text-[13px] text-ct-text-3">Running now</span>
                <span className="text-2xl font-bold">{now.length}</span>
                <span className="text-xs text-ct-text-3">of {entry.workflow.maxParallelRuns} allowed at once</span>
              </div>
              {now.map(({ run, job }) => (
                <button
                  key={job.id}
                  type="button"
                  className="flex min-h-11 items-center gap-2 rounded-lg border border-ct-border px-3 text-left hover:bg-ct-subtle"
                  onClick={() => {
                    select(run.id)
                    go('runs')
                  }}
                >
                  <span className="font-medium ct-mono">{run.ticket.key}</span>
                  <span className="truncate text-xs text-ct-text-3">{job.stepId}</span>
                </button>
              ))}
              <div className="flex flex-col gap-2">
                <span className="text-[13px] font-semibold">Used in workflows</span>
                <div className="flex min-h-11 items-center gap-2 rounded-lg border border-ct-border px-3">
                  <span className="flex-1 font-medium">{entry.workflow.name}</span>
                  <span className="text-xs text-ct-text-3">{usedIn.map((s) => s.id).join(', ') || 'no step'}</span>
                </div>
              </div>
            </aside>
          )}
        </ResizablePanel>
      </ResizablePanelGroup>
    </div>
  )
}
