import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { open } from '@tauri-apps/plugin-dialog'
import { Plus, X } from 'lucide-react'
import { useEffect, useState, type ReactNode } from 'react'
import { useRunSelection } from '@/features/runs/useRunSelection'
import { api, type AfterWhen, type AppError, type Trigger, type TriggerStatus, type WorkflowDraft } from '@/lib/api'
import { when } from '@/lib/format'
import { useNav } from '@/lib/nav'
import { useStatus } from '@/lib/queries'
import type { WorkflowEditing } from './useWorkflowDraft'

type Kind = 'schedule' | 'after' | 'webhook'

const kindOf = (t: Trigger): Kind => (t.schedule != null ? 'schedule' : t.after != null ? 'after' : 'webhook')

const PRESETS: { label: string; cron: string }[] = [
  { label: 'Every hour', cron: '0 * * * *' },
  { label: 'Every day at 9:00', cron: '0 9 * * *' },
  { label: 'Weekdays at 9:00', cron: '0 9 * * 1-5' },
  { label: 'Mondays at 8:00', cron: '0 8 * * 1' },
]

const WHEN: { id: AfterWhen; label: string }[] = [
  { id: 'passed', label: 'passes' },
  { id: 'failed', label: 'fails' },
  { id: 'ended', label: 'passes or fails' },
]

function Row({ label, htmlFor, help, children }: { label: string; htmlFor: string; help?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={htmlFor} className="ct-label">
        {label}
      </label>
      {children}
      {help && <span className="text-xs text-ct-text-3">{help}</span>}
    </div>
  )
}

function FolderInput({ id, value, onChange }: { id: string; value: string; onChange: (v: string) => void }) {
  return (
    <div className="flex gap-2">
      <input id={id} className="ct-input min-w-0 flex-1 ct-mono" placeholder="C:\dev\my-repo" value={value} onChange={(e) => onChange(e.target.value)} />
      <button
        type="button"
        className="ct-btn"
        onClick={async () => {
          const picked = await open({ directory: true })
          if (typeof picked === 'string') onChange(picked)
        }}
      >
        Browse…
      </button>
    </div>
  )
}

/** The next times a schedule gives, or why it can't be read: asked after typing stops. */
function SchedulePreview({ expression }: { expression: string }) {
  const [typed, setTyped] = useState(expression)
  useEffect(() => {
    const id = window.setTimeout(() => setTyped(expression), 300)
    return () => window.clearTimeout(id)
  }, [expression])
  const preview = useQuery({ queryKey: ['schedule', typed.trim()], queryFn: () => api.schedulePreview(typed.trim()), enabled: !!typed.trim(), retry: false })
  if (!typed.trim()) return null
  if (preview.error) return <span className="text-xs text-ct-failed">{(preview.error as unknown as AppError).message}</span>
  if (!preview.data) return null
  return <span className="text-xs text-ct-text-3">Next: {preview.data.map((t) => when(t)).join(' · ')}</span>
}

function LastStart({ status }: { status?: TriggerStatus }) {
  const select = useRunSelection((s) => s.select)
  const go = useNav((s) => s.go)
  if (!status) return null
  return (
    <div className="flex flex-col gap-0.5 text-xs text-ct-text-3">
      {status.error && <span className="text-ct-failed">Last try, {when(status.triedAt)}: {status.error}</span>}
      {status.runId && (
        <span>
          Last run:{' '}
          <button
            type="button"
            className="underline hover:text-ct-text"
            onClick={() => {
              select(status.runId)
              go('runs')
            }}
          >
            #{status.runNumber}
          </button>
          {status.runState ? ` · ${status.runState}` : ''}
          {!status.error && status.triedAt ? ` · ${when(status.triedAt)}` : ''}
        </span>
      )}
    </div>
  )
}

function TriggerCard({
  index,
  trigger,
  status,
  workflowId,
  launchable,
  onChange,
  onRemove,
}: {
  index: number
  trigger: Trigger
  status?: TriggerStatus
  workflowId: string | null
  launchable: { id: string; name: string }[]
  onChange: (t: Trigger) => void
  onRemove: () => void
}) {
  const apiAddress = useStatus().data?.apiAddress
  const kind = kindOf(trigger)
  const id = (name: string) => `trigger-${index}-${name}`
  const set = (change: Partial<Trigger>) => onChange({ ...trigger, ...change })

  return (
    <div className="flex flex-col gap-3 rounded-lg border border-ct-border p-3">
      <div className="flex items-center gap-2">
        <span className="font-semibold">{kind === 'schedule' ? 'On a schedule' : kind === 'after' ? 'After another workflow' : 'Webhook'}</span>
        <button type="button" className="ct-btn ml-auto size-8 p-0" aria-label={`Remove trigger ${index + 1}`} title="Remove" onClick={onRemove}>
          <X className="size-4" />
        </button>
      </div>

      {kind === 'schedule' && (
        <Row
          label="When"
          htmlFor={id('cron')}
          help={
            <>
              Five fields, in this computer's time: minute hour day month weekday. <SchedulePreview expression={trigger.schedule ?? ''} />
            </>
          }
        >
          <input id={id('cron')} className="ct-input ct-mono" placeholder="0 9 * * 1-5" value={trigger.schedule ?? ''} onChange={(e) => set({ schedule: e.target.value })} />
          <div className="flex flex-wrap gap-1">
            {PRESETS.map((p) => (
              <button key={p.cron} type="button" className="rounded-md border border-ct-border px-2 py-0.5 text-xs hover:bg-ct-subtle" onClick={() => set({ schedule: p.cron })}>
                {p.label}
              </button>
            ))}
          </div>
        </Row>
      )}

      {kind === 'after' && (
        <div className="flex flex-col gap-1.5">
          <label htmlFor={id('after')} className="ct-label">
            When a run of
          </label>
          <div className="flex flex-wrap items-center gap-2">
            <select id={id('after')} className="ct-input min-w-0 flex-1" value={trigger.after ?? ''} onChange={(e) => set({ after: e.target.value })}>
              <option value="">Choose a workflow</option>
              {trigger.after && !launchable.some((w) => w.id === trigger.after) && <option value={trigger.after}>{trigger.after}</option>}
              {launchable.map((w) => (
                <option key={w.id} value={w.id}>
                  {w.name}
                </option>
              ))}
            </select>
            <select
              aria-label="Which end"
              className="ct-input w-auto"
              value={trigger.when ?? 'passed'}
              onChange={(e) => set({ when: e.target.value === 'passed' ? null : (e.target.value as AfterWhen) })}
            >
              {WHEN.map((w) => (
                <option key={w.id} value={w.id}>
                  {w.label}
                </option>
              ))}
            </select>
          </div>
          <span className="text-xs text-ct-text-3">
            It runs on that run's ticket, in its folder. Only runs started from outside count, not ones a launch step started, and a
            chain that would come back to a workflow stops there.
          </span>
        </div>
      )}

      {kind !== 'after' && (
        <Row label="Folder it works in" htmlFor={id('folder')} help="A git repository on this computer. The run has no Jira ticket and works in this folder itself, not in a worktree.">
          <FolderInput id={id('folder')} value={trigger.folder ?? ''} onChange={(folder) => set({ folder })} />
        </Row>
      )}

      <Row
        label={kind === 'after' ? 'What it is asked (optional)' : 'What it is asked'}
        htmlFor={id('request')}
        help={kind === 'after' ? "Left empty: that run's request." : kind === 'webhook' ? "The call's request, when it sends one, replaces it." : 'Steps read it as {{ request }}.'}
      >
        <textarea id={id('request')} className="ct-input min-h-16" value={trigger.request ?? ''} onChange={(e) => set({ request: e.target.value })} />
      </Row>

      {kind === 'webhook' && (
        <div className="flex flex-col gap-1.5 text-xs text-ct-text-3">
          <span>
            Another tool on this computer starts it with a POST, with the API token (Integrations) as a bearer token. The body is
            optional: <code className="ct-mono">{'{"request": "…"}'}</code>.
          </span>
          <code className="rounded-md bg-ct-subtle px-2 py-1 break-all ct-mono text-ct-text">
            POST http://{apiAddress ?? '127.0.0.1:…'}/v1/workflows/{workflowId ?? '<save it first>'}/start
          </code>
        </div>
      )}

      <LastStart status={status} />
    </div>
  )
}

/** Run now: the saved workflow, on a repository, with no ticket. */
function RunNow({ workflowId, dirty, forRequests }: { workflowId: string; dirty: boolean; forRequests: boolean }) {
  const queryClient = useQueryClient()
  const select = useRunSelection((s) => s.select)
  const go = useNav((s) => s.go)
  const [opened, setOpened] = useState(false)
  const [folder, setFolder] = useState('')
  const [request, setRequest] = useState('')
  const start = useMutation({
    mutationFn: () => api.runNow(workflowId, folder.trim(), request.trim() || null),
    onSuccess: (run) => {
      void queryClient.invalidateQueries({ queryKey: ['runs'] })
      select(run.id)
      go('runs')
    },
  })

  if (!opened)
    return (
      <button type="button" className="ct-btn self-start" onClick={() => setOpened(true)}>
        Run now…
      </button>
    )
  return (
    <form
      className="flex flex-col gap-3 rounded-lg border border-ct-border p-3"
      onSubmit={(e) => {
        e.preventDefault()
        start.mutate()
      }}
    >
      <span className="font-semibold">Run now</span>
      {dirty && <span className="text-xs text-ct-waiting">It runs the saved workflow, without the changes not saved yet.</span>}
      <Row label="Folder" htmlFor="run-now-folder" help="A git repository on this computer. The run has no Jira ticket.">
        <FolderInput id="run-now-folder" value={folder} onChange={setFolder} />
      </Row>
      <Row label={forRequests ? 'What should it do?' : 'What it is asked (optional)'} htmlFor="run-now-request">
        <textarea id="run-now-request" className="ct-input min-h-16" required={forRequests} value={request} onChange={(e) => setRequest(e.target.value)} />
      </Row>
      {start.error && <span className="text-xs text-ct-failed">{(start.error as unknown as AppError).message}</span>}
      <div className="flex gap-2">
        <button type="submit" className="ct-btn ct-btn-primary" disabled={start.isPending || !folder.trim()}>
          Start
        </button>
        <button type="button" className="ct-btn" onClick={() => setOpened(false)}>
          Cancel
        </button>
      </div>
    </form>
  )
}

/** The trigger node's settings: Tracket, Run now, and the triggers that start the workflow by themselves. */
export function TriggersPanel({
  draft,
  editing,
  workflowId,
  dirty,
  launchable,
}: {
  draft: WorkflowDraft
  editing: WorkflowEditing
  /** The saved workflow, if it is saved */
  workflowId: string | null
  dirty: boolean
  launchable: { id: string; name: string }[]
}) {
  const statuses = useQuery({
    queryKey: ['triggers', workflowId],
    queryFn: () => api.triggerStatuses(workflowId ?? ''),
    enabled: !!workflowId,
    refetchInterval: 15_000,
  })
  const triggers = draft.triggers
  const setTriggers = (next: Trigger[]) => editing.setField('triggers', next)
  const add = (kind: Kind) =>
    setTriggers([...triggers, kind === 'schedule' ? { schedule: '0 9 * * 1-5', folder: '' } : kind === 'after' ? { after: '' } : { webhook: true, folder: '' }])

  return (
    <div className="flex flex-col gap-3">
      <span className="ct-label">Run now</span>
      {workflowId ? (
        <RunNow workflowId={workflowId} dirty={dirty} forRequests={draft.forRequests} />
      ) : (
        <span className="text-xs text-ct-text-3">Save the workflow to run it.</span>
      )}

      <span className="ct-label mt-2">Starts by itself</span>
      {triggers.length === 0 && <span className="text-xs text-ct-text-3">Nothing yet: add a schedule, another workflow to follow, or a webhook.</span>}
      {triggers.map((t, i) => (
        <TriggerCard
          key={i}
          index={i}
          trigger={t}
          // Statuses are of the saved triggers: shown only while this one is unchanged
          status={!dirty ? statuses.data?.[i] : undefined}
          workflowId={workflowId}
          launchable={launchable}
          onChange={(next) => setTriggers(triggers.map((old, j) => (j === i ? next : old)))}
          onRemove={() => setTriggers(triggers.filter((_, j) => j !== i))}
        />
      ))}
      <div className="flex flex-wrap gap-2">
        <button type="button" className="ct-btn" onClick={() => add('schedule')}>
          <Plus className="size-4" /> Schedule
        </button>
        <button type="button" className="ct-btn" onClick={() => add('after')}>
          <Plus className="size-4" /> After a workflow
        </button>
        {!triggers.some((t) => t.webhook) && (
          <button type="button" className="ct-btn" onClick={() => add('webhook')}>
            <Plus className="size-4" /> Webhook
          </button>
        )}
      </div>
      {triggers.some((t) => t.schedule != null) && (
        <span className="text-xs text-ct-text-3">
          Schedules run while Sprocket is open (it keeps running in the tray). A time missed while it was closed is skipped, not
          caught up on. Approvals wait until someone answers in Sprocket.
        </span>
      )}
    </div>
  )
}
