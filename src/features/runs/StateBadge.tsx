import type { JobState, RunState } from '@/lib/api'
import { cn } from '@/lib/utils'

type State = RunState | JobState | 'pending'

/** Colour, glyph and pill of each state, as the mockups draw them. */
const LOOK: Record<State, { label: string; color: string | null; glyph: string; pill: string }> = {
  running: { label: 'Running', color: 'var(--ct-accent)', glyph: 'M12 7v5l3 2', pill: 'ct-pill-run' },
  waiting: { label: 'Waiting for approval', color: 'var(--ct-waiting)', glyph: 'M10 8v8M14 8v8', pill: 'ct-pill-wait' },
  passed: { label: 'Passed', color: 'var(--ct-done)', glyph: 'M7 12.5l3.5 3.5 6.5-8', pill: 'ct-pill-pass' },
  done: { label: 'Done', color: 'var(--ct-done)', glyph: 'M7 12.5l3.5 3.5 6.5-8', pill: 'ct-pill-pass' },
  failed: { label: 'Failed', color: 'var(--ct-failed)', glyph: 'M8.5 8.5l7 7M15.5 8.5l-7 7', pill: 'ct-pill-fail' },
  rejected: { label: 'Rejected', color: 'var(--ct-failed)', glyph: 'M8.5 8.5l7 7M15.5 8.5l-7 7', pill: 'ct-pill-fail' },
  cancelled: { label: 'Cancelled', color: 'var(--ct-idle)', glyph: 'M8 16l8-8', pill: 'ct-pill-none' },
  stopped: { label: 'Stopped', color: 'var(--ct-idle)', glyph: 'M8 16l8-8', pill: 'ct-pill-none' },
  interrupted: { label: 'Interrupted', color: 'var(--ct-waiting)', glyph: 'M12 7v6M12 16.5v.5', pill: 'ct-pill-wait' },
  pending: { label: 'Not started', color: null, glyph: '', pill: 'ct-pill-none' },
}

export function stateLabel(state: State) {
  return LOOK[state].label
}

/**
 * A round state mark: filled with a white glyph, or an empty ring when not started.
 * `size` 8 or less draws a plain dot (lists of agents, step dots in tables).
 */
export function StateDot({ state, size = 20, className }: { state: State; size?: number; className?: string }) {
  const look = LOOK[state]
  const plain = size <= 10
  return (
    <span
      aria-hidden
      className={cn('flex shrink-0 items-center justify-center rounded-full', state === 'running' && 'animate-pulse', className)}
      style={{
        width: size,
        height: size,
        background: look.color ?? 'var(--ct-surface)',
        border: look.color ? undefined : `2px solid var(--ct-idle)`,
        boxSizing: 'border-box',
      }}
    >
      {!plain && look.glyph && (
        <svg width={size * 0.55} height={size * 0.55} viewBox="0 0 24 24" fill="none" stroke="#FFFFFF" strokeWidth="3.4" strokeLinecap="round" strokeLinejoin="round">
          <path d={look.glyph} />
        </svg>
      )}
    </span>
  )
}

export function StateBadge({ state }: { state: State }) {
  return <span className={cn('ct-pill', LOOK[state].pill)}>{LOOK[state].label}</span>
}
