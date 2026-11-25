import { getCurrentWindow } from '@tauri-apps/api/window'
import { useEffect, useState, type ReactNode } from 'react'
import { inTauri } from '@/lib/api'
import { useNav, type ViewId } from '@/lib/nav'
import { useStatus, useTracketConnection } from '@/lib/queries'
import { getThemePreference, setThemePreference, type ThemePreference } from '@/lib/theme'
import { Logo } from './Logo'

const TABS: { id: ViewId; label: string }[] = [
  { id: 'overview', label: 'Overview' },
  { id: 'runs', label: 'Runs' },
  { id: 'workflows', label: 'Workflows' },
  { id: 'agents', label: 'Agents' },
  { id: 'integrations', label: 'Integrations' },
]

/** Hovered: muted background, as the mockup's `.sp-ghost`. */
const ghost = 'hover:bg-[var(--muted)] hover:text-[var(--fg)]'

function WindowButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className={`flex h-full w-[46px] items-center justify-center text-[var(--mfg)] ${label === 'Close' ? 'hover:bg-red-600 hover:text-white' : ghost}`}
    >
      {children}
    </button>
  )
}

/** The window's own title bar, as Tracket's: the mark and the name, and the three window buttons. */
function TitleBar() {
  const [maximized, setMaximized] = useState(false)
  useEffect(() => {
    if (!inTauri) return
    const win = getCurrentWindow()
    const sync = () => win.isMaximized().then(setMaximized).catch(() => {})
    void sync()
    const unlisten = win.onResized(sync)
    return () => void unlisten.then((off) => off())
  }, [])
  if (!inTauri) return null
  const win = getCurrentWindow()

  return (
    <div data-tauri-drag-region className="flex h-9 flex-none items-center border-b border-[var(--border)] bg-[var(--bg)] select-none">
      <div data-tauri-drag-region className="flex items-center gap-2 pl-3.5">
        <span data-tauri-drag-region className="pointer-events-none flex">
          <Logo />
        </span>
        <span data-tauri-drag-region className="pointer-events-none text-xs font-medium text-[var(--mfg)]">
          Sprocket
        </span>
      </div>
      <div className="ml-auto flex h-full">
        <WindowButton label="Minimize" onClick={() => void win.minimize()}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden>
            <path d="M5 12h14" />
          </svg>
        </WindowButton>
        <WindowButton label={maximized ? 'Restore' : 'Maximize'} onClick={() => void win.toggleMaximize()}>
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden>
            <path d="M3 3h18v18H3z" />
          </svg>
        </WindowButton>
        <WindowButton label="Close" onClick={() => void win.close()}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden>
            <path d="M6 6l12 12M18 6L6 18" />
          </svg>
        </WindowButton>
      </div>
    </div>
  )
}

function Status() {
  const go = useNav((s) => s.go)
  const status = useStatus().data
  const { connected } = useTracketConnection()
  if (!status) return null
  const host = status.host
  const items: [string, string][] = [
    [
      host.state === 'ready' ? '#16A34A' : host.state === 'starting' ? 'var(--lit)' : 'var(--st-fail-fg)',
      host.state === 'ready' ? 'Agents ready' : host.state === 'starting' ? 'Agents starting' : 'Agents unavailable',
    ],
    [
      !status.apiAddress ? 'var(--st-fail-fg)' : connected ? '#16A34A' : '#9CA3AF',
      !status.apiAddress ? 'API not listening' : connected ? 'Tracket connected' : 'Tracket not seen yet',
    ],
  ]
  return (
    <button
      type="button"
      onClick={() => go('integrations')}
      title={host.state === 'failed' ? host.message : 'Open Integrations'}
      className="flex items-center gap-3 text-xs text-[var(--mfg)] hover:text-[var(--fg)]"
    >
      {items.map(([color, text]) => (
        <span key={text} className="flex items-center gap-1.5">
          <span aria-hidden className="size-[7px] rounded-full" style={{ background: color }} />
          {text}
        </span>
      ))}
    </button>
  )
}

const NEXT: Record<ThemePreference, ThemePreference> = { system: 'light', light: 'dark', dark: 'system' }
const THEME_ICON: Record<ThemePreference, string> = {
  dark: 'M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z',
  light: 'M12 4V2M12 22v-2M4 12H2M22 12h-2M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z',
  system: 'M3 5h18v12H3zM8 21h8M12 17v4',
}

function ThemeButton() {
  const [preference, setPreference] = useState(getThemePreference)
  const title = `Theme: ${preference[0].toUpperCase()}${preference.slice(1)} (click for ${NEXT[preference]})`
  return (
    <button
      type="button"
      aria-label={title}
      title={title}
      onClick={() => {
        setThemePreference(NEXT[preference])
        setPreference(NEXT[preference])
      }}
      className={`flex size-9 items-center justify-center rounded-lg text-[var(--mfg)] ${ghost}`}
    >
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
        <path d={THEME_ICON[preference]} />
      </svg>
    </button>
  )
}

/** The mockup's top: the title bar, then the toolbar with the screens as a segmented control and the status. */
export function AppHeader({ waiting }: { waiting: number }) {
  const { view, go } = useNav()
  return (
    <>
      <TitleBar />
      <div className="flex min-h-[45px] flex-none flex-wrap items-center gap-x-3 gap-y-2 border-b border-[var(--border)] bg-[var(--bg)] px-4 py-1.5">
        <nav role="tablist" aria-label="Sprocket" className="flex flex-wrap rounded-md bg-[var(--muted)] p-0.5">
          {TABS.map((t) => {
            const on = t.id === view
            return (
              <button
                key={t.id}
                type="button"
                role="tab"
                aria-selected={on}
                onClick={() => go(t.id)}
                className="flex min-h-7 items-center gap-1.5 rounded px-3 text-[13px] font-medium"
                style={on ? { background: 'var(--bg)', color: 'var(--fg)', boxShadow: '0 1px 2px rgba(0,0,0,0.15)' } : { color: 'var(--mfg)' }}
              >
                {t.label}
                {t.id === 'overview' && waiting > 0 && (
                  <span className="rounded-full px-1.5 text-[11px] leading-4 font-semibold" style={{ background: 'var(--lit-bg)', color: 'var(--lit)' }} aria-label={`${waiting} waiting on you`}>
                    {waiting}
                  </span>
                )}
              </button>
            )
          })}
        </nav>
        <div className="ml-auto flex items-center gap-2.5">
          <Status />
          <ThemeButton />
        </div>
      </div>
    </>
  )
}
