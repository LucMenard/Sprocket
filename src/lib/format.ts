/** The last part of a folder's path: `C:\dev\shop` gives `shop`. */
export function folderName(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).at(-1) || path
}

/** What a run works on, in a word: its ticket's key, or its folder's name when it has no ticket. */
export function workLabel(run: { ticket: { key: string } | null; folder: string }): string {
  return run.ticket?.key ?? folderName(run.folder)
}

/** What started a run, as the trigger node says it. */
export function startedFrom(run: { startedBy: string; parentRun: string | null }): string {
  const by = run.startedBy
  if (run.parentRun) return `Launched by ${by}`
  if (by === 'Schedule') return 'Started on its schedule'
  if (by === 'Webhook') return 'Started by its webhook'
  if (by.startsWith('After ')) return `Started after ${by.slice(6)}`
  if (by === 'You, in Sprocket' || by === 'Sprocket') return 'Started in Sprocket'
  return 'Started from Tracket'
}

/** "10:05" today, "Yesterday 16:12", or "24 Nov 16:12", in 24-hour time like the mockups. */
export function when(iso: string | null): string {
  if (!iso) return ''
  const date = new Date(iso)
  const time = date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })
  const days = Math.floor((startOfDay(new Date()) - startOfDay(date)) / 86_400_000)
  if (days === 0) return time
  if (days === 1) return `Yesterday ${time}`
  return `${date.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })} ${time}`
}

function startOfDay(d: Date) {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
}

/** "3 min", "1 h 12 min" between two times (the second defaults to now). */
export function duration(from: string | null, to: string | null): string {
  if (!from) return ''
  const seconds = Math.max(0, Math.round(((to ? new Date(to) : new Date()).getTime() - new Date(from).getTime()) / 1000))
  if (seconds < 60) return `${seconds} s`
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes} min`
  return `${Math.floor(minutes / 60)} h ${minutes % 60} min`
}

export function usd(value: number | null | undefined): string {
  if (!value) return ''
  return value < 0.01 ? '<$0.01' : `$${value.toFixed(2)}`
}
