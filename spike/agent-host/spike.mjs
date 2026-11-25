// Spike: can the Agent SDK do what Control Tower's agent host needs?
//   node spike.mjs run <cwd> <askWaitSeconds> [pathToClaude]
//   node spike.mjs resume <cwd> <sessionId> [pathToClaude]
// Prints one JSON line per event, like the real host would on stdout.
import { query } from '@anthropic-ai/claude-agent-sdk'

const [mode, cwd, arg, claudePath] = process.argv.slice(2)
const out = (o) => console.log(JSON.stringify({ t: new Date().toISOString(), ...o }))
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

const asks = []
const canUseTool = async (toolName, input, { signal }) => {
  const started = Date.now()
  out({ type: 'tool.ask', toolName, input })
  asks.push(toolName)
  // Stands in for a human answering in the UI some time later
  const waitMs = mode === 'run' ? Number(arg) * 1000 : 0
  await sleep(waitMs)
  out({ type: 'tool.answer', toolName, waitedSeconds: Math.round((Date.now() - started) / 1000), aborted: signal.aborted })
  return { behavior: 'allow', updatedInput: input }
}

const prompts = {
  run: [
    'This is a test of an automation harness. Do these steps and nothing else:',
    '1. Tell me the project codename written in CLAUDE.md.',
    '2. List the names of the MCP servers you have tools from (the part after mcp__, before the next __). Say "none" if none.',
    '3. Create a file hello.txt containing exactly: hi from the spike',
    'Then stop.',
  ].join('\n'),
  resume: 'What file did you create in this session, and what was the codename? Answer in one line, without using any tool.',
}

const options = {
  cwd,
  settingSources: ['user', 'project'],
  permissionMode: 'default',
  canUseTool,
  ...(claudePath ? { pathToClaudeCodeExecutable: claudePath } : {}),
  ...(mode === 'resume' ? { resume: arg } : {}),
  stderr: (line) => out({ type: 'stderr', line: line.slice(0, 300) }),
}

async function main() {
const t0 = Date.now()
try {
  for await (const m of query({ prompt: prompts[mode], options })) {
    if (m.type === 'system' && m.subtype === 'init') {
      out({ type: 'init', sessionId: m.session_id, model: m.model, cwd: m.cwd, permissionMode: m.permissionMode,
        mcpServers: m.mcp_servers, apiKeySource: m.apiKeySource, claudeCodeVersion: m.claude_code_version })
    } else if (m.type === 'assistant') {
      for (const b of m.message.content) {
        if (b.type === 'text') out({ type: 'text', text: b.text })
        if (b.type === 'tool_use') out({ type: 'tool_use', name: b.name })
      }
    } else if (m.type === 'result') {
      out({ type: 'result', subtype: m.subtype, sessionId: m.session_id, costUsd: m.total_cost_usd,
        turns: m.num_turns, result: m.result, denials: m.permission_denials })
    }
  }
} catch (e) {
  out({ type: 'error', message: String(e?.stack ?? e).slice(0, 2000) })
  process.exitCode = 1
}
out({ type: 'done', seconds: Math.round((Date.now() - t0) / 1000), asks })
}
main()
