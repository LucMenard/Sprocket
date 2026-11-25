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
