import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Redo2, Undo2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { FlowGraph, KindIcon } from '@/components/FlowGraph'
import { designSub, edgeKey, KINDS, workflowFlow } from '@/lib/flow'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { useAgents } from '@/features/agents/useAgents'
import { api, type AgentType, type AppError, type StepKind, type Workflow, type WorkflowDraft } from '@/lib/api'
import { allSteps, findPlace } from '@/lib/steps'
import { draftProblems, useWorkflowDraft } from './useWorkflowDraft'
import { useWorkflows } from './useWorkflows'
import { WorkflowInspector } from './WorkflowInspector'

const PALETTE: StepKind[] = ['agent', 'team', 'condition', 'switch', 'forEach', 'approval', 'action', 'launch']

/**
 * Save, off while there is nothing to save or the graph isn't complete. Hovering it (or focusing it) says what
 * is missing; a disabled button can't show a tooltip, so it stays focusable and refuses the click itself.
 */
/** The trigger node's subtitle: what starts the workflow, in a few words. */
function triggerSub(d: WorkflowDraft): string {
  const parts = [d.offerInTracket || d.forRequests ? 'Tracket' : 'By hand']
  const schedules = d.triggers.filter((t) => t.schedule != null).length
  const after = d.triggers.filter((t) => t.after != null).length
  if (schedules) parts.push(schedules === 1 ? 'a schedule' : `${schedules} schedules`)
  if (after) parts.push(after === 1 ? 'after a workflow' : `after ${after} workflows`)
  if (d.triggers.some((t) => t.webhook)) parts.push('webhook')
  return parts.join(' · ')
}

function SaveButton({ dirty, busy, blockers, onSave }: { dirty: boolean; busy: boolean; blockers: string[]; onSave: () => void }) {
  const [open, setOpen] = useState(false)
  const blocked = blockers.length > 0
  const off = !dirty || busy || blocked
  return (
    <span className="relative" onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}>
      <button
        type="button"
        className="ct-btn ct-btn-primary aria-disabled:cursor-not-allowed aria-disabled:opacity-50"
        aria-disabled={off}
        aria-describedby={blocked ? 'save-blockers' : undefined}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onClick={() => !off && onSave()}
      >
        Save
      </button>
      {blocked && (
        <div
          id="save-blockers"
          role="tooltip"
          className={`ct-card absolute top-full right-0 z-40 mt-2 w-80 flex-col gap-1.5 p-3 text-[13px] shadow-lg ${open ? 'flex' : 'hidden'}`}
        >
          <strong className="font-semibold">Can't save yet</strong>
          <ul className="flex list-disc flex-col gap-1 pl-4 text-ct-text-2">
            {blockers.map((b) => (
              <li key={b}>{b}</li>
            ))}
          </ul>
        </div>
      )}
    </span>
  )
}

export function WorkflowsView() {
  const queryClient = useQueryClient()
  const { data, error } = useWorkflows()
  const shared = useAgents().data?.agents ?? []
  const workflows = data?.workflows ?? []
  const problems = data?.problems ?? []
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [nodeByWorkflow, setNodeByWorkflow] = useState<Record<string, string>>({})
  const [newName, setNewName] = useState<string | null>(null)
  // What waits for a yes: deleting this workflow, or leaving its unsaved edits for another one
  const [confirming, setConfirming] = useState<{ type: 'delete' } | { type: 'leave'; to: string } | null>(null)
  const [linkError, setLinkError] = useState<string | null>(null)
  const saved = workflows.find((w) => w.id === selectedId) ?? workflows[0]
  const editing = useWorkflowDraft(saved)
  const draft = editing.draft
  const node = saved ? (nodeByWorkflow[saved.id] ?? draft?.steps[0]?.id ?? '$trigger') : '$trigger'
  // A selected link (its `edgeKey`), instead of a node
  const [link, setLink] = useState<string | null>(null)
  const select = (id: string) => {
    setLink(null)
    if (saved) setNodeByWorkflow((m) => ({ ...m, [saved.id]: id }))
  }
  const flow = draft
    ? workflowFlow(
        draft.steps,
        'design',
        (s) => ({ sub: designSub(s) }),
        { trigger: { sub: triggerSub(draft) }, end: { sub: 'Mark the run as passed' } },
        { positions: draft.layout, loose: draft.loose, cut: draft.cut, invalid: draft.extra },
      )
    : null
  const selectedEdge = link ? flow?.edges.find((e) => edgeKey(e) === link) : undefined

  /** Deletes the selected link, or the selected node: Delete in the graph, × in the inspector */
  const deleteSelected = () => {
    if (selectedEdge) {
      setLinkError(editing.deleteLink(selectedEdge))
      setLink(null)
      return
    }
    if (!draft || node.startsWith('$')) return
    const before = flow?.nodes[flow.nodes.findIndex((n) => n.id === node) - 1]?.id ?? '$trigger'
    editing.removeStep(node)
    select(before)
  }

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
      setConfirming(null)
      setSelectedId(null)
      reload()
    },
  })
  const problem = linkError ?? (save.error ?? create.error ?? remove.error)?.message
  // What keeps Save off while the graph is being changed: told when hovering it
  const blockers = draft && editing.dirty ? draftProblems(draft) : []

  // The editor's keys. Not while typing in a field, nor under a dialog
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null
      if (confirming || !draft || t?.closest('input, textarea, select, [contenteditable="true"], [role="dialog"], [role="alertdialog"]')) return
      const mod = e.ctrlKey || e.metaKey
      const key = e.key.toLowerCase()
      if (mod && key === 'z' && !e.shiftKey) return (e.preventDefault(), editing.undo())
      if (mod && (key === 'y' || (key === 'z' && e.shiftKey))) return (e.preventDefault(), editing.redo())
      if (mod && key === 's') {
        e.preventDefault()
        if (editing.dirty && !blockers.length && !save.isPending) save.mutate()
        return
      }
      if (e.key === 'Delete' || e.key === 'Backspace') return (e.preventDefault(), deleteSelected())
      if (e.key === 'Escape') return setLink(null)
      const step = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key]
      const at = flow?.nodes.find((n) => n.id === node)
      if (step && at && !link) {
        e.preventDefault()
        const by = e.shiftKey ? 50 : 10
        editing.place(node, { x: at.x + step[0] * by, y: at.y + step[1] * by })
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  const open = (id: string) => {
    save.reset()
    setLinkError(null)
    setSelectedId(id)
  }
  /** Leaving a workflow with unsaved edits asks first */
  const pick = (id: string) => {
    if (id === saved?.id) return
    if (editing.dirty) setConfirming({ type: 'leave', to: id })
    else open(id)
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
              onClick={() => (remove.reset(), setConfirming({ type: 'delete' }))}
            >
              Delete
            </button>
            <button type="button" className="ct-btn" onClick={() => void api.openWorkflowFile(saved.file)}>
              Open file
            </button>
            <div role="group" aria-label="History" className="flex">
              <button type="button" className="ct-btn ct-btn-ghost px-2.5" aria-label="Undo (Ctrl+Z)" title="Undo (Ctrl+Z)" disabled={!editing.canUndo} onClick={editing.undo}>
                <Undo2 className="size-4" />
              </button>
              <button type="button" className="ct-btn ct-btn-ghost px-2.5" aria-label="Redo (Ctrl+Y)" title="Redo (Ctrl+Y)" disabled={!editing.canRedo} onClick={editing.redo}>
                <Redo2 className="size-4" />
              </button>
            </div>
            {editing.dirty && (
              <button type="button" className="ct-btn" onClick={() => (save.reset(), editing.discard())}>
                Discard
              </button>
            )}
            <SaveButton dirty={editing.dirty} busy={save.isPending} blockers={blockers} onSave={() => save.mutate()} />
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
                      {allSteps(w.steps).length} steps · {w.agents.length} agent types
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
                {PALETTE.map((kind) => (
                  <button
                    key={kind}
                    type="button"
                    className="flex min-h-[52px] items-center gap-2.5 rounded-[10px] border border-ct-border bg-ct-surface px-2.5 py-2 text-left hover:bg-ct-subtle"
                    onClick={() => {
                      // After the selected node, next to it: first after the trigger, last before the end
                      const place = findPlace(draft.steps, node)
                      const slot = place
                        ? { parent: place.parent?.id ?? null, branch: place.branch, lane: place.lane, index: place.index + 1 }
                        : { parent: null, branch: null, index: node === '$trigger' ? 0 : draft.steps.length }
                      // An action is best done by an agent that has MCP servers
                      const agent = kind === 'action' ? (agents.find((a) => a.mcp.length) ?? agents[0]) : agents[0]
                      const id = editing.addStep(kind, slot, agent?.id ?? null, agents.map((a) => a.id))
                      if (id) select(id)
                    }}
                  >
                    <KindIcon kind={kind} />
                    <span className="flex flex-col gap-0.5">
                      <span className="font-semibold">{KINDS[kind].label}</span>
                      <span className="text-xs text-ct-text-3">{KINDS[kind].help}</span>
                    </span>
                  </button>
                ))}
                <p className="text-xs text-ct-text-3">Added after the selected node, in its branch or loop.</p>
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
                <label className="flex cursor-pointer items-start gap-2.5">
                  <input type="checkbox" className="mt-0.5 size-4" checked={draft.board} onChange={(e) => editing.setField('board', e.target.checked)} />
                  <span className="flex flex-col gap-0.5">
                    <span className="font-medium">Board</span>
                    <span className="text-xs text-ct-text-3">Notes its agents and you share during a run</span>
                  </span>
                </label>
                <div className="flex flex-col gap-1.5">
                  <span className="ct-label">Steps may ask</span>
                  <div role="group" aria-label="Agent types a step may ask" className="flex flex-wrap gap-1.5">
                    {agents.map((a) => {
                      const on = draft.colleagues.canAsk.includes(a.id)
                      return (
                        <button
                          key={a.id}
                          type="button"
                          className="ct-chip"
                          aria-pressed={on}
                          onClick={() =>
                            editing.setField('colleagues', {
                              ...draft.colleagues,
                              canAsk: on ? draft.colleagues.canAsk.filter((x) => x !== a.id) : [...draft.colleagues.canAsk, a.id],
                            })
                          }
                        >
                          {a.id}
                        </button>
                      )
                    })}
                  </div>
                  <label className="flex items-center gap-2 text-[13px]">
                    <input
                      type="number"
                      min={1}
                      max={20}
                      className="ct-input h-9 w-20"
                      value={draft.colleagues.maxAsks}
                      onChange={(e) => editing.setField('colleagues', { ...draft.colleagues, maxAsks: Math.min(20, Math.max(1, Number(e.target.value) || 1)) })}
                    />
                    asks per step at most
                  </label>
                  <span className="text-xs text-ct-text-3">
                    An agent asks another agent type a question with ask_agent; the answer comes back to it. None pressed: nobody can be asked.
                  </span>
                </div>
                <div className="flex flex-col gap-1.5">
                  <span className="ct-label">Team limits</span>
                  <div className="grid grid-cols-3 gap-2">
                    {(
                      [
                        ['maxTurns', 'turns', 1],
                        ['budgetUsd', '$ each', 0.1],
                        ['minutes', 'minutes', 1],
                      ] as const
                    ).map(([key, label, step]) => (
                      <label key={key} className="flex flex-col gap-1 text-xs text-ct-text-3">
                        <input
                          type="number"
                          min={step}
                          step={step}
                          className="ct-input h-9"
                          value={draft.teamLimits[key]}
                          onChange={(e) => editing.setField('teamLimits', { ...draft.teamLimits, [key]: Math.max(step, Number(e.target.value) || step) })}
                        />
                        {label}
                      </label>
                    ))}
                  </div>
                  <span className="text-xs text-ct-text-3">The most a team step may use, for each member. A team step can ask for less.</span>
                </div>
              </div>
            )}
          </aside>
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="wf-graph" defaultSize="50%" minSize="28%">
          <main className="ct-canvas relative h-full overflow-hidden">
            {flow && (
              <FlowGraph
                key={saved?.id}
                mode="design"
                flow={flow}
                selected={node}
                onSelect={select}
                onConnect={(from, to, port) => {
                  const why = editing.connect(from, to, port)
                  setLinkError(why)
                  if (!why) select(to)
                }}
                onPlace={(id, at) => {
                  editing.place(id, at)
                  select(id)
                }}
                onResetLayout={Object.keys(draft?.layout ?? {}).length ? editing.resetLayout : undefined}
                selectedLink={link}
                onSelectLink={setLink}
              >
                <div className="ct-card inline-flex items-center gap-2 px-3 py-2 text-[13px]">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--ct-pill-pass)" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                    <circle cx="12" cy="12" r="9" />
                    <path d="M8 12.5l2.5 2.5L16 9.5" />
                  </svg>
                  <span>
                    <strong className="font-semibold">{editing.dirty ? 'Checked on save' : 'Valid'}</strong> · {flow.nodes.length} nodes · {flow.edges.length} links
                  </span>
                </div>
              </FlowGraph>
            )}
          </main>
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="wf-inspector" defaultSize="28%" minSize="20%">
          <aside aria-label="Node settings" className="h-full overflow-y-auto bg-ct-surface px-4 py-[18px]">
            {draft && flow && (
              <WorkflowInspector
                draft={draft}
                editing={editing}
                node={node}
                agents={agents}
                launchable={workflows.filter((w) => w.id !== saved?.id).map((w) => ({ id: w.id, name: w.name, forRequests: w.forRequests }))}
                workflowId={saved?.id ?? null}
                dirty={editing.dirty}
                flow={flow}
                link={selectedEdge}
                onSelect={select}
                onSelectLink={setLink}
                onDeleteLink={deleteSelected}
                onLinkError={setLinkError}
              />
            )}
          </aside>
        </ResizablePanel>
      </ResizablePanelGroup>
      {confirming?.type === 'delete' && saved && (
        <ConfirmDialog
          title={`Delete the workflow "${saved.name}"?`}
          confirm="Delete workflow"
          danger
          busy={remove.isPending}
          onConfirm={() => remove.mutate()}
          onCancel={() => setConfirming(null)}
        >
          <p>Its file is deleted. Its past runs are kept, and runs still going finish with the workflow they started with.</p>
          {remove.error && <p className="mt-2 text-ct-failed">{remove.error.message}</p>}
        </ConfirmDialog>
      )}
      {confirming?.type === 'leave' && (
        <ConfirmDialog
          title="Discard your changes?"
          confirm="Discard changes"
          danger
          onConfirm={() => {
            const to = confirming.to
            setConfirming(null)
            editing.discard()
            open(to)
          }}
          onCancel={() => setConfirming(null)}
        >
          <p>This workflow has changes that aren't saved.</p>
        </ConfirmDialog>
      )}
    </div>
  )
}
