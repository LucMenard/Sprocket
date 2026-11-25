import { useState } from 'react'
import type { StepKind, Workflow, WorkflowDraft, WorkflowStep } from '@/lib/api'
import { allSteps, emptyStep, findPlace, readable, type Branch } from '@/lib/steps'

export function draftOf(w: Workflow): WorkflowDraft {
  return {
    name: w.name,
    description: w.description,
    offerInTracket: w.offerInTracket,
    maxParallelRuns: w.maxParallelRuns,
    board: w.board,
    steps: structuredClone(w.steps),
  }
}

/** A step name free in the draft: `review`, `review-2`… */
function freeId(steps: WorkflowStep[], base: string, taken: string[] = []) {
  const used = new Set([...allSteps(steps).map((s) => s.id), ...taken])
  let id = base
  for (let n = 2; used.has(id); n++) id = `${base}-${n}`
  return id
}

const BASE: Record<StepKind, string> = { agent: 'work', approval: 'approve', action: 'tell', condition: 'check', forEach: 'each' }

/** `steps.old.` → `steps.new.` in a text */
const renameIn = (text: string | null | undefined, from: string, to: string) =>
  text?.replace(new RegExp(`\\bsteps\\.${from.replace(/[-]/g, '\\-')}\\.`, 'g'), `steps.${to}.`) ?? null

/** Where a new node goes: in a block (the workflow's, a branch, a loop), at a place. */
export interface Slot {
  parent: string | null
  branch: Branch | null
  index: number
}

/**
 * The workflow being edited: a copy of the saved one, the edits on it, and whether it differs.
 * Renames follow every `steps.<id>.` reference and `onReject`; the rest is checked when saving.
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

  const update = (change: (d: WorkflowDraft) => void) => {
    if (!current) return
    const d = structuredClone(current)
    change(d)
    // onReject only goes back to an earlier step next to the approval
    for (const s of allSteps(d.steps)) {
      if (!s.onReject) continue
      const place = findPlace(d.steps, s.id)
      if (!place?.block.slice(0, place.index).some((b) => b.id === s.onReject)) s.onReject = null
    }
    setDraft(d)
  }

  const blockOf = (d: WorkflowDraft, slot: Slot): WorkflowStep[] | null => {
    if (!slot.parent) return d.steps
    const parent = findPlace(d.steps, slot.parent)?.step
    return parent && slot.branch ? parent[slot.branch] : null
  }

  return {
    draft: current,
    dirty,
    discard: () => setDraft(null),
    setField: <K extends keyof WorkflowDraft>(key: K, value: WorkflowDraft[K]) => update((d) => void (d[key] = value)),
    setStep: (id: string, change: Partial<WorkflowStep>) =>
      update((d) => {
        const step = findPlace(d.steps, id)?.step
        if (!step) return
        const renamed = change.id !== undefined && change.id !== id ? change.id : null
        Object.assign(step, change)
        if (!renamed) return
        for (const s of allSteps(d.steps)) {
          s.input = renameIn(s.input, id, renamed)
          s.instructions = renameIn(s.instructions, id, renamed)
          s.if = renameIn(s.if, id, renamed)
          s.forEach = renameIn(s.forEach, id, renamed)
          if (s.action) s.action = { ...s.action, text: renameIn(s.action.text, id, renamed) ?? '', to: renameIn(s.action.to, id, renamed) }
          if (s.onReject === id) s.onReject = renamed
        }
      }),
    /** Adds a node of `kind` in `slot`, set up from what it can read there; returns its name */
    addStep: (kind: StepKind, slot: Slot, agent: string | null) => {
      if (!current) return null
      const id = freeId(current.steps, BASE[kind])
      update((d) => {
        const block = blockOf(d, slot)
        if (!block) return
        const step = emptyStep(id, kind)
        const previous = block[slot.index - 1]
        const work = () => ({ ...emptyStep(freeId(d.steps, 'work', [id]), agent ? 'agent' : 'approval'), agent })
        // What it can read once in place: the last earlier step that gives something
        block.splice(slot.index, 0, step)
        const source = readable(d.steps, id).steps.filter((s) => s.kind === 'agent' || s.kind === 'action').at(-1)
        const listField = source?.output.find((f) => f.type === 'list')
        switch (kind) {
          case 'agent':
            step.agent = agent
            break
          case 'approval':
            // An approval shows what the step before it produced, and sends a rejection back to it
            if (previous?.kind === 'agent') {
              step.input = `steps.${previous.id}.output`
              step.onReject = previous.id
            }
            break
          case 'action':
            step.agent = agent
            step.action = { do: 'comment', text: source ? `{{ steps.${source.id}.output }}` : 'Done', to: null }
            break
          case 'condition':
            step.if = source ? `steps.${source.id}.output` : 'true'
            step.then = [work()]
            break
          case 'forEach':
            step.forEach = source ? `steps.${source.id}.output${listField ? `.${listField.name}` : ''}` : "'one item'"
            step.as = 'item'
            step.steps = [work()]
            break
        }
      })
      return id
    },
    /** One place up or down among the steps next to it */
    moveStep: (id: string, by: -1 | 1) =>
      update((d) => {
        const place = findPlace(d.steps, id)
        if (!place) return
        const j = place.index + by
        if (j < 0 || j >= place.block.length) return
        ;[place.block[place.index], place.block[j]] = [place.block[j], place.block[place.index]]
      }),
    /** Removes a node, with what is inside it */
    removeStep: (id: string) =>
      update((d) => {
        const place = findPlace(d.steps, id)
        place?.block.splice(place.index, 1)
      }),
  }
}

export type WorkflowEditing = ReturnType<typeof useWorkflowDraft>
