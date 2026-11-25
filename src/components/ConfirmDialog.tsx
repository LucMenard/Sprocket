import { useEffect, useId, useRef, type ReactNode } from 'react'

/**
 * Asks before something that can't be undone. The app's window doesn't reliably show the browser's own
 * `window.confirm`, so this is drawn by the app. Escape or a click outside cancels; the safe choice has the focus.
 */
export function ConfirmDialog({
  title,
  children,
  confirm,
  danger,
  busy,
  onConfirm,
  onCancel,
}: {
  title: string
  children?: ReactNode
  /** The confirming button's words: "Delete workflow" */
  confirm: string
  danger?: boolean
  busy?: boolean
  onConfirm: () => void
  onCancel: () => void
}) {
  const titleId = useId()
  const textId = useId()
  const cancel = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    cancel.current?.focus()
    const onKeyDown = (e: KeyboardEvent) => e.key === 'Escape' && onCancel()
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onCancel])

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 px-4 pt-[18vh]" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={children ? textId : undefined}
        className="ct-card flex w-full max-w-md flex-col gap-3 p-5 shadow-xl"
      >
        <h2 id={titleId} className="text-base font-bold">
          {title}
        </h2>
        {children && (
          <div id={textId} className="text-[13px] leading-normal text-ct-text-2">
            {children}
          </div>
        )}
        <div className="mt-2 flex justify-end gap-2">
          <button ref={cancel} type="button" className="ct-btn" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className={`ct-btn ${danger ? 'ct-btn-danger-solid' : 'ct-btn-primary'}`} disabled={busy} onClick={onConfirm}>
            {confirm}
          </button>
        </div>
      </div>
    </div>
  )
}
