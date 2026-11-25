import { Empty } from '@/components/Section'
import { StateBadge, StateDot, stateLabel } from '@/features/runs/StateBadge'
import { useRunSelection } from '@/features/runs/useRunSelection'
import type { Run } from '@/lib/api'
import { when } from '@/lib/format'
import { stepState } from '@/lib/steps'
import { useNav } from '@/lib/nav'

/** One dot per step in its latest state, joined by short lines, green once passed: the mockup's pipeline. */
export function Pipeline({ run }: { run: Run }) {
  return (
    <span className="flex items-center">
      {run.steps.map((step, i) => {
        const state = stepState(run, step.id)
        const passed = state === 'done'
        return (
          <span key={step.id} className="flex items-center" title={`${step.id}: ${stateLabel(state)}`}>
            <StateDot state={state} size={16} />
            {i < run.steps.length - 1 && (
              <span aria-hidden className="h-0.5 w-2.5" style={{ background: passed ? 'var(--ct-done)' : 'var(--ct-control)' }} />
            )}
          </span>
        )
      })}
    </span>
  )
}

/** The latest runs; a click opens one in Runs. */
export function RecentRuns({ runs }: { runs: Run[] }) {
  const go = useNav((s) => s.go)
  const select = useRunSelection((s) => s.select)
  if (runs.length === 0) return <Empty>No runs yet. Start one from a ticket in Tracket, or from Runs.</Empty>
  const open = (id: string) => {
    select(id)
    go('runs')
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full min-w-[720px] border-collapse text-[13px]">
        <thead>
          <tr className="text-left text-ct-text-3">
            {['Tracket ticket', 'Workflow', 'Pipeline', 'Status', 'Started'].map((h) => (
              <th key={h} scope="col" className="border-b border-ct-border p-2 font-semibold">
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {runs.slice(0, 10).map((run) => (
            <tr key={run.id} className="cursor-pointer hover:bg-ct-subtle" onClick={() => open(run.id)}>
              <td className="max-w-64 border-b border-ct-border-soft px-2 py-2.5">
                <span className="font-semibold text-ct-text underline underline-offset-2 ct-mono">{run.ticket.key}</span>
                <div className="truncate text-ct-text-2">{run.ticket.title}</div>
              </td>
              <td className="border-b border-ct-border-soft px-2 py-2.5 whitespace-nowrap">
                {run.workflowName} <span className="text-ct-text-3 ct-mono">#{run.number}</span>
              </td>
              <td className="border-b border-ct-border-soft px-2 py-2.5">
                <Pipeline run={run} />
              </td>
              <td className="border-b border-ct-border-soft px-2 py-2.5">
                <StateBadge state={run.state} />
              </td>
              <td className="border-b border-ct-border-soft px-2 py-2.5 whitespace-nowrap text-ct-text-2">{when(run.startedAt)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
