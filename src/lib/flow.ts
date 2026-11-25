import type { JobState, RunState, WorkflowStep } from './api'

export type FlowKind = 'trigger' | 'agent' | 'approval' | 'action' | 'condition' | 'forEach' | 'end'

export interface FlowNode {
  id: string
  kind: FlowKind
  title: string
  sub?: string
  /** In a run: the state of the step's latest job; 'pending' when it hasn't started */
  state?: JobState | RunState | 'pending'
  badge?: string
  /** In a run, for a condition: the branch it took */
  took?: 'then' | 'else'
  x: number
  y: number
}

/**
 * How an edge is drawn, as in the canvas: `down` to the next node, `side` from a condition to its other
 * branch, `right` into a loop's body and `return` back to the loop, `back` for a rejection, `bypass` round
 * a branch for a condition's empty side.
 */
export type EdgeMode = 'down' | 'side' | 'right' | 'return' | 'back' | 'bypass'

export interface FlowEdge {
  from: string
  to: string
  label?: string
  mode: EdgeMode
  /** For `bypass`: the x the edge goes down at */
  at?: number
  /** Leaves a condition by this branch: taken only when the condition took it */
  via?: 'then' | 'else'
}

/** The dashed frame round a loop's steps. */
export interface FlowFrame {
  id: string
  label: string
  x: number
  y: number
  w: number
  h: number
}

export interface Flow {
  nodes: FlowNode[]
  edges: FlowEdge[]
  frames: FlowFrame[]
  width: number
  height: number
}

/** The kinds of node, with the canvas's labels and icons. Colours are `--ct-kind-<kind>`. */
export const KINDS: Record<FlowKind, { label: string; icon: string; help: string }> = {
  trigger: { label: 'Trigger', icon: 'M13 3 5 14h6l-1 7 8-11h-6z', help: 'What starts the workflow' },
  agent: { label: 'Agent step', icon: 'M5 9h14v10H5zM12 5v4M9 14h.01M15 14h.01', help: 'Hand work to an agent type' },
  condition: { label: 'Condition', icon: 'M6 4v6a4 4 0 0 0 4 4h8M18 14l-3-3M18 14l-3 3M6 20v-6', help: 'Branch on a result' },
  forEach: { label: 'For each', icon: 'M17 2l3 3-3 3M4 11V9a4 4 0 0 1 4-4h12M7 22l-3-3 3-3M20 13v2a4 4 0 0 1-4 4H4', help: 'Repeat steps for every item' },
  approval: { label: 'Human approval', icon: 'M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6zM9 12l2 2 4-4', help: 'Pause until someone approves' },
  action: { label: 'Action', icon: 'M4 5h16v11H9l-5 4zM8 10h8M8 13h5', help: 'Post, comment or update a ticket' },
  end: { label: 'End', icon: 'M5 21V4M5 4h11l-2 4 2 4H5', help: 'Finish the run' },
}

export const NODE = { run: { W: 232, H: 64 }, design: { W: 240, H: 76 } }
const GAP = 64 // between a node and the next one
const COLUMN = 150 // between a column and the one on its right: room for the arrows going back
/** Where the workflow's own column starts; columns further right are branches and loop bodies */
export const LEFT = 150
const TOP = 48

type Exit = Omit<FlowEdge, 'to' | 'mode'> & { mode?: EdgeMode }

/**
 * Lays a workflow out as the canvas draws it: steps top to bottom; a condition's `yes` below it and its
 * `no` in a column on the right; a loop's steps to its right in a dashed frame; trigger first, end last.
 * `describe` gives each step's title, subtitle and, in a run, its state.
 */
export function workflowFlow(
  steps: WorkflowStep[],
  mode: 'run' | 'design',
  describe: (s: WorkflowStep) => Partial<FlowNode>,
  ends: { trigger: Partial<FlowNode>; end: Partial<FlowNode> },
): Flow {
  const { W, H } = NODE[mode]
  const nodes: FlowNode[] = []
  const edges: FlowEdge[] = []
  const frames: FlowFrame[] = []
  let right = LEFT + W

  const connect = (exits: Exit[], to: string, force?: EdgeMode) =>
    exits.forEach((e, i) => edges.push({ ...e, to, mode: force ?? e.mode ?? 'down', label: force === 'return' ? (i === 0 ? 'next' : undefined) : e.label }))

  interface Laid {
    exits: Exit[]
    bottom: number
    right: number
  }

  const block = (list: WorkflowStep[], x: number, y: number, incoming: Exit[]): Laid => {
    let exits = incoming
    let top = y
    let bottom = y - GAP
    let r = x + W
    for (const s of list) {
      connect(exits, s.id)
      const laid = node(s, x, top)
      exits = laid.exits
      bottom = laid.bottom
      top = laid.bottom + GAP
      r = Math.max(r, laid.right)
    }
    return { exits, bottom, right: r }
  }

  const node = (s: WorkflowStep, x: number, y: number): Laid => {
    nodes.push({ id: s.id, kind: s.kind, title: s.id, x, y, ...describe(s) })
    right = Math.max(right, x + W)
    if (s.kind === 'condition') {
      const yes = block(s.then, x, y + H + GAP, [{ from: s.id, label: 'yes', via: 'then' }])
      if (s.else.length) {
        const no = block(s.else, yes.right + COLUMN - 40, y + H + GAP, [{ from: s.id, label: 'no', mode: 'side', via: 'else' }])
        return { exits: [...yes.exits, ...no.exits], bottom: Math.max(yes.bottom, no.bottom), right: no.right }
      }
      const at = yes.right + 40
      return { exits: [...yes.exits, { from: s.id, label: 'no', mode: 'bypass', at, via: 'else' }], bottom: yes.bottom, right: at + 30 }
    }
    if (s.kind === 'forEach') {
      const bx = x + W + COLUMN
      const body = block(s.steps, bx, y, [{ from: s.id, label: `each ${s.as || 'item'}`, mode: 'right' }])
      connect(body.exits, s.id, 'return')
      const frame = { id: s.id, label: `Loop body · ${s.id}`, x: bx - 24, y: y - 36, w: body.right - bx + 48, h: body.bottom - y + 36 + 20 }
      frames.push(frame)
      right = Math.max(right, frame.x + frame.w)
      return { exits: [{ from: s.id, label: 'all done' }], bottom: Math.max(y + H, body.bottom + 20), right: frame.x + frame.w }
    }
    if (s.kind === 'approval' && s.onReject) edges.push({ from: s.id, to: s.onReject, label: 'rejected', mode: 'back' })
    // In a branch or loop column the arrow back goes round the right side: keep room for it
    const back = s.kind === 'approval' && s.onReject && x > LEFT
    return { exits: [{ from: s.id, label: s.kind === 'approval' ? 'approved' : undefined }], bottom: y + H, right: x + W + (back ? 70 : 0) }
  }

  nodes.push({ id: '$trigger', kind: 'trigger', title: 'Tracket', x: LEFT, y: TOP, ...ends.trigger })
  const laid = block(steps, LEFT, TOP + H + GAP, [{ from: '$trigger' }])
  const endY = laid.bottom + GAP
  connect(laid.exits, '$end')
  nodes.push({ id: '$end', kind: 'end', title: 'Done', x: LEFT, y: endY, ...ends.end })
  return { nodes, edges, frames, width: right + 60, height: endY + H + TOP }
}

/** What a step's node says under its name, in the editor. */
export function designSub(s: WorkflowStep): string {
  switch (s.kind) {
    case 'agent':
      return `Agent: ${s.agent}${s.output.length ? ' · JSON' : ''}`
    case 'approval':
      return 'Waits for your approval'
    case 'action': {
      const a = s.action
      const what = a?.do === 'status' ? `Status → ${a.to ?? ''}` : a?.do === 'slack' ? `Slack ${a.to ?? ''}` : a?.do === 'other' ? 'Something else' : 'Comment on the ticket'
      return `${what} · ${s.agent ?? ''}`
    }
    case 'condition':
      return s.if ?? ''
    case 'forEach':
      return `${s.forEach ?? ''} · ${s.parallel > 1 ? `${s.parallel} in parallel` : 'one at a time'}`
  }
}
