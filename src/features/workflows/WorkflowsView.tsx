import { useState } from 'react'
import { FlowGraph, KindIcon, KINDS, workflowFlow, type FlowKind } from '@/components/FlowGraph'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { api, type AppError } from '@/lib/api'
import { useWorkflows } from './useWorkflows'
import { WorkflowInspector } from './WorkflowInspector'

const KIND_HELP: Record<FlowKind, string> = {
  trigger: 'What starts the workflow',
  agent: 'Hand work to an agent type',
  approval: 'Pause until someone approves',
  end: 'Finish and set the run status',
}

export function WorkflowsView() {
  const { data, error } = useWorkflows()
  const workflows = data?.workflows ?? []
  const problems = data?.problems ?? []
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [nodeByWorkflow, setNodeByWorkflow] = useState<Record<string, string>>({})
  const workflow = workflows.find((w) => w.id === selectedId) ?? workflows[0]
  const node = workflow ? (nodeByWorkflow[workflow.id] ?? workflow.steps[0]?.id ?? '$trigger') : '$trigger'
  const flow = workflow ? workflowFlow(workflow.steps, 'Tracket') : null

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-x-5 gap-y-3 border-b border-ct-border bg-ct-surface px-5 py-2.5">
        <div className="flex min-w-0 flex-[1_1_320px] items-center gap-2">
          <span className="flex min-h-9 items-center text-[13px] text-ct-text-2">Workflows</span>
          {workflow && (
            <>
              <span aria-hidden className="text-ct-faint">
                /
              </span>
              <h1 className="truncate text-[17px] font-bold">{workflow.name}</h1>
              <span className="rounded-xl px-2.5 py-0.5 text-xs font-semibold whitespace-nowrap" style={{ background: 'var(--ct-pill-none-bg)', color: 'var(--ct-pill-none)' }}>
                {workflow.offerInTracket ? 'Offered in Tracket' : 'Not in Tracket'}
              </span>
            </>
          )}
        </div>
        <div className="flex flex-wrap gap-2.5">
          <button type="button" className="ct-btn" onClick={() => void api.openDataDir()}>
            Open folder
          </button>
          {workflow && (
            <button type="button" className="ct-btn ct-btn-primary" onClick={() => void api.openWorkflowFile(workflow.file)}>
              Open file
            </button>
          )}
        </div>
      </div>
      {error && <p className="p-3 text-ct-failed">{(error as unknown as AppError).message}</p>}

      <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
        <ResizablePanel id="wf-list" defaultSize="22%" minSize="16%">
          <aside className="flex h-full flex-col gap-[18px] overflow-y-auto bg-ct-surface px-3.5 py-4">
            <div className="flex flex-col gap-2">
              <span className="ct-eyebrow">Workflows</span>
              {workflows.map((w) => (
                <button key={w.id} type="button" className="ct-row min-h-[52px] border border-ct-border" aria-current={w.id === workflow?.id ? 'true' : undefined} onClick={() => setSelectedId(w.id)}>
                  <KindIcon kind="trigger" />
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate font-semibold">{w.name}</span>
                    <span className="truncate text-xs text-ct-text-3">
                      {w.steps.length} steps · {w.agents.length} agent types
                    </span>
                  </span>
                </button>
              ))}
              {data && workflows.length === 0 && <p className="text-ct-text-3">Add a .yaml file to the workflows folder.</p>}
              {problems.map((p) => (
                <div key={p.file} className="rounded-lg border p-2.5 text-xs" style={{ borderColor: 'var(--ct-danger-border)' }}>
                  <p className="font-semibold break-all ct-mono">{p.file.split(/[\\/]/).at(-1)}</p>
                  <p style={{ color: 'var(--ct-danger)' }}>{p.message}</p>
                </div>
              ))}
            </div>

            <div className="flex flex-col gap-2 border-t border-ct-border pt-3.5">
              <span className="ct-eyebrow">Node kinds</span>
              {(Object.keys(KINDS) as FlowKind[]).map((k) => (
                <div key={k} className="flex items-center gap-2.5 py-1">
                  <KindIcon kind={k} size={30} />
                  <span className="flex flex-col">
                    <span className="text-[13px] font-semibold">{KINDS[k].label}</span>
                    <span className="text-xs text-ct-text-3">{KIND_HELP[k]}</span>
                  </span>
                </div>
              ))}
              <p className="text-xs text-ct-text-3">Condition, For each and Action come with the graph editor.</p>
            </div>

            {workflow && (
              <div className="flex flex-col gap-2.5 border-t border-ct-border pt-3.5">
                <span className="ct-eyebrow">Workflow settings</span>
                <div className="flex flex-col gap-1.5">
                  <span className="ct-label">Max parallel runs</span>
                  <input className="ct-input" readOnly value={workflow.maxParallelRuns} aria-label="Max parallel runs" />
                </div>
                <p className="text-[11px] break-all text-ct-text-3 ct-mono">{workflow.file}</p>
              </div>
            )}
          </aside>
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="wf-graph" defaultSize="50%" minSize="28%">
          <main className="ct-canvas relative h-full overflow-auto">
            {workflow && flow && (
              <div className="py-6">
                <FlowGraph mode="design" nodes={flow.nodes} edges={flow.edges} selected={node} onSelect={(id) => setNodeByWorkflow((m) => ({ ...m, [workflow.id]: id }))} />
              </div>
            )}
            {flow && (
              <div className="ct-card sticky bottom-3.5 left-3.5 ml-3.5 inline-flex items-center gap-2 px-3 py-2 text-[13px]">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--ct-pill-pass)" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                  <circle cx="12" cy="12" r="9" />
                  <path d="M8 12.5l2.5 2.5L16 9.5" />
                </svg>
                <span>
                  <strong className="font-semibold">Valid</strong> · {flow.nodes.length} nodes · {flow.edges.length} links
                </span>
              </div>
            )}
          </main>
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="wf-inspector" defaultSize="28%" minSize="20%">
          <aside aria-label="Node settings" className="h-full overflow-y-auto bg-ct-surface px-4 py-[18px]">
            {workflow && <WorkflowInspector workflow={workflow} node={node} />}
            <p className="mt-4 text-xs text-ct-text-3">Read-only for now: "Open file" edits the YAML.</p>
          </aside>
        </ResizablePanel>
      </ResizablePanelGroup>
    </div>
  )
}
