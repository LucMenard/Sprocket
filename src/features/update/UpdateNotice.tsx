import { useEffect } from 'react'
import { canUpdate, useRunsGoing, useUpdates } from './updateStore'

/**
 * A band under the header when a new version is on GitHub. Unless turned off in Settings, Sprocket checks once a
 * few seconds after it starts; Settings → Updates checks on demand. Installing stops the agents first, so the
 * installer can replace their exe; runs that were going show as interrupted and can be retried.
 */
export function UpdateNotice() {
  const status = useUpdates((s) => s.status)
  const dismissed = useUpdates((s) => s.dismissed)
  const dismiss = useUpdates((s) => s.dismiss)
  const install = useUpdates((s) => s.install)
  const going = useRunsGoing()

  useEffect(() => {
    if (!canUpdate || !useUpdates.getState().auto) return
    const id = window.setTimeout(() => void useUpdates.getState().check(false), 5000)
    return () => window.clearTimeout(id)
  }, [])

  // A failed check is shown in Settings, where it was asked; a failed install here, where it started
  const visible =
    status.kind === 'installing' ||
    (status.kind === 'available' && status.update.version !== dismissed) ||
    (status.kind === 'error' && status.install === true)
  if (!visible) return null

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
          <button type="button" className="ct-btn" onClick={dismiss}>
            Later
          </button>
        </>
      )}
      {status.kind === 'installing' && <span>Installing the update{status.percent !== null ? ` (${status.percent}%)` : ''}…</span>}
      {status.kind === 'error' && (
        <>
          <span className="text-ct-failed">The update failed: {status.message}</span>
          <button type="button" className="ct-btn ml-auto" onClick={dismiss}>
            Dismiss
          </button>
        </>
      )}
    </div>
  )
}
