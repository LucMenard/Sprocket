import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { FlowGraph, KindIcon, KINDS, workflowFlow } from '@/components/FlowGraph'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { useAgents } from '@/features/agents/useAgents'
import { api, type AgentType, type AppError, type Workflow } from '@/lib/api'
import { useWorkflowDraft } from './useWorkflowDraft'
import { useWorkflows } from './useWorkflows'
import { WorkflowInspector } from './WorkflowInspector'

const PALETTE = [
  { kind: 'agent' as const, help: 'Hand work to an agent type' },
  { kind: 'approval' as const, help: 'Pause until someone approves' },
]

export function WorkflowsView() {
  const queryClient = useQueryClient()
  const { data, error } = useWorkflows()
  const shared = useAgents().data?.agents ?? []
  const workflows = data?.workflows ?? []
  const problems = data?.problems ?? []
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [nodeByWorkflow, setNodeByWorkflow] = useState<Record<string, string>>({})
  const [newName, setNewName] = useState<string | null>(null)
  const saved = workflows.find((w) => w.id === selectedId) ?? workflows[0]
  const editing = useWorkflowDraft(saved)
  const draft = editing.draft
  const node = saved ? (nodeByWorkflow[saved.id] ?? draft?.steps[0]?.id ?? '$trigger') : '$trigger'
  const select = (id: string) => saved && setNodeByWorkflow((m) => ({ ...m, [saved.id]: id }))
  const flow = draft ? workflowFlow(draft.steps, 'Tracket') : null
  // What a step can use: this workflow's own agent types first, then the shared ones
  const agents: AgentType[] = [...(saved?.agents.filter((a) => !a.shared) ?? []), ...shared.filter((a) => !saved?.agents.some((o) => !o.shared && o.id === a.id))]

  const reload = () => {
    void queryClient.invalidateQueries({ queryKey: ['workflows'] })
    void queryClient.invalidateQueries({ queryKey: ['agents'] })
  }
  const save = useMutation<Workflow, AppError>({ mutationFn: () => api.saveWorkflow(saved!.id, draft!), onSuccess: reload })
  const create = useMutation<Workflow, AppError, string>({
    mutationFn: (name) => api.createWorkflow(name),
    onSuccess: (w) => {
      setNewName(null)
      setSelectedId(w.id)
      reload()
    },
  })
  const remove = useMutation<void, AppError>({
    mutationFn: () => api.deleteWorkflow(saved!.id),
    onSuccess: () => {
      setSelectedId(null)
      reload()
    },
  })
  const problem = (save.error ?? create.error ?? remove.error)?.message

  /** Leaving a workflow with unsaved edits asks first */
  const pick = (id: string) => {
    if (id === saved?.id) return
    if (editing.dirty && !window.confirm('Discard your changes to this workflow?')) return
    save.reset()
    setSelectedId(id)
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-x-5 gap-y-3 border-b border-ct-border bg-ct-surface px-5 py-2.5">
        <div className="flex min-w-0 flex-[1_1_320px] items-center gap-2">
          <span className="flex min-h-9 items-center text-[13px] text-ct-text-2">Workflows</span>
          {draft && (
            <>
              <span aria-hidden className="text-ct-faint">
                /
              </span>
              <h1 className="truncate text-[17px] font-bold">{draft.name || 'Untitled'}</h1>
              <span className="rounded-xl px-2.5 py-0.5 text-xs font-semibold whitespace-nowrap" style={{ background: 'var(--ct-pill-none-bg)', color: 'var(--ct-pill-none)' }}>
                {editing.dirty ? 'Unsaved changes' : draft.offerInTracket ? 'Offered in Tracket' : 'Not in Tracket'}
              </span>
            </>
          )}
          {problem && <span className="text-[13px] text-ct-failed">{problem}</span>}
        </div>
        {saved && (
          <div className="flex flex-wrap gap-2.5">
            <button
              type="button"
              className="ct-btn ct-btn-danger"
              disabled={remove.isPending}
              onClick={() => window.confirm(`Delete the workflow "${saved.name}"? Its past runs are kept.`) && remove.mutate()}
            >
              Delete
            </button>
            <button type="button" className="ct-btn" onClick={() => void api.openWorkflowFile(saved.file)}>
              Open file
            </button>
            {editing.dirty && (
              <button type="button" className="ct-btn" onClick={() => (save.reset(), editing.discard())}>
                Discard
              </button>
            )}
            <button type="button" className="ct-btn ct-btn-primary" disabled={!editing.dirty || save.isPending} onClick={() => save.mutate()}>
              Save
            </button>
          </div>
        )}
      </div>
      {error && <p className="p-3 text-ct-failed">{(error as unknown as AppError).message}</p>}

      <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
        <ResizablePanel id="wf-list" defaultSize="22%" minSize="16%">
          <aside className="flex h-full flex-col gap-[18px] overflow-y-auto bg-ct-surface px-3.5 py-4">
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <span className="ct-eyebrow">Workflows</span>
                <button type="button" className="ct-btn ct-btn-ghost h-7 px-2 text-xs" onClick={() => setNewName('')}>
                  + New
                </button>
              </div>
              {newName !== null && (
                <form
                  className="flex gap-1.5"
                  onSubmit={(e) => {
                    e.preventDefault()
                    if (newName.trim()) create.mutate(newName)
                  }}
                >
                  <input className="ct-input h-9" autoFocus placeholder="Name of the workflow" value={newName} onChange={(e) => setNewName(e.target.value)} />
                  <button type="submit" className="ct-btn ct-btn-sm ct-btn-primary" disabled={create.isPending}>
                    Create
                  </button>
                </form>
              )}
              {workflows.map((w) => (
                <button key={w.id} type="button" className="ct-row min-h-[52px] border border-ct-border" aria-current={w.id === saved?.id ? 'true' : undefined} onClick={() => pick(w.id)}>
                  <KindIcon kind="trigger" />
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate font-semibold">{w.name}</span>
                    <span className="truncate text-xs text-ct-text-3">
                      {w.steps.length} steps · {w.agents.length} agent types
                    </span>
                  </span>
                </button>
              ))}
              {problems.map((p) => (
                <div key={p.file} className="rounded-lg border p-2.5 text-xs" style={{ borderColor: 'var(--ct-danger-border)' }}>
                  <p className="font-semibold break-all ct-mono">{p.file.split(/[\\/]/).at(-1)}</p>
                  <p style={{ color: 'var(--ct-danger)' }}>{p.message}</p>
                </div>
              ))}
            </div>

            {draft && (
              <div className="flex flex-col gap-2 border-t border-ct-border pt-3.5">
                <span className="ct-eyebrow">Add a node</span>
                {PALETTE.map((p) => (
                  <button
                    key={p.kind}
                    type="button"
                    className="flex min-h-[52px] items-center gap-2.5 rounded-[10px] border border-ct-border bg-ct-surface px-2.5 py-2 text-left hover:bg-ct-subtle"
                    onClick={() => {
                      // After the selected node: first after the trigger, last before the end
                      const at = node === '$trigger' ? 0 : node === '$end' ? draft.steps.length : draft.steps.findIndex((s) => s.id === node) + 1
                      const id = editing.addStep(p.kind, at, agents[0]?.id ?? null)
                      if (id) select(id)
                    }}
                  >
                    <KindIcon kind={p.kind} />
                    <span className="flex flex-col gap-0.5">
                      <span className="font-semibold">{KINDS[p.kind].label}</span>
                      <span className="text-xs text-ct-text-3">{p.help}</span>
                    </span>
                  </button>
                ))}
                <p className="text-xs text-ct-text-3">Added after the selected node. Condition, For each and Action come later.</p>
              </div>
            )}

            {draft && (
              <div className="flex flex-col gap-2.5 border-t border-ct-border pt-3.5">
                <span className="ct-eyebrow">Workflow settings</span>
                <label className="flex flex-col gap-1.5">
                  <span className="ct-label">Name</span>
                  <input className="ct-input" value={draft.name} onChange={(e) => editing.setField('name', e.target.value)} />
                </label>
                <label className="flex flex-col gap-1.5">
                  <span className="ct-label">Description</span>
                  <textarea className="ct-input h-auto py-2 leading-normal" rows={2} value={draft.description} onChange={(e) => editing.setField('description', e.target.value)} />
                </label>
                <label className="flex flex-col gap-1.5">
                  <span className="ct-label">Max parallel runs</span>
                  <input
                    type="number"
                    min={1}
                    className="ct-input"
                    value={draft.maxParallelRuns}
                    onChange={(e) => editing.setField('maxParallelRuns', Math.max(1, Number(e.target.value) || 1))}
                  />
                </label>
              </div>
            )}
          </aside>
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="wf-graph" defaultSize="50%" minSize="28%">
          <main className="ct-canvas relative h-full overflow-auto">
            {flow && (
              <>
                <div className="py-6">
                  <FlowGraph mode="design" nodes={flow.nodes} edges={flow.edges} selected={node} onSelect={select} />
                </div>
                <div className="ct-card sticky bottom-3.5 left-3.5 ml-3.5 inline-flex items-center gap-2 px-3 py-2 text-[13px]">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--ct-pill-pass)" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                    <circle cx="12" cy="12" r="9" />
                    <path d="M8 12.5l2.5 2.5L16 9.5" />
                  </svg>
                  <span>
                    <strong className="font-semibold">{editing.dirty ? 'Checked on save' : 'Valid'}</strong> · {flow.nodes.length} nodes · {flow.edges.length} links
                  </span>
                </div>
              </>
            )}
          </main>
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="wf-inspector" defaultSize="28%" minSize="20%">
          <aside aria-label="Node settings" className="h-full overflow-y-auto bg-ct-surface px-4 py-[18px]">
            {draft && <WorkflowInspector draft={draft} editing={editing} node={node} agents={agents} onSelect={select} />}
          </aside>
        </ResizablePanel>
      </ResizablePanelGroup>
    </div>
  )
}
