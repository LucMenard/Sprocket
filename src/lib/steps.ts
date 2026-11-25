import type { Job, JobState, Run, StepKind, WorkflowStep } from './api'

/** The steps inside a step: a condition's branches, a loop's steps. */
export const children = (s: WorkflowStep) => [...s.then, ...s.else, ...s.steps]

/** Every step, nested ones included, in the order they are written. */
export function allSteps(steps: WorkflowStep[]): WorkflowStep[] {
  return steps.flatMap((s) => [s, ...allSteps(children(s))])
}

export type Branch = 'then' | 'else' | 'steps'

/** Where a step is: the block it is in (by its parent and branch), its place there, the loops around it. */
export interface Place {
  step: WorkflowStep
  block: WorkflowStep[]
  index: number
  parent: WorkflowStep | null
  branch: Branch | null
  loops: WorkflowStep[]
}

export function findPlace(steps: WorkflowStep[], id: string, parent: WorkflowStep | null = null, branch: Branch | null = null, loops: WorkflowStep[] = []): Place | null {
  for (const [index, step] of steps.entries()) {
    if (step.id === id) return { step, block: steps, index, parent, branch, loops }
    const inner =
      findPlace(step.then, id, step, 'then', loops) ??
      findPlace(step.else, id, step, 'else', loops) ??
      findPlace(step.steps, id, step, 'steps', [...loops, step])
    if (inner) return inner
  }
  return null
}

/**
 * What a step can read at its place, as the Rust side checks it: earlier steps (a condition's branches once
 * it is behind; a loop's steps only from inside it), and the items of the loops around it.
 */
export function readable(steps: WorkflowStep[], id: string): { steps: WorkflowStep[]; items: string[] } {
  type Vis = { steps: WorkflowStep[]; items: string[] }
  let found: Vis | null = null
  const walk = (block: WorkflowStep[], vis: Vis): Vis => {
    for (const s of block) {
      if (found) return vis
      if (s.id === id) {
        found = { steps: [...vis.steps], items: [...vis.items] }
        return vis
      }
      if (s.kind === 'condition') {
        const t = walk(s.then, { steps: [...vis.steps], items: vis.items })
        const e = walk(s.else, { steps: [...vis.steps], items: vis.items })
        vis = { steps: [...new Set([...t.steps, ...e.steps])], items: vis.items }
      } else if (s.kind === 'forEach') {
        walk(s.steps, { steps: [...vis.steps], items: [...vis.items, s.as || 'item'] })
        vis = { ...vis, steps: [...vis.steps, s] }
      } else {
        vis = { ...vis, steps: [...vis.steps, s] }
      }
    }
    return vis
  }
  walk(steps, { steps: [], items: [] })
  return found ?? { steps: [], items: [] }
}

export function emptyStep(id: string, kind: StepKind): WorkflowStep {
  return {
    id,
    kind,
    agent: null,
    instructions: null,
    input: null,
    onReject: null,
    output: [],
    action: null,
    if: null,
    then: [],
    else: [],
    forEach: null,
    as: null,
    parallel: 1,
    onItemFailure: 'stop',
    steps: [],
  }
}

/** `each[2]/files[0]/` → the item numbers, counted from 1: `each 3 · files 1` */
export function scopeLabel(scope: string) {
  return scope
    .split('/')
    .filter(Boolean)
    .map((p) => p.replace(/^(.+)\[(\d+)\]$/, (_, id: string, n: string) => `${id} ${Number(n) + 1}`))
    .join(' · ')
}

/** The jobs of a step that count (not redone since), in every place it ran. */
export function currentJobs(run: Run, stepId: string): Job[] {
  return run.jobs.filter((j) => j.stepId === stepId && !j.superseded)
}

const BY_URGENCY: JobState[] = ['waiting', 'running', 'failed', 'interrupted', 'rejected', 'stopped', 'done']

/** A step's state in a run: of its job, or, for a step done once per loop item, the most urgent of theirs. */
export function stepState(run: Run, stepId: string): JobState | 'pending' {
  const jobs = currentJobs(run, stepId)
  if (!jobs.length) return 'pending'
  return BY_URGENCY.find((state) => jobs.some((j) => j.state === state)) ?? jobs[jobs.length - 1].state
}

/** The agent type of a step, wherever it is in the workflow. */
export const agentOf = (steps: WorkflowStep[], stepId: string) => findPlace(steps, stepId)?.step.agent ?? null
