import { openUrl } from '@tauri-apps/plugin-opener'
import { Download, FolderOpen, Info, Palette, X, type LucideIcon } from 'lucide-react'
import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { canUpdate, useRunsGoing, useUpdates } from '@/features/update/updateStore'
import { api } from '@/lib/api'
import { useStatus } from '@/lib/queries'
import { setThemePreference, useThemePreference, type ThemePreference } from '@/lib/theme'
import { cn } from '@/lib/utils'

const REPO_URL = 'https://github.com/LucMenard/Sprocket'

type Category = 'appearance' | 'updates' | 'data' | 'about'

const CATEGORIES: { id: Category; label: string; icon: LucideIcon }[] = [
  { id: 'appearance', label: 'Appearance', icon: Palette },
  { id: 'updates', label: 'Updates', icon: Download },
  { id: 'data', label: 'Data', icon: FolderOpen },
  { id: 'about', label: 'About', icon: Info },
]

function Heading({ children, first }: { children: ReactNode; first?: boolean }) {
  return <h3 className={cn('text-xs font-medium uppercase tracking-wide text-ct-text-3', !first && 'mt-8')}>{children}</h3>
}

const THEMES: { id: ThemePreference; label: string; sub: string }[] = [
  { id: 'system', label: 'System', sub: 'Follows Windows' },
  { id: 'light', label: 'Light', sub: '' },
  { id: 'dark', label: 'Dark', sub: '' },
]

function AppearanceSettings() {
  const preference = useThemePreference()
  return (
    <div>
      <Heading first>Theme</Heading>
      <div role="radiogroup" aria-label="Theme" className="mt-2 flex flex-col gap-2">
        {THEMES.map((t) => (
          <label key={t.id} className="flex items-center gap-2 text-sm">
            <input type="radio" name="theme" checked={preference === t.id} onChange={() => setThemePreference(t.id)} />
            {t.label}
            {t.sub && <span className="text-ct-text-3">{t.sub}</span>}
          </label>
        ))}
      </div>
      <p className="mt-3 text-xs text-ct-text-3">The button beside the status in the header switches between the same three.</p>
    </div>
  )
}

function UpdateSettings() {
  const version = useStatus().data?.version
  const status = useUpdates((s) => s.status)
  const auto = useUpdates((s) => s.auto)
  const setAuto = useUpdates((s) => s.setAuto)
  const check = useUpdates((s) => s.check)
  const install = useUpdates((s) => s.install)
  const going = useRunsGoing()
  const busy = status.kind === 'checking' || status.kind === 'installing'

  return (
    <div>
      <Heading first>Updates</Heading>
      <p className="mt-1 text-sm">You have Sprocket {version ?? ''}.</p>
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <button type="button" className="ct-btn" disabled={busy || !canUpdate} onClick={() => void check(true)}>
          {status.kind === 'checking' ? 'Checking…' : 'Check for updates'}
        </button>
        {status.kind === 'upToDate' && <span className="text-sm text-ct-text-3">You have the latest version.</span>}
        {status.kind === 'error' && (
          <span className="text-sm text-ct-failed">
            {status.install ? 'The update failed' : "Couldn't check"}: {status.message}
          </span>
        )}
        {status.kind === 'available' && (
          <>
            <span className="text-sm">Version {status.update.version} is available.</span>
            <button type="button" className="ct-btn ct-btn-primary" onClick={() => void install()}>
              Install and restart
            </button>
            <button
              type="button"
              className="text-xs text-ct-text-3 underline hover:text-ct-text"
              onClick={() => void openUrl(`${REPO_URL}/releases/tag/v${status.update.version}`)}
            >
              What's new
            </button>
          </>
        )}
        {status.kind === 'installing' && (
          <span className="text-sm text-ct-text-3">Downloading… {status.percent === null ? '' : `${status.percent}%`}</span>
        )}
      </div>
      {status.kind === 'available' && going > 0 && (
        <p className="mt-2 text-xs text-ct-text-3">
          Installing it stops the {going} run{going > 1 ? 's' : ''} going now; you can retry them after.
        </p>
      )}
      {!canUpdate && <p className="mt-2 text-xs text-ct-text-3">A development build doesn't update itself.</p>}

      <label className="mt-4 flex items-center gap-2 text-sm">
        <input type="checkbox" checked={auto} onChange={(e) => setAuto(e.target.checked)} />
        Check for updates when Sprocket starts
      </label>
      <p className="mt-1 pl-6 text-xs text-ct-text-3">
        When a new version is found, a band under the header offers to install it. Nothing is installed without you.
      </p>

      <p className="mt-6 text-xs text-ct-text-3">
        Checking asks github.com for the latest release, like opening its page would: GitHub sees your IP address, and
        nothing else is sent. Updates are verified against a signature before they are installed.
      </p>
    </div>
  )
}

function DataSettings() {
  const dataDir = useStatus().data?.dataDir
  const [error, setError] = useState<string>()
  return (
    <div>
      <Heading first>Data on this computer</Heading>
      <p className="mt-1 text-sm">
        Runs, their messages and the activity are in <code>sprocket.sqlite</code>; workflows and shared agent types are YAML files in
        the <code>workflows</code> and <code>agents</code> folders beside it. The API token is in the Windows Credential Manager.
      </p>
      {dataDir && <p className="mt-2 break-all rounded-md bg-ct-subtle px-2.5 py-1.5 font-mono text-xs">{dataDir}</p>}
      <button
        type="button"
        className="ct-btn mt-3"
        onClick={() => api.openDataDir().then(() => setError(undefined), (e: unknown) => setError(e instanceof Error ? e.message : String(e)))}
      >
        Open the folder
      </button>
      {error && <p className="mt-2 text-xs text-ct-failed">{error}</p>}

      <Heading>What leaves this computer</Heading>
      <ul className="mt-2 list-disc space-y-1 pl-5 text-sm text-ct-text-3">
        <li>Agents run with Claude Code: what a step asks, and the files they read, go to Anthropic as with any Claude Code session.</li>
        <li>MCP servers an agent type lists get what that agent sends them.</li>
        <li>Tracket reaches Sprocket on this computer only, with the token shown in Integrations.</li>
        <li>Checking for updates asks github.com for the latest release.</li>
      </ul>
    </div>
  )
}

const LINKS = [
  { label: 'Source code', url: REPO_URL },
  { label: 'Releases', url: `${REPO_URL}/releases` },
  { label: 'Report a problem', url: `${REPO_URL}/issues/new` },
]

function AboutSettings() {
  const version = useStatus().data?.version
  return (
    <div>
      <Heading first>About</Heading>
      <p className="mt-1 text-sm">Sprocket {version ?? ''}</p>
      <p className="text-sm text-ct-text-3">Runs Claude Code agents through workflows, for the tickets Tracket starts.</p>
      <Heading>Links</Heading>
      <ul className="mt-2 space-y-1">
        {LINKS.map(({ label, url }) => (
          <li key={url}>
            <button type="button" className="text-sm text-ct-text-3 underline hover:text-ct-text" onClick={() => void openUrl(url)}>
              {label}
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}

/** The settings, in categories listed on the left, as in Tracket. */
export function SettingsDialog({ onClose, initial = 'appearance' }: { onClose: () => void; initial?: Category }) {
  const [category, setCategory] = useState<Category>(initial)
  const tabs = useRef<Record<string, HTMLButtonElement | null>>({})

  useEffect(() => {
    const onKeyDown = (e: globalThis.KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  // Up/Down move between categories, like any vertical list of tabs
  const onTabKeyDown = (e: KeyboardEvent, at: number) => {
    const step = e.key === 'ArrowDown' ? 1 : e.key === 'ArrowUp' ? -1 : 0
    if (!step) return
    e.preventDefault()
    const next = CATEGORIES[(at + step + CATEGORIES.length) % CATEGORIES.length]
    setCategory(next.id)
    tabs.current[next.id]?.focus()
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 px-4 pt-[8vh]"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        className="flex h-[80vh] w-full max-w-3xl flex-col overflow-hidden rounded-lg border bg-background text-foreground shadow-xl"
      >
        <div className="flex items-center border-b px-4 py-3">
          <h2 id="settings-title" className="text-sm font-semibold">
            Settings
          </h2>
          <Button variant="ghost" size="icon-xs" className="ml-auto" aria-label="Close settings" onClick={onClose}>
            <X />
          </Button>
        </div>

        <div className="flex min-h-0 flex-1">
          <div role="tablist" aria-orientation="vertical" aria-label="Settings categories" className="flex w-44 shrink-0 flex-col gap-0.5 border-r bg-muted/30 p-2">
            {CATEGORIES.map(({ id, label, icon: CategoryIcon }, at) => (
              <button
                key={id}
                ref={(el) => {
                  tabs.current[id] = el
                }}
                type="button"
                role="tab"
                id={`settings-tab-${id}`}
                aria-selected={category === id}
                aria-controls="settings-panel"
                tabIndex={category === id ? 0 : -1}
                onClick={() => setCategory(id)}
                onKeyDown={(e) => onTabKeyDown(e, at)}
                className={cn(
                  'flex items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50',
                  category === id ? 'bg-accent font-medium text-accent-foreground' : 'text-muted-foreground hover:bg-accent/60',
                )}
              >
                <CategoryIcon className="size-4 shrink-0" />
                {label}
              </button>
            ))}
          </div>

          <div role="tabpanel" id="settings-panel" aria-labelledby={`settings-tab-${category}`} className="min-w-0 flex-1 overflow-auto px-6 py-5">
            {category === 'appearance' && <AppearanceSettings />}
            {category === 'updates' && <UpdateSettings />}
            {category === 'data' && <DataSettings />}
            {category === 'about' && <AboutSettings />}
          </div>
        </div>
      </div>
    </div>
  )
}
