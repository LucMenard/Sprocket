import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Send } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { api, type AppError, type Note, type Run } from '@/lib/api'
import { when } from '@/lib/format'
import { allSteps } from '@/lib/steps'

/** Who a note can be for: the run's steps that an agent does, and their agent types. */
function addressees(run: Run) {
  const steps = allSteps(run.steps).filter((s) => s.agent)
  return [...new Set([...steps.map((s) => s.id), ...steps.map((s) => s.agent!)])]
}

function NoteRow({ note }: { note: Note }) {
  const person = !note.agent
  return (
    <li className={`flex flex-col gap-1 ${person ? 'items-end' : 'items-start'}`}>
      <span className="text-xs text-ct-text-3">
        <strong className="font-semibold text-ct-text-2">{note.author}</strong>
        {note.agent && <> · {note.agent}</>}
        {note.to && <> → {note.to}</>} · {when(note.at)}
      </span>
      <p
        className="max-w-[92%] rounded-xl border px-3 py-2 text-[13px] leading-normal whitespace-pre-wrap"
        style={
          person
            ? { background: 'var(--ct-accent-soft)', borderColor: 'var(--ct-accent-ring)' }
            : { background: 'var(--ct-surface)', borderColor: 'var(--ct-border)' }
        }
      >
        {note.body}
      </p>
    </li>
  )
}

/**
 * The run's board as a chat: what its agents posted for each other and for you, the reasons of rejections,
 * and your own notes, which the agents read at their next look.
 */
export function Board({ run, enabled }: { run: Run; enabled: boolean }) {
  const queryClient = useQueryClient()
  const [body, setBody] = useState('')
  const [to, setTo] = useState('')
  const end = useRef<HTMLLIElement>(null)
  const post = useMutation<Note, AppError>({
    mutationFn: () => api.postNote(run.id, body, to || null),
    onSuccess: () => {
      setBody('')
      void queryClient.invalidateQueries({ queryKey: ['runs'] })
    },
  })
  useEffect(() => {
    end.current?.scrollIntoView({ block: 'end' })
  }, [run.notes.length])

  if (!enabled) {
    return <p className="text-ct-text-3">This workflow has no board. Turn it on in its settings, in Workflows.</p>
  }
  return (
    <div className="flex min-h-full flex-col gap-3">
      <ol className="flex flex-1 flex-col gap-3" aria-label="Notes">
        {run.notes.length === 0 && <li className="text-ct-text-3">No notes yet. Agents post here what later steps should know; you can too.</li>}
        {run.notes.map((n) => (
          <NoteRow key={n.id} note={n} />
        ))}
        <li ref={end} aria-hidden />
      </ol>
      <form
        className="sticky bottom-0 flex flex-col gap-2 border-t border-ct-border bg-ct-surface pt-3"
        onSubmit={(e) => {
          e.preventDefault()
          if (body.trim()) post.mutate()
        }}
      >
        <textarea
          aria-label="Your note"
          rows={3}
          className="ct-input h-auto py-2 leading-normal"
          placeholder="A note for the agents of this run"
          value={body}
          onChange={(e) => setBody(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && body.trim()) post.mutate()
          }}
        />
        <div className="flex items-center gap-2">
          <select aria-label="For" className="ct-input h-9 flex-1" value={to} onChange={(e) => setTo(e.target.value)}>
            <option value="">For everyone</option>
            {addressees(run).map((a) => (
              <option key={a} value={a}>
                For {a}
              </option>
            ))}
          </select>
          <button type="submit" className="ct-btn ct-btn-primary h-9" disabled={!body.trim() || post.isPending}>
            <Send className="size-4" /> Post
          </button>
        </div>
        {post.error && <p className="text-xs text-ct-failed">{post.error.message}</p>}
      </form>
    </div>
  )
}
