import { useMutation, useQueryClient } from '@tanstack/react-query'
import { openUrl } from '@tauri-apps/plugin-opener'
import { api, type AppError, type Run } from '@/lib/api'
import { stepState } from '@/lib/steps'
import { duration, usd, when } from '@/lib/format'
import { StateBadge } from './StateBadge'
import { useRunSelection } from './useRunSelection'

/** The bar under the header: which run, its state, and what can be done with it. */
export function RunHeader({ run }: { run: Run }) {
  const queryClient = useQueryClient()
  const select = useRunSelection((s) => s.select)
  const done = () => queryClient.invalidateQueries({ queryKey: ['runs'] })
  const cancel = useMutation({ mutationFn: () => api.cancelRun(run.id), onSettled: done })
  const again = useMutation({
    mutationFn: () => api.startRun({ workflow: run.workflowId, ticket: run.ticket, folder: run.folder, startedBy: 'Sprocket', request: run.request ?? undefined }),
    onSuccess: (next) => select(next.id),
    onSettled: done,
  })
  const active = run.state === 'running' || run.state === 'waiting' || run.state === 'interrupted'
  const error = (cancel.error ?? again.error) as unknown as AppError | null

  return (
    <div className="flex flex-wrap items-center gap-x-5 gap-y-3 border-b border-ct-border bg-ct-surface px-5 py-2.5">
      <div className="flex min-w-0 flex-[1_1_320px] items-center gap-2">
        <h1 className="text-[17px] font-bold whitespace-nowrap">{run.workflowName}</h1>
        <span className="text-[13px] text-ct-text-2 ct-mono">#{run.number}</span>
        <StateBadge state={run.state} />
        {error && <span className="text-xs text-ct-failed">{error.message}</span>}
      </div>
      <div className="flex flex-wrap gap-2">
        {active && (
          <button type="button" className="ct-btn ct-btn-danger" disabled={cancel.isPending} onClick={() => cancel.mutate()}>
            Cancel run
          </button>
        )}
        <button type="button" className="ct-btn" onClick={() => void openUrl(run.ticket.url)}>
          Open in Jira
        </button>
        <button type="button" className="ct-btn" disabled={again.isPending || active} onClick={() => again.mutate()}>
          Run again
        </button>
      </div>
    </div>
  )
}

/** The facts card over the graph: ticket, who started it, when, how long, how far. */
export function RunFacts({ run }: { run: Run }) {
  const done = run.steps.filter((s) => stepState(run, s.id) === 'done').length
  const facts: [string, string][] = [
    ['Started by', run.startedBy],
    ['Started', when(run.startedAt)],
    ['Duration', duration(run.startedAt, run.endedAt)],
    ['Steps', `${done} of ${run.steps.length} done`],
  ]
  if (run.costUsd) facts.push(['Cost', usd(run.costUsd)])
  if (run.request) facts.unshift(['Asked', run.request])

  return (
    <div className="ct-card flex flex-wrap items-center gap-x-6 gap-y-2.5 px-4 py-3 text-[13px]">
      <span className="flex min-w-0 items-center gap-1.5">
        <span className="text-ct-text-3">Jira</span>
        <button
          type="button"
          className="inline-flex min-h-8 items-center font-semibold text-ct-text underline underline-offset-2 ct-mono hover:underline"
          onClick={() => void openUrl(run.ticket.url)}
        >
          {run.ticket.key}
        </button>
        <span className="truncate">{run.ticket.title}</span>
      </span>
      {facts.map(([label, value]) => (
        <span key={label}>
          <span className="text-ct-text-3">{label}</span> {value}
        </span>
      ))}
    </div>
  )
}
