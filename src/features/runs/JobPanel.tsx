import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState, type ReactNode } from 'react'
import { Dialog } from '@/components/Dialog'
import { KindIcon } from '@/components/FlowGraph'
import { ApprovalCard } from '@/features/approvals/ApprovalCard'
import { describe } from '@/lib/activity'
import { api, type AppError, type Job, type Run } from '@/lib/api'
import { duration, usd, when } from '@/lib/format'
import { StateDot, stateLabel } from './StateBadge'
import { Transcript } from './Transcript'

function Facts({ rows }: { rows: [string, ReactNode, boolean?][] }) {
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-[13px]">
      {rows.map(([label, value, strong]) => (
        <div key={label} className="contents">
          <dt className="text-ct-text-3">{label}</dt>
          <dd className={strong ? 'min-w-0 font-semibold' : 'min-w-0'}>{value}</dd>
        </div>
      ))}
    </dl>
  )
}

function Heading({ mark, eyebrow, title }: { mark: ReactNode; eyebrow: string; title: string }) {
  return (
    <div className="flex items-center gap-2.5">
      {mark}
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className="ct-eyebrow">{eyebrow}</span>
        <span className="truncate text-base font-bold">{title}</span>
      </span>
    </div>
  )
}

/** Colour of a log line, as in the mockup's dark log. */
function logColor(what: string) {
  if (what === 'job.done' || what.endsWith('.approved') || what === 'run.passed') return 'var(--ct-log-ok)'
  if (what.endsWith('.failed') || what.endsWith('.rejected') || what.endsWith('.interrupted')) return 'var(--ct-log-err)'
  if (what === 'gate.opened' || what === 'tool.asked') return 'var(--ct-log-wait)'
  return 'var(--ct-log)'
}

/** The trigger and the end node: what started the run, and how it ended. */
export function RunEndsPanel({ run, which }: { run: Run; which: 'trigger' | 'end' }) {
  return which === 'trigger' ? (
    <div className="flex flex-col gap-[18px]">
      <Heading mark={<KindIcon kind="trigger" size={28} />} eyebrow="Trigger" title="Started from Tracket" />
      <Facts rows={[['Started by', run.startedBy], ['Started', when(run.startedAt)], ['Ticket', `${run.ticket.key} ${run.ticket.title}`], ['Worktree', <span key="v" className="text-xs break-all ct-mono">{run.folder}</span>]]} />
    </div>
  ) : (
    <div className="flex flex-col gap-[18px]">
      <Heading mark={<KindIcon kind="end" size={28} />} eyebrow="End" title="How the run ends" />
      <Facts rows={[['Status', stateLabel(run.state), true], ['Ended', run.endedAt ? when(run.endedAt) : 'Not yet'], ['Cost', usd(run.costUsd) || '–']]} />
    </div>
  )
}

/** The selected step: its latest job (or an earlier attempt), what it waits for, its log and session. */
export function JobPanel({ run, stepId }: { run: Run; stepId: string }) {
  const queryClient = useQueryClient()
  const step = run.steps.find((s) => s.id === stepId)
  const attempts = run.jobs.filter((j) => j.stepId === stepId)
  const [attemptId, setAttemptId] = useState<string | null>(null)
  const [sessionOpen, setSessionOpen] = useState(false)
  const job: Job | undefined = attempts.find((j) => j.id === attemptId) ?? attempts.at(-1)
  const retry = useMutation({
    mutationFn: () => api.retryJob(job!.id),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['runs'] }),
  })
  if (!step) return null

  const index = run.steps.indexOf(step) + 1
  const isAgent = step.kind === 'agent'
  const approvals = run.approvals.filter((a) => a.jobId === job?.id)
  const canRetry =
    !!job && job === attempts.at(-1) && (job.state === 'failed' || job.state === 'interrupted') && (run.state === 'failed' || run.state === 'interrupted')

  return (
    <div className="flex min-h-full flex-col gap-[18px]">
      <Heading
        mark={<StateDot state={job?.state ?? 'pending'} size={28} />}
        eyebrow={`${isAgent ? 'Agent step' : 'Human approval'} · step ${index}`}
        title={step.id}
      />

      {!job ? (
        <p className="text-ct-text-3">Not started yet.</p>
      ) : (
        <>
          <Facts
            rows={[
              ['Status', stateLabel(job.state), true],
              ['Run by', isAgent ? step.agent : 'You'],
              ['Duration', duration(job.startedAt, job.endedAt) || '–'],
              ['Attempt', `${job.attempt} of ${attempts.length}`],
              ...(job.costUsd ? ([['Cost', usd(job.costUsd)]] as [string, string][]) : []),
            ]}
          />
          {attempts.length > 1 && (
            <div className="flex flex-wrap gap-1.5" role="group" aria-label="Attempts">
              {attempts.map((a) => (
                <button key={a.id} type="button" className="ct-chip flex items-center gap-1.5" aria-pressed={a.id === job.id} onClick={() => setAttemptId(a.id)}>
                  <StateDot state={a.state} size={8} />
                  Attempt {a.attempt}
                </button>
              ))}
            </div>
          )}

          {approvals.map((a) => (
            <ApprovalCard key={a.id} approval={a} run={run} />
          ))}
          {job.error && (
            <p className="rounded-lg p-2.5 text-[13px]" style={{ background: 'var(--ct-pill-fail-bg)', color: 'var(--ct-pill-fail)' }}>
              {job.error}
            </p>
          )}
          {job.output && (
            <div className="flex flex-col gap-2">
              <span className="text-[13px] font-semibold">{isAgent ? 'Output' : 'Note'}</span>
              <pre className="max-h-60 overflow-auto rounded-lg border border-ct-border bg-ct-subtle p-3 text-xs whitespace-pre-wrap ct-mono">{job.output}</pre>
            </div>
          )}

          <div className="flex flex-col gap-2">
            <span className="text-[13px] font-semibold">Log</span>
            <ol className="ct-log flex flex-col">
              {job.log.map((line, i) => (
                <li key={i} className="flex gap-2.5 whitespace-pre">
                  <span style={{ color: 'var(--ct-log-time)' }}>{when(line.at)}</span>
                  <span style={{ color: logColor(line.what) }}>{describe(line.what, line.detail)}</span>
                </li>
              ))}
            </ol>
          </div>
          {retry.error && <p className="text-xs text-ct-failed">{(retry.error as unknown as AppError).message}</p>}

          {(canRetry || isAgent) && (
          <div className="mt-auto flex flex-wrap gap-2 border-t border-ct-border pt-3.5">
            {canRetry && (
              <button type="button" className="ct-btn ct-btn-primary" disabled={retry.isPending} onClick={() => retry.mutate()}>
                {job.hasSession ? 'Resume job' : 'Retry job'}
              </button>
            )}
            {isAgent && (
              <button type="button" className="ct-btn" onClick={() => setSessionOpen(true)}>
                Open agent session
              </button>
            )}
          </div>
          )}
        </>
      )}

      {sessionOpen && job && (
        <Dialog title={`${step.agent} · ${step.id} · attempt ${job.attempt}`} onClose={() => setSessionOpen(false)}>
          <Transcript jobId={job.id} live={job.state === 'running' || job.state === 'waiting'} />
        </Dialog>
      )}
    </div>
  )
}
