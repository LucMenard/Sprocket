/** Activity rows (`run.started`, `tool.asked`…) in words, for the event feed and the job logs. */
export function describe(what: string, detail: string | null): string {
  const d = detail ?? ''
  switch (what) {
    case 'run.started':
      return `Run started: ${d}`
    case 'run.passed':
      return 'Run passed'
    case 'run.cancelled':
      return `Run cancelled by ${d}`
    case 'run.interrupted':
      return `Run interrupted: ${d}`
    case 'job.started':
      return `Step ${d} started`
    case 'job.resumed':
      return `Step ${d} resumed its session`
    case 'job.retried':
      return `Step ${d} retried`
    case 'job.done':
      return `Step ${d} done`
    case 'job.failed':
      return `Step failed: ${d}`
    case 'job.interrupted':
      return `Interrupted: ${d}`
    case 'gate.opened':
      return `Waiting for approval at ${d}`
    case 'gate.approved':
      return `Approved by ${d}`
    case 'gate.rejected':
      return `Rejected by ${d}`
    case 'tool.asked':
      return `Agent asks to use ${d}`
    case 'tool.approved':
      return `Tool allowed by ${d}`
    case 'tool.rejected':
      return `Tool denied by ${d}`
    case 'note.posted':
      return `Note on the board from ${d}`
    case 'team.started':
      return `Team at work: ${d}`
    case 'team.finished':
      return `Team done: ${d}`
    case 'team.limit':
      return `Team stopped at a limit: ${d}`
    case 'team.resumed':
      return `Team goes on: ${d}`
    case 'colleague.asked':
      return `Asked ${d}`
    case 'colleague.answered':
      return `${d} answered`
    case 'colleague.failed':
      return `${d} couldn't answer`
    default:
      return d ? `${what}: ${d}` : what
  }
}

/** Events that need someone's attention are shown in the waiting colour. */
export function isAttention(what: string) {
  return what === 'gate.opened' || what === 'tool.asked' || what.endsWith('.failed') || what.endsWith('.interrupted')
}
