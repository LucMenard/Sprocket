import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'

/** The mockups' white card with a title, the building block of every screen. */
export function Section({ title, aside, children, className }: { title: string; aside?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cn('ct-card flex flex-col gap-3 px-[18px] py-4', className)}>
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-[15px] font-semibold">{title}</h2>
        {aside}
      </div>
      {children}
    </section>
  )
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="text-ct-text-3">{children}</p>
}
