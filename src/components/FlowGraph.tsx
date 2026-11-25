import { StateDot } from '@/features/runs/StateBadge'
import type { JobState, RunState } from '@/lib/api'

export type FlowKind = 'trigger' | 'agent' | 'approval' | 'end'

export interface FlowNode {
  id: string
  kind: FlowKind
  title: string
  sub?: string
  /** In a run: the state of the step's latest job; 'pending' when it hasn't started */
  state?: JobState | RunState | 'pending'
  badge?: string
}

/** Edges go from a node to the next one; `back` edges return to an earlier node (a rejection). */
export interface FlowEdge {
  from: string
  to: string
  label?: string
  back?: boolean
}

/** The kinds of node, with the icon and colours of the mockups' workflow editor. */
export const KINDS: Record<FlowKind, { label: string; icon: string }> = {
  trigger: { label: 'Trigger', icon: 'M13 3 5 14h6l-1 7 8-11h-6z' },
  agent: { label: 'Agent step', icon: 'M5 9h14v10H5zM12 5v4M9 14h.01M15 14h.01' },
  approval: { label: 'Human approval', icon: 'M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6zM9 12l2 2 4-4' },
  end: { label: 'End', icon: 'M5 21V4M5 4h11l-2 4 2 4H5' },
}

/** The coloured square with the node kind's icon. */
export function KindIcon({ kind, size = 36 }: { kind: FlowKind; size?: number }) {
  return (
    <span
      aria-hidden
      className="flex shrink-0 items-center justify-center rounded-lg"
      style={{ width: size, height: size, background: `var(--ct-kind-${kind}-bg)`, color: `var(--ct-kind-${kind})` }}
    >
      <svg width={size / 2} height={size / 2} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round">
        <path d={KINDS[kind].icon} />
      </svg>
    </span>
  )
}

const reached = (state: FlowNode['state']) => !!state && state !== 'pending'

/**
 * Steps top to bottom, joined by curved arrows, as the mockups draw them. `run` mode is the Runs screen:
 * each node shows its state, and arrows along the path taken are green. `design` mode is the workflow
 * editor: kind icons, connection ports, and the selected node's links in the accent colour.
 */
export function FlowGraph({
  nodes,
  edges,
  selected,
  onSelect,
  mode,
}: {
  nodes: FlowNode[]
  edges: FlowEdge[]
  selected?: string
  onSelect?: (id: string) => void
  mode: 'run' | 'design'
}) {
  const W = mode === 'run' ? 232 : 240
  const H = mode === 'run' ? 64 : 76
  const GAP = 64
  const LEFT = 150 // room on the left for the arrows going back
  const TOP = 24
  const index = new Map(nodes.map((n, i) => [n.id, i]))
  const y = (id: string) => TOP + (index.get(id) ?? 0) * (H + GAP)
  const width = LEFT + W + 60
  const height = TOP * 2 + nodes.length * H + (nodes.length - 1) * GAP
  const cx = LEFT + W / 2
  const backs = edges.filter((e) => e.back)
  const byId = new Map(nodes.map((n) => [n.id, n]))

  const look = (e: FlowEdge) => {
    if (mode === 'run') {
      const taken = !e.back && reached(byId.get(e.from)?.state) && reached(byId.get(e.to)?.state)
      return { color: taken ? 'var(--ct-done)' : 'var(--ct-edge)', width: taken ? 2.2 : 1.5, hot: false }
    }
    const hot = e.from === selected || e.to === selected
    return { color: hot ? 'var(--ct-accent)' : 'var(--ct-faint)', width: hot ? 2.2 : 1.6, hot }
  }

  return (
    <div className="relative mx-auto" style={{ width, height }}>
      <svg className="absolute inset-0 overflow-visible" width={width} height={height} aria-hidden>
        {edges.map((e) => {
          const { color, width: w } = look(e)
          if (!e.back) {
            const sy = y(e.from) + H
            const ty = y(e.to)
            const my = (sy + ty) / 2
            return (
              <g key={`${e.from}-${e.to}`}>
                <path d={`M${cx} ${sy} C${cx} ${my},${cx} ${my},${cx} ${ty - 8}`} fill="none" stroke={color} strokeWidth={w} />
                <path d={`M${cx - 5} ${ty - 9} L${cx + 5} ${ty - 9} L${cx} ${ty - 1} Z`} fill={color} />
              </g>
            )
          }
          // A rejection loops back on the left, dashed, as the mockup's loop edges
          const sy = y(e.from) + H / 2
          const ty = y(e.to) + H / 2
          const bend = LEFT - 90 - backs.indexOf(e) * 24
          return (
            <g key={`${e.from}-${e.to}-back`}>
              <path d={`M${LEFT} ${sy} C${bend} ${sy},${bend} ${ty},${LEFT - 8} ${ty}`} fill="none" stroke={color} strokeWidth={w} strokeDasharray="6 5" />
              <path d={`M${LEFT - 9} ${ty - 5} L${LEFT - 9} ${ty + 5} L${LEFT - 1} ${ty} Z`} fill={color} />
            </g>
          )
        })}
      </svg>

      {edges
        .filter((e) => e.label)
        .map((e) => {
          const { hot } = look(e)
          const pos = e.back
            ? { left: LEFT - 70 - backs.indexOf(e) * 24, top: (y(e.from) + y(e.to) + H) / 2 }
            : { left: cx + 34, top: y(e.from) + H + GAP / 2 }
          return (
            <span
              key={`${e.from}-${e.to}-label`}
              className="pointer-events-none absolute -translate-x-1/2 -translate-y-1/2 rounded-[10px] border px-2 py-0.5 text-[11px] font-semibold whitespace-nowrap"
              style={{
                ...pos,
                background: 'var(--ct-surface)',
                borderColor: hot ? 'var(--ct-accent-ring)' : 'var(--ct-border)',
                color: hot ? 'var(--ct-accent-text)' : 'var(--ct-text-2)',
              }}
            >
              {e.label}
            </span>
          )
        })}

      {nodes.map((n) => {
        const isSelected = n.id === selected
        const style: React.CSSProperties = {
          left: LEFT,
          top: y(n.id),
          width: W,
          height: H,
          background: 'var(--ct-surface)',
        }
        if (mode === 'run') {
          style.border = n.state === 'running' ? '1.5px solid var(--ct-accent)' : n.state === 'failed' ? '1px solid var(--ct-danger-border)' : '1px solid var(--ct-control)'
          if (isSelected) style.boxShadow = '0 0 0 4px var(--ct-focus)'
        } else {
          style.border = isSelected ? '1.5px solid var(--ct-accent)' : '1px solid var(--ct-control)'
          style.boxShadow = isSelected ? '0 0 0 4px var(--ct-focus), 0 4px 12px rgba(17,24,39,0.08)' : 'var(--ct-shadow)'
        }
        return (
          <button
            key={n.id}
            type="button"
            aria-pressed={isSelected}
            aria-label={`${KINDS[n.kind].label}: ${n.title}`}
            onClick={() => onSelect?.(n.id)}
            className="absolute flex items-center gap-3 rounded-xl px-3 text-left"
            style={style}
          >
            {mode === 'design' && (
              <>
                <span aria-hidden className="absolute -top-1.5 left-1/2 -ml-1.5 size-2.5 rounded-full border-[1.5px]" style={{ background: 'var(--ct-surface)', borderColor: 'var(--ct-faint)' }} />
                {n.kind !== 'end' && (
                  <span aria-hidden className="absolute -bottom-1.5 left-1/2 -ml-1.5 size-2.5 rounded-full border-[1.5px]" style={{ background: 'var(--ct-surface)', borderColor: 'var(--ct-faint)' }} />
                )}
              </>
            )}
            {mode === 'run' ? <StateDot state={n.state ?? 'pending'} size={24} /> : <KindIcon kind={n.kind} />}
            <span className="flex min-w-0 flex-1 flex-col gap-0.5">
              {mode === 'design' && <span className="text-[11px] font-semibold tracking-[0.05em] text-ct-text-3 uppercase">{KINDS[n.kind].label}</span>}
              <span className={mode === 'run' ? 'truncate text-[13px] font-semibold' : 'truncate font-semibold'} style={n.state === 'pending' ? { color: 'var(--ct-text-3)' } : undefined}>
                {n.title}
              </span>
              {n.sub && <span className="truncate text-xs text-ct-text-3">{n.sub}</span>}
            </span>
            {n.badge && (
              <span className="rounded-lg px-1.5 py-px text-[11px] font-semibold" style={{ background: 'var(--ct-pill-wait-bg)', color: 'var(--ct-pill-wait)' }}>
                {n.badge}
              </span>
            )}
          </button>
        )
      })}
    </div>
  )
}

/** The nodes and edges of a linear workflow: trigger, its steps, end, and the arrows back of `onReject`. */
export function workflowFlow(
  steps: { id: string; kind: 'agent' | 'approval'; agent: string | null; onReject: string | null }[],
  trigger: string,
): { nodes: FlowNode[]; edges: FlowEdge[] } {
  const nodes: FlowNode[] = [
    { id: '$trigger', kind: 'trigger', title: trigger, sub: 'Manual, from a ticket' },
    ...steps.map((s) => ({
      id: s.id,
      kind: s.kind,
      title: s.id,
      sub: s.kind === 'agent' ? `Agent: ${s.agent}` : 'Waits for your approval',
    })),
    { id: '$end', kind: 'end', title: 'Done', sub: 'Mark the run as passed' },
  ]
  const edges: FlowEdge[] = []
  for (let i = 0; i + 1 < nodes.length; i++) {
    edges.push({ from: nodes[i].id, to: nodes[i + 1].id, label: nodes[i].kind === 'approval' ? 'approved' : undefined })
  }
  for (const s of steps) if (s.onReject) edges.push({ from: s.id, to: s.onReject, label: 'rejected', back: true })
  return { nodes, edges }
}
