import { useMutation, useQueryClient } from '@tanstack/react-query'
import { open } from '@tauri-apps/plugin-dialog'
import { useState, type FormEvent, type ReactNode } from 'react'
import { useWorkflows } from '@/features/workflows/useWorkflows'
import { api, type AppError, type Run } from '@/lib/api'

function Field({ id, label, children }: { id: string; label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={id} className="ct-label">
        {label}
      </label>
      {children}
    </div>
  )
}

/** Starts a run by hand, for when Tracket isn't the one starting it (or to try a workflow out). */
export function StartRunForm({ onStarted, onCancel }: { onStarted: (run: Run) => void; onCancel: () => void }) {
  const queryClient = useQueryClient()
  const { data } = useWorkflows()
  const workflows = data?.workflows ?? []
  const [workflow, setWorkflow] = useState('')
  const [key, setKey] = useState('')
  const [title, setTitle] = useState('')
  const [url, setUrl] = useState('')
  const [folder, setFolder] = useState('')

  const start = useMutation({
    mutationFn: () =>
      api.startRun({
        workflow: workflow || workflows[0]?.id || '',
        ticket: { key: key.trim().toUpperCase(), title: title.trim(), url: url.trim() },
        folder: folder.trim(),
        startedBy: 'Sprocket',
      }),
    onSuccess: (run) => {
      void queryClient.invalidateQueries({ queryKey: ['runs'] })
      onStarted(run)
    },
  })

  const submit = (e: FormEvent) => {
    e.preventDefault()
    start.mutate()
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <h2 className="text-[17px] font-bold">Start a run</h2>
        <p className="text-[13px] text-ct-text-3">Tracket starts runs from a ticket. This does the same by hand, in a worktree you already have.</p>
      </div>
      <Field id="sr-workflow" label="Workflow">
        <select id="sr-workflow" className="ct-input" value={workflow || workflows[0]?.id || ''} onChange={(e) => setWorkflow(e.target.value)}>
          {workflows.map((w) => (
            <option key={w.id} value={w.id}>
              {w.name}
            </option>
          ))}
        </select>
      </Field>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-[10rem_1fr]">
        <Field id="sr-key" label="Ticket">
          <input id="sr-key" className="ct-input ct-mono" required placeholder="SIS-244" value={key} onChange={(e) => setKey(e.target.value)} />
        </Field>
        <Field id="sr-title" label="Title">
          <input id="sr-title" className="ct-input" required value={title} onChange={(e) => setTitle(e.target.value)} />
        </Field>
      </div>
      <Field id="sr-url" label="Link">
        <input id="sr-url" className="ct-input" required type="url" placeholder="https://acme.atlassian.net/browse/SIS-244" value={url} onChange={(e) => setUrl(e.target.value)} />
      </Field>
      <Field id="sr-folder" label="Worktree folder">
        <div className="flex gap-2">
          <input id="sr-folder" className="ct-input ct-mono" required value={folder} onChange={(e) => setFolder(e.target.value)} />
          <button
            type="button"
            className="ct-btn"
            onClick={async () => {
              const picked = await open({ directory: true })
              if (typeof picked === 'string') setFolder(picked)
            }}
          >
            Browse…
          </button>
        </div>
      </Field>
      {start.error && <p className="text-[13px] text-ct-failed">{(start.error as unknown as AppError).message}</p>}
      <div className="flex gap-2">
        <button type="submit" className="ct-btn ct-btn-primary" disabled={start.isPending || workflows.length === 0}>
          Start run
        </button>
        <button type="button" className="ct-btn" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  )
}
