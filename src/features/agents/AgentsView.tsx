import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { useRuns } from '@/features/runs/useRuns'
import { useRunSelection } from '@/features/runs/useRunSelection'
import { useWorkflows } from '@/features/workflows/useWorkflows'
import { api, type AgentType, type AppError } from '@/lib/api'
import { useNav } from '@/lib/nav'
import { useMcpServers } from '@/lib/queries'
import { AgentEditor } from './AgentEditor'
import { useAgents } from './useAgents'

const NEW_AGENT = { description: null, model: 'claude-sonnet-5-5', ask: 'before-writes' as const, tools: [], mcp: [], instructions: null }

export function AgentsView() {
  const queryClient = useQueryClient()
  const agents = useAgents()
  const workflows = useWorkflows().data?.workflows ?? []
  const runs = useRuns().data ?? []
  const mcpServers = (useMcpServers().data?.servers ?? []).map((s) => s.name)
  const go = useNav((s) => s.go)
  const selectRun = useRunSelection((s) => s.select)
  const shared = agents.data?.agents ?? []
  // Agent types still defined inside a workflow file
  const local = workflows.flatMap((w) => w.agents.filter((a) => !a.shared).map((agent) => ({ workflow: w, agent })))
  const [selected, setSelected] = useState<string | null>(null)
  const [newName, setNewName] = useState<string | null>(null)
  const current = shared.find((a) => `shared/${a.id}` === selected) ?? (selected ? undefined : shared[0])
  const currentLocal = local.find((l) => `local/${l.workflow.id}/${l.agent.id}` === selected)

  const reload = () => {
    void queryClient.invalidateQueries({ queryKey: ['agents'] })
    void queryClient.invalidateQueries({ queryKey: ['workflows'] })
  }
  const create = useMutation<AgentType, AppError, string>({
    mutationFn: (name) => api.saveAgent(name.trim().toLowerCase().replace(/[^a-z0-9_-]+/g, '-').replace(/^-+|-+$/g, ''), NEW_AGENT, true),
    onSuccess: (a) => {
      setNewName(null)
      setSelected(`shared/${a.id}`)
      reload()
    },
  })
  const share = useMutation<AgentType, AppError, { workflow: string; id: string }>({
    mutationFn: ({ workflow, id }) => api.shareAgent(workflow, id),
    onSuccess: (a) => {
      setSelected(`shared/${a.id}`)
      reload()
    },
  })

  const shown = current ?? currentLocal?.agent
  const usedIn = shown ? workflows.filter((w) => w.steps.some((s) => s.agent === shown.id) && w.agents.some((a) => a.id === shown.id && a.shared === shown.shared)) : []
  const working = shown
    ? runs.flatMap((run) =>
        usedIn.some((w) => w.id === run.workflowId)
          ? run.jobs.filter((j) => (j.state === 'running' || j.state === 'waiting') && run.steps.find((s) => s.id === j.stepId)?.agent === shown.id).map((job) => ({ run, job }))
          : [],
      )
    : []

  return (
    <div className="flex h-full min-h-0 flex-col">
      {agents.error && <p className="p-3 text-ct-failed">{(agents.error as unknown as AppError).message}</p>}
      <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
        <ResizablePanel id="agents-list" defaultSize="24%" minSize="16%">
          <nav aria-label="Agent types" className="flex h-full flex-col gap-1 overflow-y-auto bg-ct-surface px-2.5 py-3.5">
            <div className="flex items-center justify-between px-2 pb-2">
              <span className="ct-eyebrow">Agent types</span>
              <button type="button" className="ct-btn ct-btn-ghost h-7 px-2 text-xs" onClick={() => setNewName('')}>
                + New
              </button>
            </div>
            {newName !== null && (
              <form
                className="flex gap-1.5 px-1 pb-2"
                onSubmit={(e) => {
                  e.preventDefault()
                  if (newName.trim()) create.mutate(newName)
                }}
              >
                <input className="ct-input h-9" autoFocus placeholder="reviewer" value={newName} onChange={(e) => setNewName(e.target.value)} />
                <button type="submit" className="ct-btn ct-btn-sm ct-btn-primary" disabled={create.isPending}>
                  Create
                </button>
              </form>
            )}
            {create.error && <p className="px-2 pb-2 text-xs text-ct-failed">{create.error.message}</p>}
            {shared.map((a) => (
              <button key={a.id} type="button" className="ct-row min-h-[52px]" aria-current={a === current ? 'true' : undefined} onClick={() => setSelected(`shared/${a.id}`)}>
                <span aria-hidden className="size-2 shrink-0 rounded-full" style={{ background: 'var(--ct-idle)' }} />
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="truncate font-semibold">{a.id}</span>
                  <span className="truncate text-xs text-ct-text-3">{a.model}</span>
                </span>
              </button>
            ))}
            {agents.data?.problems.map((p) => (
              <p key={p.file} className="mx-2 rounded-lg border p-2 text-xs" style={{ borderColor: 'var(--ct-danger-border)', color: 'var(--ct-danger)' }}>
                {p.file.split(/[\\/]/).at(-1)}: {p.message}
              </p>
            ))}

            {local.length > 0 && (
              <>
                <span className="mt-3 px-2 pb-1 ct-eyebrow">In workflow files</span>
                {local.map(({ workflow, agent }) => {
                  const key = `local/${workflow.id}/${agent.id}`
                  return (
                    <button key={key} type="button" className="ct-row min-h-[52px]" aria-current={key === selected ? 'true' : undefined} onClick={() => setSelected(key)}>
                      <span aria-hidden className="size-2 shrink-0 rounded-full border-2" style={{ borderColor: 'var(--ct-idle)' }} />
                      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                        <span className="truncate font-semibold">{agent.id}</span>
                        <span className="truncate text-xs text-ct-text-3">
                          {agent.model} · {workflow.name}
                        </span>
                      </span>
                    </button>
                  )
                })}
              </>
            )}
          </nav>
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="agents-detail" defaultSize="52%" minSize="30%">
          <main className="h-full overflow-y-auto px-9 py-7">
            {current && <AgentEditor key={current.id} agent={current} mcpServers={mcpServers} usedBy={usedIn.map((w) => w.name)} onDeleted={() => setSelected(null)} />}
            {!current && currentLocal && (
              <div className="flex flex-col gap-4">
                <span className="text-[13px] text-ct-text-3">Agent type in {currentLocal.workflow.name}</span>
                <h1 className="text-[26px] leading-tight font-bold">{currentLocal.agent.id}</h1>
                <section className="ct-card flex flex-col gap-3 p-5">
                  <p>
                    This agent type is defined inside the workflow file, so only that workflow can use it and it can't be edited
                    here. Move it to the shared agent types to edit it and use it in other workflows.
                  </p>
                  <p className="text-[13px] text-ct-text-3">
                    {currentLocal.agent.model} · ask {currentLocal.agent.ask} · tools {currentLocal.agent.tools.join(', ') || 'all'} · MCP{' '}
                    {currentLocal.agent.mcp.join(', ') || 'none'}
                  </p>
                  <button
                    type="button"
                    className="ct-btn ct-btn-primary self-start"
                    disabled={share.isPending}
                    onClick={() => share.mutate({ workflow: currentLocal.workflow.id, id: currentLocal.agent.id })}
                  >
                    Move to shared agent types
                  </button>
                  {share.error && <p className="text-[13px] text-ct-failed">{share.error.message}</p>}
                </section>
              </div>
            )}
            {!shown && agents.data && <p className="text-ct-text-3">No agent type yet: create one with + New.</p>}
          </main>
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="agents-side" defaultSize="24%" minSize="18%">
          {shown && (
            <aside aria-label="Details" className="flex h-full flex-col gap-4 overflow-y-auto bg-ct-surface px-4 py-[18px]">
              <span className="ct-eyebrow">Usage</span>
              <div className="flex flex-col gap-1 rounded-[10px] border border-ct-border p-3.5">
                <span className="text-[13px] text-ct-text-3">Running now</span>
                <span className="text-2xl font-bold">{working.length}</span>
              </div>
              {working.map(({ run, job }) => (
                <button
                  key={job.id}
                  type="button"
                  className="flex min-h-11 items-center gap-2 rounded-lg border border-ct-border px-3 text-left hover:bg-ct-subtle"
                  onClick={() => {
                    selectRun(run.id)
                    go('runs')
                  }}
                >
                  <span className="font-medium ct-mono">{run.ticket.key}</span>
                  <span className="truncate text-xs text-ct-text-3">{job.stepId}</span>
                </button>
              ))}
              <div className="flex flex-col gap-2">
                <span className="text-[13px] font-semibold">Used in workflows</span>
                {usedIn.length === 0 && <p className="text-[13px] text-ct-text-3">Not used in any workflow yet.</p>}
                {usedIn.map((w) => (
                  <button
                    key={w.id}
                    type="button"
                    className="flex min-h-11 items-center gap-2 rounded-lg border border-ct-border px-3 text-left hover:bg-ct-subtle"
                    onClick={() => go('workflows')}
                  >
                    <span className="flex-1 font-medium">{w.name}</span>
                    <span className="text-xs text-ct-text-3">
                      {w.steps
                        .filter((s) => s.agent === shown.id)
                        .map((s) => s.id)
                        .join(', ')}
                    </span>
                  </button>
                ))}
              </div>
            </aside>
          )}
        </ResizablePanel>
      </ResizablePanelGroup>
    </div>
  )
}
