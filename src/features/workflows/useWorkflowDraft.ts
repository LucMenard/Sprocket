import { useState } from 'react'
import type { Workflow, WorkflowDraft, WorkflowStep } from '@/lib/api'

export function draftOf(w: Workflow): WorkflowDraft {
  return {
    name: w.name,
    description: w.description,
    offerInTracket: w.offerInTracket,
    maxParallelRuns: w.maxParallelRuns,
    steps: w.steps.map((s) => ({ ...s })),
  }
}

const output = (id: string) => `steps.${id}.output`

/** A step name free in the draft: `review`, `review-2`… */
function freeId(steps: WorkflowStep[], base: string) {
  let id = base
  for (let n = 2; steps.some((s) => s.id === id); n++) id = `${base}-${n}`
  return id
}

/**
 * The workflow being edited: a copy of the saved one, the edits on it, and whether it differs.
 * References between steps (`steps.<id>.output`, onReject) follow renames, moves and deletions.
 */
export function useWorkflowDraft(saved: Workflow | undefined) {
  const [draft, setDraft] = useState<WorkflowDraft | null>(null)
  const [base, setBase] = useState<Workflow | undefined>(saved)
  // Another workflow, or the saved one changed (after a save): start again from it
  if (saved !== base) {
    setBase(saved)
    setDraft(null)
  }
  const current = draft ?? (saved ? draftOf(saved) : null)
  const dirty = !!draft && !!saved && JSON.stringify(draft) !== JSON.stringify(draftOf(saved))

  const update = (change: (d: WorkflowDraft) => WorkflowDraft) => current && setDraft(change(structuredClone(current)))

  /** Keeps only references to steps before each step */
  const tidy = (d: WorkflowDraft): WorkflowDraft => {
    d.steps.forEach((s, i) => {
      const before = new Set(d.steps.slice(0, i).map((p) => p.id))
      const ref = s.input?.match(/^steps\.(.+)\.output$/)?.[1]
      if (ref && !before.has(ref)) s.input = null
      if (s.onReject && !before.has(s.onReject)) s.onReject = null
    })
    return d
  }

  return {
    draft: current,
    dirty,
    discard: () => setDraft(null),
    setField: <K extends keyof WorkflowDraft>(key: K, value: WorkflowDraft[K]) => update((d) => ({ ...d, [key]: value })),
    setStep: (id: string, change: Partial<WorkflowStep>) =>
      update((d) => {
        const step = d.steps.find((s) => s.id === id)
        if (!step) return d
        const renamed = change.id !== undefined && change.id !== id ? change.id : null
        Object.assign(step, change)
        if (renamed) {
          for (const s of d.steps) {
            if (s.input === output(id)) s.input = output(renamed)
            if (s.onReject === id) s.onReject = renamed
          }
        }
        return tidy(d)
      }),
    /** Adds a step at position `at` (0 is first); returns its name */
    addStep: (kind: WorkflowStep['kind'], at: number, agent: string | null) => {
      if (!current) return null
      const id = freeId(current.steps, kind === 'agent' ? 'work' : 'approve')
      update((d) => {
        const previous = d.steps[at - 1]
        d.steps.splice(at, 0, {
          id,
          kind,
          agent: kind === 'agent' ? agent : null,
          instructions: null,
          // An approval shows what the step before it produced, and sends a rejection back to it
          input: kind === 'approval' && previous?.kind === 'agent' ? output(previous.id) : null,
          onReject: kind === 'approval' && previous?.kind === 'agent' ? previous.id : null,
        })
        return d
      })
      return id
    },
    moveStep: (id: string, by: -1 | 1) =>
      update((d) => {
        const i = d.steps.findIndex((s) => s.id === id)
        const j = i + by
        if (i < 0 || j < 0 || j >= d.steps.length) return d
        ;[d.steps[i], d.steps[j]] = [d.steps[j], d.steps[i]]
        return tidy(d)
      }),
    removeStep: (id: string) => update((d) => tidy({ ...d, steps: d.steps.filter((s) => s.id !== id) })),
  }
}

export type WorkflowEditing = ReturnType<typeof useWorkflowDraft>
