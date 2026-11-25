import { useRef, useState } from 'react'
import type { Point, StepKind, Workflow, WorkflowDraft, WorkflowStep } from '@/lib/api'
import { workflowFlow, type FlowEdge, type LinkRef } from '@/lib/flow'
import { type Branch, allSteps, emptyStep, findPlace, isWithin, readable } from '@/lib/steps'

export function draftOf(w: Workflow): WorkflowDraft {
  return {
    name: w.name,
    description: w.description,
    offerInTracket: w.offerInTracket,
    maxParallelRuns: w.maxParallelRuns,
    board: w.board,
    steps: structuredClone(w.steps),
    layout: structuredClone(w.layout ?? {}),
    loose: [],
    cut: [],
    extra: [],
  }
}

/**
 * What stops the draft from being saved, in words: unlinked nodes, deleted links, branches and loops left empty,
 * steps without an agent type. Saving checks the rest.
 */
export function draftProblems(d: WorkflowDraft): string[] {
  const out: string[] = []
  if (!d.name.trim()) out.push('The workflow has no name.')
  if (!d.steps.length) out.push('Nothing follows the trigger: link a node after it.')
  const loose = d.loose.flatMap((chain) => allSteps(chain)).map((s) => s.id)
  if (loose.length) out.push(`Not linked: ${loose.join(', ')}. Drag a bottom dot onto them, or delete them.`)
  for (const x of d.extra) {
    const [from, to] = x.split('>')
    out.push(
      to === '$end'
        ? `${from} → Done can't be saved: ${from} is inside a branch or a loop, so the run goes on after it. Delete this link.`
        : `${from} → ${to} can't be saved: ${to} holds ${from}, so it can't run after it. Delete this link.`,
    )
  }
  for (const c of d.cut) {
    const [from, to] = c.split('>')
    out.push(`The link ${from.replace('$trigger', 'Tracket')} → ${to === '$end' ? 'Done' : to} is deleted: link them again, or change the steps.`)
  }
  for (const s of allSteps(d.steps)) {
    if (s.kind === 'condition' && !s.then.length && !s.else.length) out.push(`The condition ${s.id} has no branch.`)
    if (s.kind === 'forEach' && !s.steps.length) out.push(`The loop ${s.id} has no steps.`)
    if ((s.kind === 'agent' || s.kind === 'action') && !s.agent) out.push(`${s.id} has no agent type.`)
  }
  return out
}

/** Every step of the draft, the unlinked ones too. */
const everyStep = (d: WorkflowDraft) => [...allSteps(d.steps), ...d.loose.flatMap((chain) => allSteps(chain))]

/** Where a step is in the draft: in the workflow, or in a chain no link reaches. */
const find = (d: WorkflowDraft, id: string) => findPlace(d.steps, id) ?? d.loose.reduce<ReturnType<typeof findPlace>>((f, chain) => f ?? findPlace(chain, id), null)

/** A step name free in the draft: `review`, `review-2`… */
function freeId(d: WorkflowDraft, base: string, taken: string[] = []) {
  const used = new Set([...everyStep(d).map((s) => s.id), ...taken])
  let id = base
  for (let n = 2; used.has(id); n++) id = `${base}-${n}`
  return id
}

const BASE: Record<StepKind, string> = { agent: 'work', approval: 'approve', action: 'tell', condition: 'check', forEach: 'each', parallel: 'together' }

/** `steps.old.` → `steps.new.` in a text */
const renameIn = (text: string | null | undefined, from: string, to: string) =>
  text?.replace(new RegExp(`\\bsteps\\.${from.replace(/[-]/g, '\\-')}\\.`, 'g'), `steps.${to}.`) ?? null

/** Where a new node goes: in a block (the workflow's, a branch, a loop, a parallel branch), at a place. */
export interface Slot {
  parent: string | null
  branch: Branch | null
  /** Which of a parallel group's branches */
  lane?: number
  index: number
}

/**
 * After an edit: a parallel group keeps its non-empty branches, and becomes plain steps again when it has
 * only one left.
 */
function tidyGroups(block: WorkflowStep[]) {
  for (let i = 0; i < block.length; i++) {
    const s = block[i]
    for (const inner of [s.then, s.else, s.steps, ...s.branches]) tidyGroups(inner)
    if (s.kind !== 'parallel') continue
    s.branches = s.branches.filter((b) => b.length > 0)
    if (s.branches.length <= 1) {
      block.splice(i, 1, ...(s.branches[0] ?? []))
      i--
    }
  }
}

/**
 * The workflow being edited: a copy of the saved one, the edits on it, and whether it differs.
 * Renames follow every `steps.<id>.` reference and `onReject`; the rest is checked when saving.
 */
export function useWorkflowDraft(saved: Workflow | undefined) {
  const [draft, setDraft] = useState<WorkflowDraft | null>(null)
  const [base, setBase] = useState<Workflow | undefined>(saved)
  // Undo and redo: the drafts before and after this one (null: the saved workflow)
  const [history, setHistory] = useState<{ past: (WorkflowDraft | null)[]; future: (WorkflowDraft | null)[] }>({ past: [], future: [] })
  // Typing in one field makes one undo step, not one per key
  const lastEdit = useRef<{ tag: string; at: number } | null>(null)
  // Another workflow, or the saved one changed (after a save): start again from it
  if (saved !== base) {
    setBase(saved)
    setDraft(null)
    setHistory({ past: [], future: [] })
  }
  const current = draft ?? (saved ? draftOf(saved) : null)
  const dirty = !!draft && !!saved && JSON.stringify(draft) !== JSON.stringify(draftOf(saved))

  const update = (change: (d: WorkflowDraft) => void, tag?: string) => {
    if (!current) return
    const now = Date.now()
    const same = !!tag && lastEdit.current?.tag === tag && now - lastEdit.current.at < 1500
    lastEdit.current = tag ? { tag, at: now } : null
    if (!same) setHistory((h) => ({ past: [...h.past.slice(-99), draft], future: [] }))
    const d = structuredClone(current)
    change(d)
    tidyGroups(d.steps)
    d.loose.forEach(tidyGroups)
    d.loose = d.loose.filter((chain) => chain.length > 0)
    // onReject only goes back to an earlier step next to the approval
    for (const s of everyStep(d)) {
      if (!s.onReject) continue
      const place = find(d, s.id)
      if (!place?.block.slice(0, place.index).some((b) => b.id === s.onReject)) s.onReject = null
    }
    // A deleted link whose nodes have changed since is forgotten
    // Impossible links whose nodes are gone are forgotten
    const ids = new Set(everyStep(d).map((s) => s.id))
    d.extra = d.extra.filter((x) => x.split('>').every((id) => id.startsWith('$') || ids.has(id)))
    if (d.cut.length) {
      const drawn = new Set(workflowFlow(d.steps, 'design', () => ({}), { trigger: {}, end: {} }, { loose: d.loose }).edges.map((e) => `${e.from}>${e.to}`))
      d.cut = d.cut.filter((c) => drawn.has(c))
    }
    setDraft(d)
  }

  const blockOf = (d: WorkflowDraft, slot: Slot): WorkflowStep[] | null => {
    if (!slot.parent) return d.steps
    const parent = find(d, slot.parent)?.step
    if (!parent || !slot.branch) return null
    return slot.branch === 'branches' ? (parent.branches[slot.lane ?? 0] ?? null) : parent[slot.branch]
  }

  /**
   * `to` runs after `from`. When `from` has nothing after it (or is a condition or loop with an empty branch or
   * body), `to` goes there; when something follows it already, the two run side by side in a parallel group.
   * An approval linked to an earlier step next to it gets a rejection link instead.
   * Returns why it can't be done, or null.
   */
  const connect = (from: string, to: string): string | null => {
    if (!current || from === to || to === '$trigger') return null
    // Linking again what was deleted
    if (current.cut.includes(`${from}>${to}`)) {
      update((d) => void (d.cut = d.cut.filter((c) => c !== `${from}>${to}`)))
      return null
    }
    // What the workflow can't hold is drawn anyway; saving waits until it goes
    const impossible = () => {
      if (!current.extra.includes(`${from}>${to}`)) update((d) => void (d.extra = [...d.extra, `${from}>${to}`]))
      return null
    }
    if (to === '$end') {
      const at = from === '$trigger' ? null : find(current, from)
      if (from === '$trigger') {
        // Nothing runs: every step waits, unlinked
        if (current.steps.length) update((d) => void d.loose.push(d.steps.splice(0)))
        return null
      }
      // A step of the workflow itself ends the run: what came after it waits, unlinked
      if (at && at.block === current.steps) {
        if (at.index < at.block.length - 1) update((d) => void d.loose.push(find(d, from)!.block.splice(at.index + 1)))
        return null
      }
      return impossible()
    }
    const target = find(current, to)
    if (!target) return null
    const source = from === '$trigger' ? null : find(current, from)
    if (from !== '$trigger' && !source) return null
    if (source && isWithin(target.step, from)) return impossible()
    // Already linked: the next step, the start of one of its branches, or of a parallel branch after it
    const after = source ? source.block[source.index + 1] : current.steps[0]
    const starts = [after, ...(after?.kind === 'parallel' ? after.branches.map((b) => b[0]) : [])]
    if (source) starts.push(source.step.then[0], source.step.else[0], source.step.steps[0])
    if (starts.some((s) => s?.id === to)) return null
    if (source?.step.kind === 'approval' && source.block.slice(0, source.index).some((s) => s.id === to)) {
      update((d) => void (find(d, from)!.step.onReject = to))
      return null
    }
    update((d) => {
      // Take `to` (with what is inside it) out of where it is; the head of an unlinked chain brings the chain
      const was = find(d, to)!
      const chainHead = was.index === 0 && d.loose.includes(was.block)
      const moved = chainHead ? was.block.splice(0) : was.block.splice(was.index, 1)
      const group = (first: WorkflowStep) => ({ ...emptyStep(freeId(d, 'together', moved.map((m) => m.id)), 'parallel' as const), branches: [[first], moved] })
      if (from === '$trigger') {
        const first = d.steps[0]
        if (!first) d.steps.push(...moved)
        else if (first.kind === 'parallel') first.branches.push(moved)
        else d.steps[0] = group(first)
        return
      }
      const at = find(d, from)!
      const s = at.step
      if (s.kind === 'condition' && s.then.length === 0) return void s.then.push(...moved)
      if (s.kind === 'condition' && s.else.length === 0) return void s.else.push(...moved)
      if (s.kind === 'forEach' && s.steps.length === 0) return void s.steps.push(...moved)
      const next = at.block[at.index + 1]
      if (!next) at.block.push(...moved)
      else if (next.kind === 'parallel') next.branches.push(moved)
      else at.block[at.index + 1] = group(next)
    })
    return null
  }

  /**
   * Removes a link. A rejection link just goes. A parallel branch, or a condition's branch, comes right after
   * the group or the condition instead. Any other link into a step unlinks that step and the ones after it: they
   * wait, unlinked, until they are linked again. Returns why it can't be done, or null.
   */
  const unlink = (link: LinkRef): string | null => {
    if (!current) return null
    update((d) => {
      if (link.type === 'reject') {
        find(d, link.approval)!.step.onReject = null
        return
      }
      if (link.type === 'next') {
        const at = find(d, link.to)
        if (at) d.loose.push(at.block.splice(at.index))
        return
      }
      const id = link.type === 'lane' ? link.group : link.condition
      const at = find(d, id)!
      const steps = link.type === 'lane' ? at.step.branches.splice(link.lane, 1)[0] : at.step[link.branch].splice(0)
      at.block.splice(at.index + 1, 0, ...(steps ?? []))
    })
    return null
  }

  return {
    draft: current,
    dirty,
    discard: () => {
      setDraft(null)
      setHistory({ past: [], future: [] })
    },
    /**
     * Deletes any link. One that stands for something in the steps changes them (see `unlink`); one that only
     * follows from the others is marked deleted, and the draft can't be saved until it is linked again.
     */
    deleteLink: (edge: FlowEdge) => {
      const key = `${edge.from}>${edge.to}`
      if (edge.invalid) update((d) => void (d.extra = d.extra.filter((x) => x !== key)))
      else if (edge.link) return unlink(edge.link)
      else update((d) => void (d.cut = [...d.cut, key]))
      return null
    },
    canUndo: history.past.length > 0,
    canRedo: history.future.length > 0,
    undo: () => {
      if (!history.past.length) return
      lastEdit.current = null
      setHistory((h) => ({ past: h.past.slice(0, -1), future: [draft, ...h.future] }))
      setDraft(history.past[history.past.length - 1])
    },
    redo: () => {
      if (!history.future.length) return
      lastEdit.current = null
      setHistory((h) => ({ past: [...h.past, draft], future: h.future.slice(1) }))
      setDraft(history.future[0])
    },
    connect,
    unlink,
    /** A node placed by hand, in the graph's coordinates; kept in the workflow file */
    place: (id: string, p: Point) => update((d) => void (d.layout = { ...d.layout, [id]: { x: Math.round(p.x / 10) * 10, y: Math.round(p.y / 10) * 10 } })),
    /** Back to the automatic layout */
    resetLayout: () => update((d) => void (d.layout = {})),
    setField: <K extends keyof WorkflowDraft>(key: K, value: WorkflowDraft[K]) => update((d) => void (d[key] = value), `field:${key}`),
    setStep: (id: string, change: Partial<WorkflowStep>) =>
      update((d) => {
        const step = find(d, id)?.step
        if (!step) return
        const renamed = change.id !== undefined && change.id !== id ? change.id : null
        Object.assign(step, change)
        if (!renamed) return
        if (d.layout[id]) {
          d.layout = { ...d.layout, [renamed]: d.layout[id] }
          delete d.layout[id]
        }
        for (const s of everyStep(d)) {
          s.input = renameIn(s.input, id, renamed)
          s.instructions = renameIn(s.instructions, id, renamed)
          s.if = renameIn(s.if, id, renamed)
          s.forEach = renameIn(s.forEach, id, renamed)
          if (s.action) s.action = { ...s.action, text: renameIn(s.action.text, id, renamed) ?? '', to: renameIn(s.action.to, id, renamed) }
          if (s.onReject === id) s.onReject = renamed
        }
      }, change.id === undefined ? `step:${id}:${Object.keys(change).join(',')}` : undefined),
    /** Adds a node of `kind` in `slot`, set up from what it can read there; returns its name */
    addStep: (kind: StepKind, slot: Slot, agent: string | null) => {
      if (!current) return null
      const id = freeId(current, BASE[kind])
      update((d) => {
        const block = blockOf(d, slot)
        if (!block) return
        const step = emptyStep(id, kind)
        const previous = block[slot.index - 1]
        const work = () => ({ ...emptyStep(freeId(d, 'work', [id]), agent ? 'agent' : 'approval'), agent })
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
        const place = find(d, id)
        if (!place) return
        const j = place.index + by
        if (j < 0 || j >= place.block.length) return
        ;[place.block[place.index], place.block[j]] = [place.block[j], place.block[place.index]]
      }),
    /** Removes a node, with what is inside it */
    removeStep: (id: string) =>
      update((d) => {
        const place = find(d, id)
        place?.block.splice(place.index, 1)
        if (d.layout[id]) {
          d.layout = { ...d.layout }
          delete d.layout[id]
        }
      }),
  }
}

export type WorkflowEditing = ReturnType<typeof useWorkflowDraft>
