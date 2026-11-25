import { Plus } from 'lucide-react'
import { useState } from 'react'
import { FlowGraph, workflowFlow, type FlowNode } from '@/components/FlowGraph'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { type AppError, type Run, type RunState } from '@/lib/api'
import { duration, when } from '@/lib/format'
import { JobPanel, RunEndsPanel } from './JobPanel'
import { RunFacts, RunHeader } from './RunHeader'
import { StartRunForm } from './StartRunForm'
import { StateDot, stateLabel } from './StateBadge'
import { useRuns } from './useRuns'
import { useRunSelection } from './useRunSelection'

const FILTERS: { id: string; label: string; states: RunState[] | null }[] = [
  { id: 'all', label: 'All', states: null },
  { id: 'running', label: 'Running', states: ['running', 'waiting', 'interrupted'] },
  { id: 'failed', label: 'Failed', states: ['failed'] },
  { id: 'passed', label: 'Passed', states: ['passed'] },
]

/** The run's workflow as a graph, each node in the state of its step's latest job. */
function runFlow(run: Run) {
  const flow = workflowFlow(run.steps, 'Tracket')
  const stateOf = (node: FlowNode): FlowNode['state'] => {
    if (node.id === '$trigger') return 'done'
    if (node.id === '$end') return run.state === 'passed' ? 'done' : run.state === 'failed' || run.state === 'cancelled' ? run.state : 'pending'
    return run.jobs.filter((j) => j.stepId === node.id).at(-1)?.state ?? 'pending'
  }
  const attempts = (id: string) => run.jobs.filter((j) => j.stepId === id).length
  const nodes = flow.nodes.map((n) => ({
    ...n,
    sub: n.id === '$trigger' ? `Started by ${run.startedBy}` : n.id === '$end' ? 'Run passes' : n.sub,
    state: stateOf(n),
    badge: attempts(n.id) > 1 ? `×${attempts(n.id)}` : undefined,
  }))
  return { nodes, edges: flow.edges }
}

/** The step to show first: the one that needs you, else the one running, else the last that ran. */
function defaultNode(run: Run) {
  const waiting = run.approvals[0]?.jobId
  const job = run.jobs.find((j) => j.id === waiting) ?? run.jobs.find((j) => j.state === 'running') ?? run.jobs.at(-1)
  return job?.stepId ?? '$trigger'
}

function RunList({ runs, selected, onPick, onNew }: { runs: Run[]; selected?: string; onPick: (id: string) => void; onNew: () => void }) {
  const [filter, setFilter] = useState('all')
  const states = FILTERS.find((f) => f.id === filter)?.states
  const shown = runs.filter((r) => !states || states.includes(r.state))
  return (
    <nav aria-label="Runs" className="flex h-full flex-col gap-3 overflow-y-auto bg-ct-surface px-2.5 py-3.5">
      <div className="flex items-center justify-between px-1.5">
        <span className="ct-eyebrow">Runs</span>
        <span className="flex items-center gap-2 text-xs text-ct-text-3">
          {shown.length}
          <button type="button" className="ct-btn ct-btn-ghost h-7 px-1.5" aria-label="Start a run" title="Start a run" onClick={onNew}>
            <Plus className="size-4" />
          </button>
        </span>
      </div>
      <div role="group" aria-label="Filter runs" className="flex flex-wrap gap-1.5 px-1">
        {FILTERS.map((f) => (
          <button key={f.id} type="button" className="ct-chip" aria-pressed={f.id === filter} onClick={() => setFilter(f.id)}>
            {f.label}
          </button>
        ))}
      </div>
      <div className="flex flex-col gap-0.5">
        {shown.map((r) => (
          <button key={r.id} type="button" className="ct-row min-h-14" aria-current={r.id === selected ? 'true' : undefined} onClick={() => onPick(r.id)}>
            <StateDot state={r.state} size={20} />
            <span className="flex min-w-0 flex-1 flex-col gap-0.5">
              <span className="flex items-baseline gap-1.5">
                <span className="truncate font-semibold">{r.workflowName}</span>
                <span className="text-xs text-ct-text-3 ct-mono">#{r.number}</span>
              </span>
              <span className="truncate text-xs text-ct-text-3">
                {stateLabel(r.state)} · {r.ticket.key} · {when(r.startedAt)}
              </span>
            </span>
            <span className="text-xs whitespace-nowrap text-ct-text-2">{duration(r.startedAt, r.endedAt)}</span>
          </button>
        ))}
        {shown.length === 0 && <p className="px-2.5 py-2 text-ct-text-3">No runs.</p>}
      </div>
    </nav>
  )
}

export function RunsView() {
  const { data, error } = useRuns()
  const runs = data ?? []
  const { selected, select } = useRunSelection()
  const [starting, setStarting] = useState(false)
  const [nodeByRun, setNodeByRun] = useState<Record<string, string>>({})
  const run = runs.find((r) => r.id === selected) ?? runs[0]
  const node = run ? (nodeByRun[run.id] ?? defaultNode(run)) : undefined

  return (
    <div className="flex h-full min-h-0 flex-col">
      {run && !starting && <RunHeader run={run} />}
      {error && <p className="p-3 text-ct-failed">{(error as unknown as AppError).message}</p>}
      <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
        <ResizablePanel id="runs-list" defaultSize="22%" minSize="16%">
          <RunList
            runs={runs}
            selected={run?.id}
            onPick={(id) => {
              setStarting(false)
              select(id)
            }}
            onNew={() => setStarting(true)}
          />
        </ResizablePanel>
        <ResizableHandle />
        {starting || !run ? (
          <ResizablePanel id="runs-start" defaultSize="78%">
            <div className="h-full overflow-y-auto px-6 py-5">
              <div className="ct-card max-w-xl p-5">
                <StartRunForm
                  onStarted={(r) => {
                    setStarting(false)
                    select(r.id)
                  }}
                  onCancel={() => setStarting(false)}
                />
              </div>
            </div>
          </ResizablePanel>
        ) : (
          <>
            <ResizablePanel id="runs-graph" defaultSize="50%" minSize="28%">
              <div className="flex h-full flex-col gap-4 px-6 py-5">
                <RunFacts run={run} />
                <div className="ct-canvas min-h-0 flex-1 overflow-auto rounded-[10px] border border-ct-border">
                  <FlowGraph mode="run" {...runFlow(run)} selected={node} onSelect={(id) => setNodeByRun((m) => ({ ...m, [run.id]: id }))} />
                </div>
              </div>
            </ResizablePanel>
            <ResizableHandle />
            <ResizablePanel id="runs-job" defaultSize="28%" minSize="20%">
              <aside aria-label="Step details" className="h-full overflow-y-auto bg-ct-surface px-4 py-[18px]">
                {node === '$trigger' || node === '$end' ? (
                  <RunEndsPanel run={run} which={node === '$trigger' ? 'trigger' : 'end'} />
                ) : (
                  <JobPanel key={`${run.id}-${node}`} run={run} stepId={node!} />
                )}
              </aside>
            </ResizablePanel>
          </>
        )}
      </ResizablePanelGroup>
    </div>
  )
}
