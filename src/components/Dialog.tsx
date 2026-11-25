import { X } from 'lucide-react'
import { useEffect, useId, type ReactNode } from 'react'
import { Button } from '@/components/ui/button'

/** A modal panel over the window; Escape or a click outside closes it. */
export function Dialog({ title, onClose, children }: { title: ReactNode; onClose: () => void; children: ReactNode }) {
  const titleId = useId()
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 px-4 pt-[6vh]"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="flex max-h-[86vh] w-full max-w-3xl flex-col overflow-hidden rounded-lg border bg-background shadow-xl"
      >
        <div className="flex items-center gap-2 border-b px-4 py-3">
          <h2 id={titleId} className="min-w-0 truncate text-sm font-semibold">
            {title}
          </h2>
          <Button variant="ghost" size="icon-xs" className="ml-auto" aria-label="Close" onClick={onClose}>
            <X />
          </Button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto px-4 py-3">{children}</div>
      </div>
    </div>
  )
}
