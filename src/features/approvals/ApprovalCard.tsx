import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { useRunSelection } from '@/features/runs/useRunSelection'
import { api, type AppError, type Approval, type Run } from '@/lib/api'
import { when } from '@/lib/format'
import { useNav } from '@/lib/nav'

/** What a tool ask wants to do, shortest useful form first. */
export function describeInput(input: unknown): string {
  if (input && typeof input === 'object') {
    const o = input as Record<string, unknown>
    for (const key of ['command', 'file_path', 'url', 'pattern', 'query']) {
      if (typeof o[key] === 'string') return o[key] as string
    }
  }
  return JSON.stringify(input, null, 2)
}

/**
 * A gate (approval step) or a tool ask, in the mockup's orange card: what it is, the run, Approve and
 * Reject. What the agent produced or wants to do opens on demand; Reject first asks for a reason.
 * `showRun` adds the link to the run, for lists outside it.
 */
export function ApprovalCard({ approval, run, showRun = false }: { approval: Approval; run: Run; showRun?: boolean }) {
  const queryClient = useQueryClient()
  const go = useNav((s) => s.go)
  const select = useRunSelection((s) => s.select)
  const [rejecting, setRejecting] = useState(false)
  const [details, setDetails] = useState(false)
  const [reason, setReason] = useState('')
  const decide = useMutation({
    mutationFn: (approve: boolean) => api.decide(approval.id, approve, reason),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['runs'] }),
  })
  const tool = approval.kind === 'tool'
  const job = run.jobs.find((j) => j.id === approval.jobId)
  const title = tool ? `The ${job?.stepId} agent wants to use ${approval.toolName}` : `Approve the step "${job?.stepId}"`

  return (
    <div className="flex flex-col gap-2.5 rounded-lg border p-3" style={{ background: 'var(--ct-wait-card-bg)', borderColor: 'var(--ct-wait-card-border)' }}>
      <div className="flex flex-wrap items-center gap-3">
        <span className="flex min-w-0 flex-[1_1_260px] flex-col gap-0.5">
          <span className="font-semibold">{title}</span>
          <span className="text-xs text-ct-text-2">
            <span className="ct-mono">{run.ticket.key}</span> {run.ticket.title} · {run.workflowName} #{run.number} · {when(approval.createdAt)}
          </span>
        </span>
        <button type="button" className="ct-btn ct-btn-sm ct-btn-approve" disabled={decide.isPending} onClick={() => decide.mutate(true)}>
          Approve
        </button>
        <button
          type="button"
          className="ct-btn ct-btn-sm"
          disabled={decide.isPending}
          onClick={() => (rejecting ? decide.mutate(false) : setRejecting(true))}
        >
          {rejecting ? 'Reject' : 'Reject…'}
        </button>
        {showRun && (
          <button
            type="button"
            className="flex min-h-9 items-center text-[13px] font-medium text-ct-text underline underline-offset-2 hover:underline"
            onClick={() => {
              select(run.id)
              go('runs')
            }}
          >
            Open run
          </button>
        )}
      </div>

      {rejecting && (
        <input
          className="ct-input"
          autoFocus
          aria-label="Reason, sent to the agent"
          placeholder={tool ? 'Why not (optional, the agent sees it)' : 'What to change (optional, sent back to the agent)'}
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && decide.mutate(false)}
        />
      )}
      {details ? (
        <pre className="max-h-64 overflow-auto rounded-md bg-ct-surface p-2.5 text-xs whitespace-pre-wrap ct-mono">
          {tool ? describeInput(approval.input) : approval.summary}
        </pre>
      ) : (
        <button type="button" className="self-start text-xs text-ct-text-3 underline underline-offset-2" onClick={() => setDetails(true)}>
          {tool ? 'Show what it wants to do' : 'Show what to approve'}
        </button>
      )}
      {decide.error && <p className="text-xs text-ct-failed">{(decide.error as unknown as AppError).message}</p>}
    </div>
  )
}
