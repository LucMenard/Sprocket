import { LayoutGrid, Maximize, Minus, Plus } from 'lucide-react'
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { StateDot } from '@/features/runs/StateBadge'
import { edgeKey, KINDS, LEFT, NODE, type Flow, type FlowEdge, type FlowKind, type FlowNode } from '@/lib/flow'

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
      // Clear of both nodes, also when one was moved by hand
      const sx = a.x + W, sy = a.y + H / 2, tx = b.x + W / 2 + 16, ty = b.y, bx = Math.max(e.at ?? 0, sx + 30, b.x + W + 30), r = 10, my = ty - 26
      const d = `M${sx} ${sy} L${bx - r} ${sy} Q${bx} ${sy} ${bx} ${sy + r} L${bx} ${my - r} Q${bx} ${my} ${bx - r} ${my} L${tx + r} ${my} Q${tx} ${my} ${tx} ${my + r} L${tx} ${ty - 8}`
      return { d, arrow: head(tx, ty, 'down'), label: { left: bx, top: (sy + my) / 2 } }
    }
    default: {
      const sx = a.x + W / 2, sy = a.y + H, tx = b.x + W / 2, ty = b.y, my = (sy + ty) / 2
      return { d: `M${sx} ${sy} C${sx} ${my},${tx} ${my},${tx} ${ty - 8}`, arrow: head(tx, ty, 'down'), label: { left: (sx + tx) / 2 + 34, top: my } }
    }
  }
}

const MIN_ZOOM = 0.4
const MAX_ZOOM = 1.6
const clampZoom = (z: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.round(z * 100) / 100))

function ZoomButton({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button type="button" aria-label={label} title={label} onClick={onClick} className="flex size-9 items-center justify-center rounded-md hover:bg-ct-subtle">
      {children}
    </button>
  )
}

/**
 * A workflow as the canvas draws it, in a viewport: drag the background (or scroll) to pan, Ctrl+scroll or the
 * buttons to zoom. `run` mode is the Runs screen: each node shows its state, and arrows along the path taken
 * are green. `design` mode is the workflow editor: kind icons, the selected node's links in the accent colour,
 * and a port under each node to drag onto another one (`onConnect`).
 */
export function FlowGraph({
  flow,
  selected,
  onSelect,
  onConnect,
  onPlace,
  onResetLayout,
  selectedLink,
  onSelectLink,
  mode,
  children,
}: {
  flow: Flow
  selected?: string
  onSelect?: (id: string) => void
  /** `port`: the dot it was dragged from, on a node with several (`FlowNode.ports`); `next` otherwise */
  onConnect?: (from: string, to: string, port: string) => void
  /** A node dropped somewhere: where it now is, in the graph's coordinates */
  onPlace?: (id: string, at: { x: number; y: number }) => void
  /** Puts every node back where the automatic layout has it */
  onResetLayout?: () => void
  /** The selected link (`edgeKey`), and what a click on a link does */
  selectedLink?: string | null
  onSelectLink?: (key: string) => void
  mode: 'run' | 'design'
  /** Drawn over the viewport, bottom left: the editor's validity card */
  children?: React.ReactNode
}) {
  const { W, H } = NODE[mode]
  const byId = new Map(flow.nodes.map((n) => [n.id, n]))
  const backs = flow.edges.filter((e) => e.mode === 'back')
  const dashed = (e: FlowEdge) => e.mode === 'back' || e.mode === 'return' || !!e.invalid
  const viewport = useRef<HTMLDivElement>(null)
  const [view, setView] = useState<{ zoom: number; x: number; y: number } | null>(null)
  const drag = useRef<
    | { kind: 'pan'; sx: number; sy: number; x: number; y: number }
    | { kind: 'link'; from: string; port: string }
    | { kind: 'node'; id: string; sx: number; sy: number; moved: boolean }
    | null
  >(null)
  // A node being dragged: how far it has gone (graph units) and what it is over
  const [moving, setMoving] = useState<{ id: string; dx: number; dy: number; over: string | null } | null>(null)
  // The click that ends a drag doesn't also select
  const dragged = useRef(false)
  const [panning, setPanning] = useState(false)
  const [linking, setLinking] = useState<{ from: string; port: string; x: number; y: number; over: string | null } | null>(null)

  /** The whole graph in view, centred; not above 100% */
  const fit = useCallback(() => {
    const el = viewport.current
    if (!el) return
    const zoom = clampZoom(Math.min(1, (el.clientWidth - 32) / flow.width, (el.clientHeight - 32) / flow.height))
    setView({ zoom, x: Math.max(16, (el.clientWidth - flow.width * zoom) / 2), y: 16 })
  }, [flow.width, flow.height])
  // First look: at 100% when it fits across, centred, from the top. Only once the pane has its size: a pane
  // still being laid out is a few pixels wide, which would shrink the graph to nothing
  useLayoutEffect(() => {
    const el = viewport.current
    if (view || !el) return
    const first = () => {
      if (el.clientWidth < 120) return false
      const zoom = clampZoom(Math.min(1, (el.clientWidth - 32) / flow.width))
      setView({ zoom, x: Math.max(16, (el.clientWidth - flow.width * zoom) / 2), y: 8 })
      return true
    }
    if (first()) return
    const watch = new ResizeObserver(() => first() && watch.disconnect())
    watch.observe(el)
    return () => watch.disconnect()
  }, [view, flow.width])
  const v = view ?? { zoom: 1, x: 0, y: 0 }

  /** Zooms keeping the point under (cx, cy), in viewport pixels, where it is */
  const zoomAt = useCallback((zoom: number, cx: number, cy: number) => {
    setView((old) => {
      const o = old ?? { zoom: 1, x: 0, y: 0 }
      const z = clampZoom(zoom)
      return { zoom: z, x: cx - ((cx - o.x) * z) / o.zoom, y: cy - ((cy - o.y) * z) / o.zoom }
    })
  }, [])
  const zoomBy = (step: number) => {
    const el = viewport.current
    if (el) zoomAt(v.zoom + step, el.clientWidth / 2, el.clientHeight / 2)
  }

  // Scroll pans, Ctrl+scroll zooms; the listener can't be passive, to keep the page from scrolling
  useEffect(() => {
    const el = viewport.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      if (e.ctrlKey || e.metaKey) {
        const r = el.getBoundingClientRect()
        setView((o) => {
          const old = o ?? { zoom: 1, x: 0, y: 0 }
          const z = clampZoom(old.zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1))
          const cx = e.clientX - r.left
          const cy = e.clientY - r.top
          return { zoom: z, x: cx - ((cx - old.x) * z) / old.zoom, y: cy - ((cy - old.y) * z) / old.zoom }
        })
      } else {
        setView((o) => {
          const old = o ?? { zoom: 1, x: 0, y: 0 }
          return { ...old, x: old.x - (e.shiftKey ? e.deltaY : e.deltaX), y: old.y - (e.shiftKey ? 0 : e.deltaY) }
        })
      }
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [])

  /** A pointer position in the graph's own coordinates */
  const toGraph = (clientX: number, clientY: number) => {
    const r = viewport.current!.getBoundingClientRect()
    return { x: (clientX - r.left - v.x) / v.zoom, y: (clientY - r.top - v.y) / v.zoom }
  }
  const nodeAt = (clientX: number, clientY: number) =>
    (document.elementFromPoint(clientX, clientY)?.closest('[data-node-id]') as HTMLElement | null)?.dataset.nodeId ?? null

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0 || (e.target as Element).closest('[data-node-id], [data-edge], button')) return
    e.currentTarget.setPointerCapture(e.pointerId)
    drag.current = { kind: 'pan', sx: e.clientX, sy: e.clientY, x: v.x, y: v.y }
    setPanning(true)
  }
  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current
    if (d?.kind === 'pan') setView({ zoom: v.zoom, x: d.x + e.clientX - d.sx, y: d.y + e.clientY - d.sy })
    if (d?.kind === 'link') {
      const over = nodeAt(e.clientX, e.clientY)
      setLinking({ from: d.from, port: d.port, ...toGraph(e.clientX, e.clientY), over: over !== d.from ? over : null })
    }
    if (d?.kind === 'node') {
      if (!d.moved && Math.hypot(e.clientX - d.sx, e.clientY - d.sy) < 5) return
      // Only now: capturing on press would take the click away from the node
      if (!d.moved) viewport.current?.setPointerCapture(e.pointerId)
      d.moved = true
      setMoving({ id: d.id, dx: (e.clientX - d.sx) / v.zoom, dy: (e.clientY - d.sy) / v.zoom, over: null })
    }
  }
  const onPointerUp = (e: React.PointerEvent) => {
    const d = drag.current
    drag.current = null
    setPanning(false)
    if (d?.kind === 'link') {
      const to = nodeAt(e.clientX, e.clientY)
      setLinking(null)
      if (to && to !== d.from) onConnect?.(d.from, to, d.port)
    }
    if (d?.kind === 'node' && d.moved) {
      dragged.current = true
      setTimeout(() => (dragged.current = false), 0)
      setMoving(null)
      const n = byId.get(d.id)
      if (n) onPlace?.(d.id, { x: n.x + (e.clientX - d.sx) / v.zoom, y: n.y + (e.clientY - d.sy) / v.zoom })
    }
  }
  const startMove = (id: string) => (e: React.PointerEvent) => {
    if (e.button !== 0) return
    drag.current = { kind: 'node', id, sx: e.clientX, sy: e.clientY, moved: false }
  }
  const startLink = (from: string, port = 'next') => (e: React.PointerEvent) => {
    e.stopPropagation()
    e.preventDefault()
    viewport.current?.setPointerCapture(e.pointerId)
    drag.current = { kind: 'link', from, port }
    setLinking({ from, port, ...toGraph(e.clientX, e.clientY), over: null })
  }

  const look = (e: FlowEdge) => {
    if (mode === 'run') {
      const from = byId.get(e.from)
      const taken = e.mode !== 'back' && reached(from?.state) && reached(byId.get(e.to)?.state) && (!e.via || from?.took === e.via)
      return { color: taken ? 'var(--ct-done)' : 'var(--ct-edge)', width: taken ? 2.2 : 1.5, hot: false }
    }
    if (selectedLink === edgeKey(e)) return { color: 'var(--ct-accent)', width: 3, hot: true }
    if (e.invalid) return { color: 'var(--ct-failed)', width: 2, hot: false }
    // With a link selected, it alone stands out
    const hot = !selectedLink && (e.from === selected || e.to === selected)
    return { color: hot ? 'var(--ct-accent)' : 'var(--ct-faint)', width: hot ? 2.2 : 1.6, hot }
  }
  const drawn = flow.edges
    .map((e, i) => {
      const a = byId.get(e.from)
      const b = byId.get(e.to)
      return a && b ? { e, key: `${e.from}-${e.to}-${e.mode}-${i}`, ...geometry(e, a, b, W, H, backs.indexOf(e)), ...look(e) } : null
    })
    .filter((d) => d !== null)
  const linkFrom = linking ? byId.get(linking.from) : undefined
  // The dragged line starts at the dot it was dragged from
  const fromPorts = linkFrom?.ports ?? [{ id: 'next' }]
  const fromX = linkFrom ? linkFrom.x + (W * (Math.max(0, fromPorts.findIndex((p) => p.id === linking?.port)) + 1)) / (fromPorts.length + 1) : 0

  return (
    <div
      ref={viewport}
      className="relative h-full w-full touch-none overflow-hidden select-none"
      style={{ cursor: panning ? 'grabbing' : 'grab' }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => {
        drag.current = null
        setPanning(false)
        setLinking(null)
        setMoving(null)
      }}
    >
      <div className="absolute top-0 left-0 origin-top-left" style={{ width: flow.width, height: flow.height, transform: `translate(${v.x}px, ${v.y}px) scale(${v.zoom})` }}>
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

        <svg className="pointer-events-none absolute inset-0 overflow-visible" width={flow.width} height={flow.height} aria-hidden>
          {drawn.map((d) => (
            <g key={d.key}>
              <path d={d.d} fill="none" stroke={d.color} strokeWidth={d.width} strokeDasharray={dashed(d.e) ? '6 5' : undefined} />
              <path d={d.arrow} fill={d.color} />
              {onSelectLink && (
                // A wider line nobody sees, easier to click than the drawn one
                <path
                  data-edge={edgeKey(d.e)}
                  d={d.d}
                  fill="none"
                  stroke="transparent"
                  strokeWidth={14}
                  style={{ pointerEvents: 'stroke', cursor: 'pointer' }}
                  onClick={() => onSelectLink(edgeKey(d.e))}
                />
              )}
            </g>
          ))}
          {linking && linkFrom && (
            <path
              d={`M${fromX} ${linkFrom.y + H} C${fromX} ${linkFrom.y + H + 60},${linking.x} ${linking.y - 60},${linking.x} ${linking.y}`}
              fill="none"
              stroke="var(--ct-accent)"
              strokeWidth={2}
              strokeDasharray="6 5"
            />
          )}
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
          const isTarget = linking?.over === n.id || moving?.over === n.id
          const isMoving = moving?.id === n.id
          const style: React.CSSProperties = { left: n.x, top: n.y, width: W, height: H }
          if (isMoving) Object.assign(style, { transform: `translate(${moving.dx}px, ${moving.dy}px)`, zIndex: 10 })
          const box: React.CSSProperties = { background: 'var(--ct-surface)' }
          if (mode === 'run') {
            box.border = n.state === 'running' ? '1.5px solid var(--ct-accent)' : n.state === 'failed' ? '1px solid var(--ct-danger-border)' : '1px solid var(--ct-control)'
            if (isSelected) box.boxShadow = '0 0 0 4px var(--ct-focus)'
          } else {
            box.border = isSelected || isTarget ? '1.5px solid var(--ct-accent)' : n.loose ? '1.5px dashed var(--ct-faint)' : '1px solid var(--ct-control)'
            box.boxShadow = isSelected || isTarget ? '0 0 0 4px var(--ct-focus), 0 4px 12px rgba(17,24,39,0.08)' : 'var(--ct-shadow)'
          }
          return (
            <div key={n.id} data-node-id={n.id} className="absolute" style={style}>
              <button
                type="button"
                aria-pressed={isSelected}
                aria-label={`${KINDS[n.kind].label}: ${n.title}`}
                onClick={() => {
                  if (dragged.current) return
                  onSelect?.(n.id)
                }}
                onPointerDown={mode === 'design' && onPlace ? startMove(n.id) : undefined}
                className="flex size-full cursor-pointer items-center gap-3 rounded-xl px-3 text-left"
                style={box}
              >
                {mode === 'run' ? <StateDot state={n.state ?? 'pending'} size={24} /> : <KindIcon kind={n.kind} />}
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  {mode === 'design' && <span className="text-[11px] font-semibold tracking-[0.05em] text-ct-text-3 uppercase">{KINDS[n.kind].label}</span>}
                  <span className={mode === 'run' ? 'truncate text-[13px] font-semibold' : 'truncate font-semibold'} style={n.state === 'pending' ? { color: 'var(--ct-text-3)' } : undefined}>
                    {n.title}
                  </span>
                  {n.loose ? (
                    <span className="truncate text-xs font-semibold" style={{ color: 'var(--ct-pill-wait)' }}>
                      Not linked · drag a dot onto it
                    </span>
                  ) : (
                    n.sub && <span className="truncate text-xs text-ct-text-3">{n.sub}</span>
                  )}
                </span>
                {n.badge && (
                  <span className="rounded-lg px-1.5 py-px text-[11px] font-semibold whitespace-nowrap" style={{ background: 'var(--ct-pill-wait-bg)', color: 'var(--ct-pill-wait)' }}>
                    {n.badge}
                  </span>
                )}
              </button>
              {mode === 'design' && (
                <>
                  <span aria-hidden className="pointer-events-none absolute -top-1.5 left-1/2 -ml-1.5 size-2.5 rounded-full border-[1.5px]" style={{ background: 'var(--ct-surface)', borderColor: 'var(--ct-faint)' }} />
                  {n.kind !== 'end' &&
                    onConnect &&
                    // The out ports: drag one onto another node to link them that way (the inspector's Links does the same by
                    // keyboard). One per way out: a condition's yes and no, a switch's cases, a loop's body, and after it
                    (n.ports ?? [{ id: 'next', label: '' }]).map((p, i, all) => {
                      const active = linking?.from === n.id && linking.port === p.id
                      return (
                        <span
                          key={p.id}
                          data-port={p.id}
                          className="absolute top-full -mt-2 flex -translate-x-1/2 flex-col items-center"
                          style={{ left: `${((i + 1) / (all.length + 1)) * 100}%` }}
                        >
                          <span
                            aria-hidden
                            title={p.label ? `${p.label}: drag onto a node to link it this way` : 'Drag onto another node to link them'}
                            onPointerDown={startLink(n.id, p.id)}
                            className="size-4 cursor-crosshair rounded-full border-2 hover:scale-125"
                            style={{ background: 'var(--ct-surface)', borderColor: active ? 'var(--ct-accent)' : 'var(--ct-faint)' }}
                          />
                          {p.label && (
                            <span className="pointer-events-none mt-0.5 max-w-[64px] truncate text-[10px] leading-none font-semibold text-ct-text-3">{p.label}</span>
                          )}
                        </span>
                      )
                    })}
                </>
              )}
            </div>
          )
        })}
      </div>

      <div className="pointer-events-none absolute right-3.5 bottom-3.5 flex flex-col items-end gap-2">
        {mode === 'design' && (
          <span className="rounded-[10px] border border-ct-border bg-ct-surface px-3 py-2 text-xs text-ct-text-3">
            Drag nodes to move them · drag a bottom dot onto a node to link · click a link to select it · Del deletes · Ctrl+Z undoes
          </span>
        )}
        <div role="group" aria-label="Zoom" className="ct-card pointer-events-auto flex items-center p-0.5">
          <ZoomButton label="Zoom out" onClick={() => zoomBy(-0.1)}>
            <Minus className="size-4" />
          </ZoomButton>
          <span className="min-w-12 text-center text-[13px] font-semibold tabular-nums">{Math.round(v.zoom * 100)}%</span>
          <ZoomButton label="Zoom in" onClick={() => zoomBy(0.1)}>
            <Plus className="size-4" />
          </ZoomButton>
          <span aria-hidden className="mx-0.5 h-6 w-px bg-ct-border" />
          <ZoomButton label="Fit to view" onClick={fit}>
            <Maximize className="size-4" />
          </ZoomButton>
          {onResetLayout && (
            <ZoomButton label="Reset layout: every node back in its place" onClick={onResetLayout}>
              <LayoutGrid className="size-4" />
            </ZoomButton>
          )}
        </div>
      </div>
      {children && <div className="absolute bottom-3.5 left-3.5">{children}</div>}
    </div>
  )
}
