import { Empty, Section } from '@/components/Section'
import { ApprovalCard } from '@/features/approvals/ApprovalCard'
import { useRuns } from '@/features/runs/useRuns'
import { useRunSelection } from '@/features/runs/useRunSelection'
import { useWorkflows } from '@/features/workflows/useWorkflows'
import { describe, isAttention } from '@/lib/activity'
import type { Run } from '@/lib/api'
import { when } from '@/lib/format'
import { useNav } from '@/lib/nav'
import { useActivity } from '@/lib/queries'
import { agentOf } from '@/lib/steps'
import { RecentRuns } from './RecentRuns'

/** Each agent type, with what it is doing now: the job of a run it is running or waiting in. */
function useAgentsAtWork(runs: Run[]) {
  const workflows = useWorkflows().data?.workflows ?? []
  const ids = [...new Set(workflows.flatMap((w) => w.agents.map((a) => a.id)))].sort()
  return ids.map((id) => {
    const busy = runs.flatMap((run) =>
      run.jobs
        .filter((j) => (j.state === 'running' || j.state === 'waiting') && agentOf(run.steps, j.stepId) === id)
        .map((job) => ({ run, job })),
    )
    return { id, busy }
  })
}

function Stat({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="ct-card flex flex-col gap-1 px-4 py-3.5">
      <span className="text-[13px] text-ct-text-3">{label}</span>
      <span className="text-[26px] font-bold">{value}</span>
    </div>
  )
}

export function OverviewView() {
  const { data, dataUpdatedAt } = useRuns()
  const runs = data ?? []
  const activity = useActivity(12).data ?? []
  const agents = useAgentsAtWork(runs)
  const go = useNav((s) => s.go)
  const select = useRunSelection((s) => s.select)

  const waiting = runs.flatMap((run) => run.approvals.map((approval) => ({ run, approval })))
  const active = runs.filter((r) => r.state === 'running' || r.state === 'waiting').length
  // "Today" as of when the runs were fetched, so rendering stays pure
  const today = new Date(dataUpdatedAt).toDateString()
  const failedToday = runs.filter((r) => r.state === 'failed' && new Date(r.startedAt).toDateString() === today).length
  const busy = agents.filter((a) => a.busy.length > 0).length
  const openRun = (id: string | null) => {
    if (!id) return
    select(id)
    go('runs')
  }

  return (
    <div className="mx-auto flex w-full max-w-[1360px] flex-col gap-5 p-6">
      <h1 className="text-[22px] font-bold">Overview</h1>

      <div className="grid grid-cols-[repeat(auto-fit,minmax(200px,1fr))] gap-3">
        <Stat label="Active runs" value={active} />
        <Stat label="Waiting on you" value={waiting.length} />
        <Stat label="Agents busy" value={`${busy} of ${agents.length}`} />
        <Stat label="Failed today" value={failedToday} />
      </div>

      <div className="flex flex-wrap items-start gap-5">
        <div className="flex min-w-0 flex-[999_1_560px] flex-col gap-5">
          <Section title="Waiting on you" aside={<span className="text-xs text-ct-text-3">{waiting.length} pending</span>}>
            {waiting.length === 0 ? (
              <Empty>Nothing to approve right now.</Empty>
            ) : (
              waiting.map(({ run, approval }) => <ApprovalCard key={approval.id} approval={approval} run={run} showRun />)
            )}
          </Section>

          <Section
            title="Runs"
            aside={
              <button type="button" className="text-[13px] font-medium text-ct-text underline underline-offset-2 hover:underline" onClick={() => go('runs')}>
                All runs
              </button>
            }
          >
            <RecentRuns runs={runs} />
          </Section>
        </div>

        <div className="flex min-w-0 flex-[1_1_320px] flex-col gap-5">
          <Section title="Agents">
            {agents.length === 0 && <Empty>No agent types: they are defined in workflow files.</Empty>}
            {agents.map(({ id, busy }) => {
              const first = busy[0]
              return (
                <div key={id} className="flex min-h-10 items-center gap-2.5">
                  <span
                    aria-hidden
                    className="size-2 shrink-0 rounded-full"
                    style={{ background: first ? (first.job.state === 'waiting' ? 'var(--ct-waiting)' : 'var(--ct-accent)') : 'var(--ct-idle)' }}
                  />
                  <span className="flex-1 font-semibold">{id}</span>
                  <span className="text-xs text-ct-text-2">{first ? (first.job.state === 'waiting' ? 'Waiting for you' : 'Running') : 'Idle'}</span>
                  <button
                    type="button"
                    className="min-w-16 text-right text-xs text-ct-text-3 ct-mono hover:underline disabled:cursor-default disabled:no-underline"
                    disabled={!first}
                    onClick={() => openRun(first?.run.id ?? null)}
                  >
                    {first?.run.ticket.key ?? ''}
                  </button>
                </div>
              )
            })}
          </Section>

          <Section title="Recent events">
            {activity.length === 0 && <Empty>Nothing has happened yet.</Empty>}
            <ol className="flex flex-col gap-2.5">
              {activity.map((a, i) => (
                <li key={`${a.at}-${i}`}>
                  <button type="button" className="flex w-full gap-2.5 text-left text-[13px] leading-[1.45]" onClick={() => openRun(a.runId)}>
                    <span className="w-10 shrink-0 text-xs text-ct-text-3 ct-mono">{when(a.at)}</span>
                    <span className="min-w-0">
                      <span className="font-semibold ct-mono">{a.ticketKey}</span>{' '}
                      <span style={isAttention(a.what) ? { color: 'var(--ct-waiting)' } : undefined}>{describe(a.what, a.detail)}</span>
                    </span>
                  </button>
                </li>
              ))}
            </ol>
          </Section>
        </div>
      </div>
    </div>
  )
}
