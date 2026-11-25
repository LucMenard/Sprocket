import { useState, type ReactNode } from 'react'
import { useWorkflows } from '@/features/workflows/useWorkflows'
import { api } from '@/lib/api'
import { when } from '@/lib/format'
import { useMcpServers, useStatus, useTracketConnection } from '@/lib/queries'

function Dot({ color }: { color: string }) {
  return <span aria-hidden className="size-2 shrink-0 rounded-full" style={{ background: color }} />
}

const mcpColor = (status: string) =>
  status === 'connected' ? 'var(--ct-connected)' : status === 'pending' ? 'var(--ct-waiting)' : status === 'failed' ? 'var(--ct-failed)' : 'var(--ct-idle)'

/** A card of the mockup's Integrations screen: a title, a line under it, then its content. */
function Card({ title, sub, aside, children }: { title: string; sub?: string; aside?: ReactNode; children: ReactNode }) {
  return (
    <section className="ct-card flex flex-col gap-3 px-5 py-[18px]">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-col gap-0.5">
          <h2 className="text-[15px] font-semibold">{title}</h2>
          {sub && <span className="text-[13px] text-ct-text-3">{sub}</span>}
        </div>
        {aside}
      </div>
      {children}
    </section>
  )
}

function CopyToken() {
  const [copied, setCopied] = useState(false)
  return (
    <button
      type="button"
      className="ct-btn ct-btn-primary"
      onClick={async () => {
        await navigator.clipboard.writeText(await api.apiToken())
        setCopied(true)
        setTimeout(() => setCopied(false), 1500)
      }}
    >
      {copied ? 'Copied' : 'Copy token'}
    </button>
  )
}

function TracketPanel() {
  const status = useStatus().data
  const { calls, lastOk, connected } = useTracketConnection()
  return (
    <>
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="flex-1 text-[22px] font-bold">Tracket</h1>
        <span className={connected ? 'ct-pill ct-pill-pass' : 'ct-pill ct-pill-none'}>
          {connected ? `Connected · last call ${when(new Date(lastOk!.at).toISOString())}` : 'No call from Tracket yet'}
        </span>
      </div>

      <Card title="Connection" sub="Tracket starts runs and approves steps through this address." aside={status?.apiAddress ? <CopyToken /> : undefined}>
        {status?.apiAddress ? (
          <div className="flex flex-col gap-1.5">
            <span className="ct-label">Address</span>
            <input className="ct-input ct-mono" readOnly value={`http://${status.apiAddress}`} aria-label="Address" />
            <p className="text-[13px] text-ct-text-3">
              In Tracket, open Settings › Sprocket and enter this address and the token. Only this computer can reach it, and
              the token is kept in Windows Credential Manager on both sides.
            </p>
          </div>
        ) : (
          <p className="text-ct-failed">{status?.apiError ?? 'The local API is not running'}</p>
        )}
      </Card>

      <Card title="Recent calls" sub="What Tracket (or anything holding the token) asked Sprocket.">
        {calls.length === 0 ? (
          <p className="text-ct-text-3">None since Sprocket started.</p>
        ) : (
          <ol className="flex flex-col">
            {calls.map((c, i) => (
              <li key={i} className="flex items-center gap-3 border-t border-ct-border-soft py-2 text-[13px] first:border-t-0">
                <span className="w-12 text-xs text-ct-text-3 ct-mono">{when(new Date(c.at).toISOString())}</span>
                <span className="flex-1 ct-mono">
                  {c.method} {c.path}
                </span>
                <span className={c.status >= 400 ? 'ct-pill ct-pill-fail' : 'ct-pill ct-pill-pass'}>{c.status}</span>
              </li>
            ))}
          </ol>
        )}
      </Card>

      <div className="grid grid-cols-[repeat(auto-fit,minmax(240px,1fr))] items-stretch gap-5 opacity-70">
        <Card title="When to start a workflow" sub="Coming later: rules on Jira events.">
          <p className="text-[13px] text-ct-text-3">Such as a label added, a ticket created or moved. For now runs start by hand.</p>
        </Card>
        <Card title="Update the ticket when the run…" sub="Coming later: status mapping.">
          <p className="text-[13px] text-ct-text-3">Move the Jira status as the run starts, waits, passes or fails. Sprocket writes nothing to Jira yet.</p>
        </Card>
        <Card title="Post back to Tracket" sub="Coming later.">
          <p className="text-[13px] text-ct-text-3">A comment per step, approvals on the ticket, links to pull requests.</p>
        </Card>
      </div>
    </>
  )
}

function McpPanel({ name, status, source }: { name: string; status: string; source: string | null }) {
  const workflows = useWorkflows().data?.workflows ?? []
  const users = workflows.flatMap((w) => w.agents.filter((a) => a.mcp.includes(name)).map((a) => `${a.id} (${w.name})`))
  return (
    <>
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="flex-1 text-[22px] font-bold ct-mono">{name}</h1>
        <span className={status === 'connected' ? 'ct-pill ct-pill-pass' : status === 'pending' ? 'ct-pill ct-pill-wait' : 'ct-pill ct-pill-none'}>{status}</span>
      </div>
      <Card title="Used by" sub="Agent types that list it in their mcp: servers.">
        {users.length ? <p>{users.join(', ')}</p> : <p className="text-ct-text-3">No agent type uses it.</p>}
      </Card>
      <Card title="Where it comes from">
        <p className="text-[13px] text-ct-text-2">
          {source === 'claudeai' ? 'A claude.ai connector of your account.' : `Your Claude Code settings (${source ?? 'unknown'}).`} Sprocket adds no
          server of its own: agents use the ones Claude Code has, limited to those their agent type lists.
        </p>
      </Card>
    </>
  )
}

export function IntegrationsView() {
  const mcp = useMcpServers().data
  const { connected } = useTracketConnection()
  const [selected, setSelected] = useState('tracket')
  const server = mcp?.servers.find((s) => s.name === selected)

  return (
    <div className="flex h-full min-h-0">
      <nav aria-label="Integrations" className="flex w-[280px] shrink-0 flex-col gap-3.5 overflow-y-auto border-r border-ct-border bg-ct-surface px-3 py-4">
        <div className="flex flex-col gap-1">
          <span className="px-2 pb-1 ct-eyebrow">Work comes from</span>
          <button type="button" className="ct-row min-h-[52px]" aria-current={selected === 'tracket' ? 'page' : undefined} onClick={() => setSelected('tracket')}>
            <span aria-hidden className="flex size-[30px] shrink-0 items-center justify-center rounded-[7px] font-bold text-white" style={{ background: '#0F766E' }}>
              T
            </span>
            <span className="flex flex-1 flex-col gap-px">
              <span className="font-semibold">Tracket</span>
              <span className="text-xs text-ct-text-2">Tickets and approvals</span>
            </span>
            <Dot color={connected ? 'var(--ct-connected)' : 'var(--ct-idle)'} />
          </button>
        </div>
        <div className="flex flex-col gap-1">
          <span className="px-2 pb-1 ct-eyebrow">Tools for agents</span>
          {!mcp?.seenAt && <p className="px-2 text-xs text-ct-text-3">Listed once an agent has run.</p>}
          {mcp?.servers.map((s) => (
            <button key={s.name} type="button" className="ct-row min-h-11 py-1" aria-current={selected === s.name ? 'page' : undefined} onClick={() => setSelected(s.name)}>
              <span className="min-w-0 flex-1 truncate font-medium">{s.name}</span>
              <span className="text-xs text-ct-text-2">{s.status}</span>
              <Dot color={mcpColor(s.status)} />
            </button>
          ))}
          {mcp?.seenAt && <p className="px-2 pt-1 text-[11px] text-ct-text-3">As the latest agent session saw them, {when(mcp.seenAt)}.</p>}
        </div>
      </nav>
      <main className="min-w-0 flex-1 overflow-y-auto px-8 py-6">
        <div className="flex max-w-[1080px] flex-col gap-5">{server ? <McpPanel {...server} /> : <TracketPanel />}</div>
      </main>
    </div>
  )
}
