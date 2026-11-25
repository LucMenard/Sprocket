import { StateDot } from '@/features/runs/StateBadge'
import { KINDS, LEFT, NODE, type Flow, type FlowEdge, type FlowKind, type FlowNode } from '@/lib/flow'

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

/** The path of an edge, its arrowhead, and where its label goes. */
function geometry(e: FlowEdge, a: FlowNode, b: FlowNode, W: number, H: number, backIndex: number) {
  const head = (x: number, y: number, dir: 'down' | 'left' | 'right') =>
    dir === 'down'
      ? `M${x - 5} ${y - 9} L${x + 5} ${y - 9} L${x} ${y - 1} Z`
      : dir === 'left'
        ? `M${x + 9} ${y - 5} L${x + 9} ${y + 5} L${x + 1} ${y} Z`
        : `M${x - 9} ${y - 5} L${x - 9} ${y + 5} L${x - 1} ${y} Z`
  switch (e.mode) {
    case 'side': {
      const sx = a.x + W, sy = a.y + H / 2, tx = b.x + W / 2, ty = b.y
      return { d: `M${sx} ${sy} C${tx} ${sy},${tx} ${sy},${tx} ${ty - 8}`, arrow: head(tx, ty, 'down'), label: { left: (sx + tx) / 2, top: sy - 12 } }
    }
    case 'right': {
      const sx = a.x + W, sy = a.y + 22, tx = b.x, ty = b.y + 22
      return { d: `M${sx} ${sy} L${tx - 8} ${ty}`, arrow: head(tx, ty, 'right'), label: { left: (sx + tx) / 2, top: sy - 14 } }
    }
    case 'return': {
      const sx = a.x, sy = a.y + H - 22, tx = b.x + W, ty = b.y + H - 22
      const d = sy === ty ? `M${sx} ${sy} L${tx + 8} ${ty}` : `M${sx} ${sy} C${sx - 60} ${sy},${tx + 60} ${ty},${tx + 8} ${ty}`
      return { d, arrow: head(tx, ty, 'left'), label: { left: (sx + tx) / 2, top: Math.max(sy, ty) + 14 } }
    }
    case 'back': {
      if (a.x > LEFT) {
        // In a branch or loop column: round the right side, clear of the edges coming in on the left
        const sx = a.x + W, sy = a.y + H / 2, tx = b.x + W, ty = b.y + H / 2
        const bend = Math.max(sx, tx) + 50 + backIndex * 10
        return { d: `M${sx} ${sy} C${bend} ${sy},${bend} ${ty},${tx + 8} ${ty}`, arrow: head(tx, ty, 'left'), label: { left: bend - 12, top: (sy + ty) / 2 } }
      }
      const sx = a.x, sy = a.y + H / 2, tx = b.x, ty = b.y + H / 2
      const bend = Math.min(sx, tx) - 90 - backIndex * 24
      return { d: `M${sx} ${sy} C${bend} ${sy},${bend} ${ty},${tx - 8} ${ty}`, arrow: head(tx, ty, 'right'), label: { left: bend + 20, top: (sy + ty) / 2 } }
    }
    case 'bypass': {
      // Round the branch on the right, then into the next node from above
      const sx = a.x + W, sy = a.y + H / 2, tx = b.x + W / 2 + 16, ty = b.y, bx = e.at ?? sx + 40, r = 10, my = ty - 26
      const d = `M${sx} ${sy} L${bx - r} ${sy} Q${bx} ${sy} ${bx} ${sy + r} L${bx} ${my - r} Q${bx} ${my} ${bx - r} ${my} L${tx + r} ${my} Q${tx} ${my} ${tx} ${my + r} L${tx} ${ty - 8}`
      return { d, arrow: head(tx, ty, 'down'), label: { left: bx, top: (sy + my) / 2 } }
    }
    default: {
      const sx = a.x + W / 2, sy = a.y + H, tx = b.x + W / 2, ty = b.y, my = (sy + ty) / 2
      return { d: `M${sx} ${sy} C${sx} ${my},${tx} ${my},${tx} ${ty - 8}`, arrow: head(tx, ty, 'down'), label: { left: (sx + tx) / 2 + 34, top: my } }
    }
  }
}

/**
 * A workflow as the canvas draws it. `run` mode is the Runs screen: each node shows its state, and arrows
 * along the path taken are green. `design` mode is the workflow editor: kind icons, connection ports, and
 * the selected node's links in the accent colour.
 */
export function FlowGraph({ flow, selected, onSelect, mode }: { flow: Flow; selected?: string; onSelect?: (id: string) => void; mode: 'run' | 'design' }) {
  const { W, H } = NODE[mode]
  const byId = new Map(flow.nodes.map((n) => [n.id, n]))
  const backs = flow.edges.filter((e) => e.mode === 'back')
  const dashed = (e: FlowEdge) => e.mode === 'back' || e.mode === 'return'

  const look = (e: FlowEdge) => {
    if (mode === 'run') {
      const from = byId.get(e.from)
      const taken = e.mode !== 'back' && reached(from?.state) && reached(byId.get(e.to)?.state) && (!e.via || from?.took === e.via)
      return { color: taken ? 'var(--ct-done)' : 'var(--ct-edge)', width: taken ? 2.2 : 1.5, hot: false }
    }
    const hot = e.from === selected || e.to === selected
    return { color: hot ? 'var(--ct-accent)' : 'var(--ct-faint)', width: hot ? 2.2 : 1.6, hot }
  }
  const drawn = flow.edges
    .map((e, i) => {
      const a = byId.get(e.from)
      const b = byId.get(e.to)
      return a && b ? { e, key: `${e.from}-${e.to}-${e.mode}-${i}`, ...geometry(e, a, b, W, H, backs.indexOf(e)), ...look(e) } : null
    })
    .filter((d) => d !== null)

  return (
    <div className="relative mx-auto" style={{ width: flow.width, height: flow.height }}>
      {flow.frames.map((f) => {
        const hot = selected === f.id
        return (
          <div
            key={f.id}
            aria-hidden
            className="pointer-events-none absolute rounded-[14px] border-[1.5px] border-dashed"
            style={{ left: f.x, top: f.y, width: f.w, height: f.h, background: 'var(--ct-loop-frame)', borderColor: hot ? 'var(--loop-fg)' : 'var(--ct-loop-border)' }}
          >
            <span className="absolute top-[9px] left-3 text-xs font-semibold whitespace-nowrap" style={{ color: 'var(--loop-fg)' }}>
              {f.label}
            </span>
          </div>
        )
      })}

      <svg className="absolute inset-0 overflow-visible" width={flow.width} height={flow.height} aria-hidden>
        {drawn.map((d) => (
          <g key={d.key}>
            <path d={d.d} fill="none" stroke={d.color} strokeWidth={d.width} strokeDasharray={dashed(d.e) ? '6 5' : undefined} />
            <path d={d.arrow} fill={d.color} />
          </g>
        ))}
      </svg>

      {drawn
        .filter((d) => d.e.label)
        .map((d) => (
          <span
            key={`${d.key}-label`}
            className="pointer-events-none absolute -translate-x-1/2 -translate-y-1/2 rounded-[10px] border px-2 py-0.5 text-[11px] font-semibold whitespace-nowrap"
            style={{
              ...d.label,
              background: 'var(--ct-surface)',
              borderColor: d.hot ? 'var(--ct-accent-ring)' : 'var(--ct-border)',
              color: d.hot ? 'var(--ct-accent-text)' : 'var(--ct-text-2)',
            }}
          >
            {d.e.label}
          </span>
        ))}

      {flow.nodes.map((n) => {
        const isSelected = n.id === selected
        const style: React.CSSProperties = { left: n.x, top: n.y, width: W, height: H, background: 'var(--ct-surface)' }
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
              <span className="rounded-lg px-1.5 py-px text-[11px] font-semibold whitespace-nowrap" style={{ background: 'var(--ct-pill-wait-bg)', color: 'var(--ct-pill-wait)' }}>
                {n.badge}
              </span>
            )}
          </button>
        )
      })}
    </div>
  )
}

