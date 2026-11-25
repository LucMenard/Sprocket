import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Check } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { api, type AgentDraft, type AgentType, type AppError, type AskPolicy } from '@/lib/api'

const ASK: { id: AskPolicy; label: string; help: string }[] = [
  { id: 'never', label: 'Never', help: 'Runs on its own' },
  { id: 'before-writes', label: 'Before writes', help: 'Read freely, ask before changing things' },
  { id: 'every-action', label: 'Every action', help: 'Ask before each tool call' },
]

const MODELS = ['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5', 'claude-fable-5-2']
const TOOLS = ['Read', 'Grep', 'Glob', 'Edit', 'Write', 'Bash', 'WebSearch', 'WebFetch', 'TodoWrite', 'NotebookEdit']

function Card({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section className="ct-card flex flex-col gap-4 p-5">
      {title && <h2 className="text-[15px] font-semibold">{title}</h2>}
      {children}
    </section>
  )
}

/** The mockup's toggle chips: pressed ones are in the list. */
function Toggles({ label, options, value, onChange, mono }: { label: string; options: string[]; value: string[]; onChange: (v: string[]) => void; mono?: boolean }) {
  const all = [...options, ...value.filter((v) => !options.includes(v))]
  return (
    <div role="group" aria-label={label} className="flex flex-wrap gap-2">
      {all.map((o) => {
        const on = value.includes(o)
        return (
          <button
            key={o}
            type="button"
            aria-pressed={on}
            onClick={() => onChange(on ? value.filter((v) => v !== o) : [...value, o])}
            className={`inline-flex min-h-9 items-center gap-1.5 rounded-[18px] border px-3.5 text-[13px] font-medium ${mono ? 'ct-mono' : ''}`}
            style={
              on
                ? { background: 'var(--ct-accent-soft)', color: 'var(--ct-accent-text)', borderColor: 'var(--ct-accent-ring)' }
                : { background: 'var(--ct-surface)', color: 'var(--ct-text-3)', borderColor: 'var(--ct-control)' }
            }
          >
            {on && <Check aria-hidden className="size-3.5" strokeWidth={2.4} />}
            {o}
          </button>
        )
      })}
    </div>
  )
}

const draftOf = (a: AgentType): AgentDraft => ({
  description: a.description ?? '',
  model: a.model,
  ask: a.ask,
  tools: [...a.tools],
  mcp: [...a.mcp],
  instructions: a.instructions ?? '',
  board: a.board,
})

/** A shared agent type, editable; Save writes agents/<id>.yaml. */
export function AgentEditor({ agent, mcpServers, usedBy, onDeleted }: { agent: AgentType; mcpServers: string[]; usedBy: string[]; onDeleted: () => void }) {
  const queryClient = useQueryClient()
  const [draft, setDraft] = useState<AgentDraft>(() => draftOf(agent))
  const [confirmDelete, setConfirmDelete] = useState(false)
  const dirty = JSON.stringify(draft) !== JSON.stringify(draftOf(agent))
  const set = <K extends keyof AgentDraft>(key: K, value: AgentDraft[K]) => setDraft((d) => ({ ...d, [key]: value }))
  const reload = () => {
    void queryClient.invalidateQueries({ queryKey: ['agents'] })
    void queryClient.invalidateQueries({ queryKey: ['workflows'] })
  }
  const save = useMutation<AgentType, AppError>({ mutationFn: () => api.saveAgent(agent.id, draft, false), onSuccess: reload })
  const remove = useMutation<void, AppError>({
    mutationFn: () => api.deleteAgent(agent.id),
    onSuccess: () => {
      reload()
      onDeleted()
    },
  })
  const error = (save.error ?? remove.error)?.message

  return (
    <div className="flex flex-col gap-[22px]">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex flex-col gap-1.5">
          <span className="text-[13px] text-ct-text-3">Agent type</span>
          <h1 className="text-[26px] leading-tight font-bold tracking-[-0.015em]">{agent.id}</h1>
        </div>
        <div className="flex flex-wrap gap-2">
          {dirty && (
            <button type="button" className="ct-btn" onClick={() => (save.reset(), setDraft(draftOf(agent)))}>
              Discard
            </button>
          )}
          <button type="button" className="ct-btn ct-btn-primary" disabled={!dirty || save.isPending} onClick={() => save.mutate()}>
            Save changes
          </button>
        </div>
      </div>
      {error && <p className="text-[13px] text-ct-failed">{error}</p>}

      <Card title="General">
        <div className="grid grid-cols-[repeat(auto-fit,minmax(200px,1fr))] gap-4">
          <label className="flex flex-col gap-1.5">
            <span className="ct-label">Name</span>
            <input className="ct-input ct-mono" readOnly value={agent.id} title="The file name; workflows refer to it" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="ct-label">Model</span>
            <select className="ct-input ct-mono" value={draft.model} onChange={(e) => set('model', e.target.value)}>
              {[...MODELS, ...(MODELS.includes(draft.model) ? [] : [draft.model])].map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </label>
        </div>
        <label className="flex flex-col gap-1.5">
          <span className="ct-label">Description</span>
          <textarea className="ct-input h-auto py-2.5 leading-normal" rows={2} value={draft.description ?? ''} onChange={(e) => set('description', e.target.value)} />
        </label>
      </Card>

      <Card title="Access">
        <div className="flex flex-col gap-2">
          <span className="ct-label">MCP servers it can use</span>
          <Toggles label="MCP servers it can use" options={mcpServers} value={draft.mcp} onChange={(v) => set('mcp', v)} />
          <span className="text-xs text-ct-text-3">
            {draft.mcp.length ? 'Only these; the others are refused.' : 'None: no MCP tool is even loaded, which saves context.'}
            {mcpServers.length === 0 && ' Your servers are listed here once an agent has run.'}
          </span>
        </div>
        <div className="flex flex-col gap-2">
          <span className="ct-label">Tools</span>
          <Toggles label="Tools" options={TOOLS} value={draft.tools} onChange={(v) => set('tools', v)} mono />
          <span className="text-xs text-ct-text-3">{draft.tools.length ? 'Only these.' : "None pressed: all of Claude Code's tools."}</span>
        </div>
        <label className="flex cursor-pointer items-start gap-2.5">
          <input type="checkbox" className="mt-0.5 size-4" checked={draft.board} onChange={(e) => set('board', e.target.checked)} />
          <span className="flex flex-col gap-0.5">
            <span className="font-medium">Posts to the run's board</span>
            <span className="text-xs text-ct-text-3">It can always read the board, when its workflow has one.</span>
          </span>
        </label>
        <fieldset className="flex flex-col gap-2">
          <legend className="mb-2 ct-label">Ask a human before</legend>
          <div className="grid grid-cols-[repeat(auto-fit,minmax(180px,1fr))] gap-2">
            {ASK.map((a) => {
              const on = a.id === draft.ask
              return (
                <label
                  key={a.id}
                  className="flex cursor-pointer items-start gap-2.5 rounded-lg border px-3 py-2.5"
                  style={on ? { background: 'var(--ct-accent-soft)', borderColor: 'var(--ct-accent-ring)' } : { borderColor: 'var(--ct-control)' }}
                >
                  <input type="radio" name={`ask-${agent.id}`} className="mt-0.5" checked={on} onChange={() => set('ask', a.id)} />
                  <span className="flex flex-col gap-0.5">
                    <span className="font-medium">{a.label}</span>
                    <span className="text-xs text-ct-text-3">{a.help}</span>
                  </span>
                </label>
              )
            })}
          </div>
        </fieldset>
      </Card>

      <Card>
        <label className="flex flex-col gap-1.5">
          <span className="ct-label">Instructions</span>
          <textarea className="ct-input h-auto py-2.5 leading-normal" rows={6} value={draft.instructions ?? ''} onChange={(e) => set('instructions', e.target.value)} />
          <span className="text-xs text-ct-text-3">Added to every step this agent type does, before the step's own instructions.</span>
        </label>
      </Card>

      <div className="flex flex-col gap-2 border-t border-ct-border pt-3.5">
        <span className="text-[13px] font-semibold">Danger zone</span>
        <button
          type="button"
          className="ct-btn ct-btn-danger self-start"
          disabled={usedBy.length > 0 || remove.isPending}
          title={usedBy.length ? `Used by ${usedBy.join(', ')}` : undefined}
          onClick={() => setConfirmDelete(true)}
        >
          Delete agent type
        </button>
        {usedBy.length > 0 && <span className="text-xs text-ct-text-3">Used by {usedBy.join(', ')}: change their steps first.</span>}
      </div>
      {confirmDelete && (
        <ConfirmDialog
          title={`Delete the agent type "${agent.id}"?`}
          confirm="Delete agent type"
          danger
          busy={remove.isPending}
          onConfirm={() => remove.mutate(undefined, { onSettled: () => setConfirmDelete(false) })}
          onCancel={() => setConfirmDelete(false)}
        >
          <p>Its file, agents/{agent.id}.yaml, is deleted. No workflow uses it.</p>
        </ConfirmDialog>
      )}
    </div>
  )
}
