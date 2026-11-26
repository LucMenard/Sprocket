import { invoke } from '@tauri-apps/api/core'
import { relaunch } from '@tauri-apps/plugin-process'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { useEffect, useState } from 'react'
import { useRuns } from '@/features/runs/useRuns'
import { inTauri } from '@/lib/api'

type Status =
  | { kind: 'idle' }
  | { kind: 'available'; update: Update }
  /** `percent` is null until the size of the download is known */
  | { kind: 'installing'; percent: number | null }
  | { kind: 'error'; message: string }

const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error))

/**
 * A band under the header when a new version is on GitHub: checked once a few seconds after start (not in a dev
 * build, which must not replace itself with a release). Installing stops the agents first, so the installer can
 * replace their exe; runs that were going show as interrupted and can be retried.
 */
export function UpdateNotice() {
  const [status, setStatus] = useState<Status>({ kind: 'idle' })
  const [dismissed, setDismissed] = useState(false)
  const going = (useRuns().data ?? []).filter((r) => r.state === 'running' || r.state === 'waiting').length

  useEffect(() => {
    if (!inTauri || import.meta.env.DEV) return
    const id = window.setTimeout(() => {
      check()
        .then((update) => update && setStatus({ kind: 'available', update }))
        .catch(() => {}) // offline, or GitHub unreachable: try again next start
    }, 5000)
    return () => window.clearTimeout(id)
  }, [])

  if (status.kind === 'idle' || dismissed) return null

  const install = async () => {
    if (status.kind !== 'available') return
    let total = 0
    let received = 0
    setStatus({ kind: 'installing', percent: null })
    try {
      await invoke('stop_agents_for_update')
      // The download is checked against the public key in the app's configuration before it runs
      await status.update.downloadAndInstall((event) => {
        if (event.event === 'Started') total = event.data.contentLength ?? 0
        if (event.event === 'Progress') {
          received += event.data.chunkLength
          setStatus({ kind: 'installing', percent: total ? Math.min(100, Math.round((received / total) * 100)) : null })
        }
      })
      await relaunch()
    } catch (error) {
      setStatus({ kind: 'error', message: messageOf(error) })
    }
  }

  return (
    <div role="status" className="flex flex-wrap items-center gap-3 border-b border-ct-border bg-ct-surface px-5 py-2 text-[13px]">
      {status.kind === 'available' && (
        <>
          <span>
            Sprocket {status.update.version} is available.
            {going > 0 && (
              <span className="text-ct-text-3">
                {' '}
                Installing it stops the {going} run{going > 1 ? 's' : ''} going now; you can retry them after.
              </span>
            )}
          </span>
          <button type="button" className="ct-btn ct-btn-primary ml-auto" onClick={() => void install()}>
            Install and restart
          </button>
          <button type="button" className="ct-btn" onClick={() => setDismissed(true)}>
            Later
          </button>
        </>
      )}
      {status.kind === 'installing' && <span>Installing the update{status.percent !== null ? ` (${status.percent}%)` : ''}…</span>}
      {status.kind === 'error' && (
        <>
          <span className="text-ct-failed">The update failed: {status.message}</span>
          <button type="button" className="ct-btn ml-auto" onClick={() => setDismissed(true)}>
            Dismiss
          </button>
        </>
      )}
    </div>
  )
}
