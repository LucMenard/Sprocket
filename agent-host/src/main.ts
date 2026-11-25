// Sprocket's agent host. Started by the Rust core:
//   sprocket-agent-host --claude <path to claude.exe> [--settings user,project]
// Reads CoreMessage lines on stdin, writes HostMessage lines on stdout, exits when stdin closes.
import { execFile } from 'node:child_process'
import { createInterface } from 'node:readline'
import { promisify } from 'node:util'
import { Jobs, type HostConfig } from './jobs.ts'
import { PROTOCOL, type CoreMessage, type HostMessage } from './protocol.ts'

declare const __SDK_VERSION__: string

const send = (message: HostMessage) => process.stdout.write(`${JSON.stringify(message)}\n`)
const log = (text: string) => process.stderr.write(`${text}\n`)

function parseArgs(argv: string[]): HostConfig {
  const value = (flag: string) => {
    const i = argv.indexOf(flag)
    return i >= 0 ? argv[i + 1] : undefined
  }
  const claudePath = value('--claude')
  if (!claudePath) throw new Error('--claude <path> is required')
  const settings = (value('--settings') ?? 'user,project').split(',').filter(Boolean)
  const valid = settings.filter((s): s is 'user' | 'project' | 'local' => ['user', 'project', 'local'].includes(s))
  return { claudePath, settingSources: valid }
}

async function claudeVersion(path: string): Promise<string> {
  const { stdout } = await promisify(execFile)(path, ['--version'], { timeout: 15_000, windowsHide: true })
  // "2.1.331 (Claude Code)"
  return stdout.trim().split(/\s+/)[0] ?? stdout.trim()
}

async function main() {
  let config: HostConfig
  let claudeCode: string
  try {
    config = parseArgs(process.argv.slice(2))
    claudeCode = await claudeVersion(config.claudePath)
  } catch (error) {
    send({ type: 'host.error', message: `Can't start Claude Code: ${String(error)}` })
    process.exit(1)
  }

  const jobs = new Jobs(config, send)
  const lines = createInterface({ input: process.stdin })
  lines.on('line', (line) => {
    if (!line.trim()) return
    try {
      jobs.handle(JSON.parse(line) as CoreMessage)
    } catch (error) {
      log(`Bad message from the core: ${String(error)}`)
    }
  })
  // The core went away (quit or crash): stop the agents rather than leave claude.exe running
  lines.on('close', () => {
    jobs.stopAll()
    setTimeout(() => process.exit(0), 2000).unref()
  })

  send({ type: 'host.ready', protocol: PROTOCOL, sdk: __SDK_VERSION__, claudeCode })
}

void main()
