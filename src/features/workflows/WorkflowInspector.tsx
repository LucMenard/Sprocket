import { Plus, X } from 'lucide-react'
import type { ReactNode } from 'react'
import { KindIcon } from '@/components/FlowGraph'
import { KINDS, type FlowKind } from '@/lib/flow'
import type { ActionKind, AgentType, OutputType, WorkflowDraft, WorkflowStep } from '@/lib/api'
import { findPlace, readable } from '@/lib/steps'
import type { WorkflowEditing } from './useWorkflowDraft'

function Field({ label, htmlFor, help, children }: { label: string; htmlFor?: string; help?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={htmlFor} className="ct-label">
        {label}
      </label>
      {children}
      {help && <span className="text-xs text-ct-text-3">{help}</span>}
    </div>
  )
}

const output = (id: string) => `steps.${id}.output`
const TYPES: OutputType[] = ['text', 'number', 'yes/no', 'list']
const ACTIONS: { id: ActionKind; label: string; to?: string }[] = [
  { id: 'comment', label: 'Comment on the ticket' },
  { id: 'status', label: 'Move the ticket to a status', to: 'Status' },
  { id: 'slack', label: 'Post to Slack', to: 'Channel' },
  { id: 'other', label: 'Something else' },
]

/** What a step can read at its place, written as it is typed: `steps.triage.output.files`, `item`. */
function Readable({ refs }: { refs: string[] }) {
  if (!refs.length) return null
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-xs text-ct-text-3">It can read</span>
      <div className="flex flex-wrap gap-1">
        {refs.map((r) => (
          <code key={r} className="rounded-md bg-ct-subtle px-1.5 py-0.5 text-[11px] ct-mono">
            {r}
          </code>
        ))}
      </div>
    </div>
  )
}

function AgentSelect({ id, value, agents, onChange }: { id: string; value: string | null; agents: AgentType[]; onChange: (v: string) => void }) {
  return (
    <select id={id} className="ct-input" value={value ?? ''} onChange={(e) => onChange(e.target.value)}>
      {!agents.some((a) => a.id === value) && <option value={value ?? ''}>{value ?? 'Choose one'}</option>}
      {agents.map((a) => (
        <option key={a.id} value={a.id}>
          {a.id} · {a.model}
          {a.shared ? '' : ' (this workflow)'}
          {a.mcp.length ? ` · ${a.mcp.join(', ')}` : ''}
        </option>
      ))}
    </select>
  )
}

/**
 * The selected node's settings, as in the canvas's inspector. Changes go to the draft; Save in the toolbar
 * writes the file, after the same checks as loading it.
 */
export function WorkflowInspector({
  draft,
  editing,
  node,
  agents,
  onSelect,
}: {
  draft: WorkflowDraft
  editing: WorkflowEditing
  node: string
  /** The agent types a step can use: shared ones and this workflow's own */
  agents: AgentType[]
  onSelect: (id: string) => void
}) {
  const place = findPlace(draft.steps, node)
  const step = place?.step
  const kind: FlowKind = node === '$trigger' ? 'trigger' : node === '$end' ? 'end' : (step?.kind ?? 'agent')
  const title = node === '$trigger' ? 'Tracket' : node === '$end' ? 'Done' : node
  const vis = step ? readable(draft.steps, step.id) : { steps: [], items: [] }
  const sources = vis.steps.filter((s) => s.kind !== 'condition')
  const refs = [
    ...sources.flatMap((s) => (s.kind === 'agent' && s.output.length ? s.output.map((f) => `${output(s.id)}.${f.name}`) : [output(s.id)])),
    ...vis.items,
  ]
  const set = (change: Partial<WorkflowStep>) => step && editing.setStep(step.id, change)
  const addInside = (branch: 'then' | 'else' | 'steps') => {
    if (!step) return
    const id = editing.addStep(agents[0] ? 'agent' : 'approval', { parent: step.id, branch, index: step[branch].length }, agents[0]?.id ?? null)
    if (id) onSelect(id)
  }
  const sourceSelect = (id: string, value: string | null, none: string, onChange: (v: string | null) => void) => (
    <select id={id} className="ct-input" value={value ?? ''} onChange={(e) => onChange(e.target.value || null)}>
      <option value="">{none}</option>
      {value && !sources.some((s) => output(s.id) === value) && <option value={value}>{value}</option>}
      {sources.map((s) => (
        <option key={s.id} value={output(s.id)}>
          The output of {s.id}
        </option>
      ))}
    </select>
  )

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-2.5">
        <KindIcon kind={kind} />
        <span className="flex flex-col gap-0.5">
          <span className="ct-eyebrow">{KINDS[kind].label}</span>
          <span className="text-base font-bold">{title}</span>
        </span>
      </div>

      {node === '$trigger' && (
        <>
          <Field label="Connector" help="Jira events and schedules come later.">
            <input className="ct-input" readOnly value="Tracket (manual only)" aria-label="Connector" />
          </Field>
          <label className="flex cursor-pointer items-start gap-2.5">
            <input type="checkbox" className="mt-0.5 size-4" checked={draft.offerInTracket} onChange={(e) => editing.setField('offerInTracket', e.target.checked)} />
            <span className="flex flex-col gap-0.5">
              <span className="font-medium">Offer it in Tracket</span>
              <span className="text-xs text-ct-text-3">In the ticket's Agents menu</span>
            </span>
          </label>
        </>
      )}

      {node === '$end' && (
        <Field label="Mark the run as" help="Every step done: the run passes. A step that fails, or a rejection with nowhere to go back to, fails it.">
          <input className="ct-input" readOnly value="Passed" aria-label="Mark the run as" />
        </Field>
      )}

      {step && place && (
        <>
          <Field label="Name" htmlFor="n-id" help="Lowercase letters, digits, - or _. Other steps read it by this name.">
            <input
              id="n-id"
              className="ct-input ct-mono"
              value={step.id}
              onChange={(e) => {
                const id = e.target.value.toLowerCase().replace(/[^a-z0-9_-]/g, '-')
                if (!id || findPlace(draft.steps, id)) return
                editing.setStep(step.id, { id })
                onSelect(id)
              }}
            />
          </Field>

          {step.kind === 'agent' && (
            <>
              <Field label="Agent type" htmlFor="n-agent">
                <AgentSelect id="n-agent" value={step.agent} agents={agents} onChange={(agent) => set({ agent })} />
              </Field>
              <Field label="Input" htmlFor="n-input" help="What the agent gets besides the ticket.">
                {sourceSelect('n-input', step.input, 'The ticket only', (input) => set({ input }))}
              </Field>
              <Field label="Extra instructions for this step" htmlFor="n-instr" help={<>Use {'{{ … }}'} to put in what it can read.</>}>
                <textarea id="n-instr" rows={5} className="ct-input h-auto py-2.5 leading-normal" value={step.instructions ?? ''} onChange={(e) => set({ instructions: e.target.value })} />
              </Field>
              <fieldset className="flex flex-col gap-2">
                <legend className="mb-1.5 ct-label">Output</legend>
                <div role="group" aria-label="Output" className="flex gap-1.5">
                  {[
                    { json: false, label: 'Text' },
                    { json: true, label: 'JSON fields' },
                  ].map((o) => (
                    <button
                      key={o.label}
                      type="button"
                      className="ct-chip"
                      aria-pressed={o.json === step.output.length > 0}
                      onClick={() => set({ output: o.json ? (step.output.length ? step.output : [{ name: 'summary', type: 'text' }]) : [] })}
                    >
                      {o.label}
                    </button>
                  ))}
                </div>
                {step.output.map((f, i) => (
                  <div key={i} className="flex gap-1.5">
                    <input
                      aria-label="Field name"
                      className="ct-input h-9 flex-1 ct-mono"
                      value={f.name}
                      onChange={(e) => set({ output: step.output.map((o, j) => (j === i ? { ...o, name: e.target.value.toLowerCase().replace(/[^a-z0-9_-]/g, '_') } : o)) })}
                    />
                    <select
                      aria-label="Field type"
                      className="ct-input h-9 w-28"
                      value={f.type}
                      onChange={(e) => set({ output: step.output.map((o, j) => (j === i ? { ...o, type: e.target.value as OutputType } : o)) })}
                    >
                      {TYPES.map((t) => (
                        <option key={t}>{t}</option>
                      ))}
                    </select>
                    <button type="button" className="ct-btn ct-btn-ghost h-9 px-2" aria-label={`Remove ${f.name}`} onClick={() => set({ output: step.output.filter((_, j) => j !== i) })}>
                      <X className="size-4" />
                    </button>
                  </div>
                ))}
                {step.output.length > 0 && (
                  <button
                    type="button"
                    className="ct-btn ct-btn-sm self-start"
                    onClick={() => set({ output: [...step.output, { name: `field_${step.output.length + 1}`, type: 'text' }] })}
                  >
                    <Plus className="size-3.5" /> Add a field
                  </button>
                )}
                <span className="text-xs text-ct-text-3">
                  {step.output.length
                    ? `Conditions and loops can read each field: ${output(step.id)}.${step.output[0].name}`
                    : 'The agent answers in text. JSON fields let conditions and loops read its result.'}
                </span>
              </fieldset>
            </>
          )}

          {step.kind === 'approval' && (
            <>
              <Field label="Shows" htmlFor="n-shows" help="What you read before approving.">
                {sourceSelect('n-shows', step.input, "Just the step's name", (input) => set({ input }))}
              </Field>
              <Field label="Approvers">
                <input className="ct-input" readOnly value="You, in Sprocket or in Tracket" aria-label="Approvers" />
              </Field>
              <Field label="If rejected" htmlFor="n-reject" help="Going back sends your reason to that step's agent.">
                <select id="n-reject" className="ct-input" value={step.onReject ?? ''} onChange={(e) => set({ onReject: e.target.value || null })}>
                  <option value="">{place.loops.length ? 'This item fails' : 'The run fails'}</option>
                  {place.block.slice(0, place.index).map((s) => (
                    <option key={s.id} value={s.id}>
                      Back to {s.id}
                    </option>
                  ))}
                </select>
              </Field>
            </>
          )}

          {step.kind === 'action' && (
            <>
              <Field label="Do" htmlFor="n-do">
                <select
                  id="n-do"
                  className="ct-input"
                  value={step.action?.do ?? 'comment'}
                  onChange={(e) => set({ action: { text: '', ...step.action, do: e.target.value as ActionKind } })}
                >
                  {ACTIONS.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.label}
                    </option>
                  ))}
                </select>
              </Field>
              {ACTIONS.find((a) => a.id === (step.action?.do ?? 'comment'))?.to && (
                <Field label={ACTIONS.find((a) => a.id === step.action?.do)!.to!} htmlFor="n-to">
                  <input
                    id="n-to"
                    className="ct-input"
                    placeholder={step.action?.do === 'slack' ? '#team-channel' : 'In review'}
                    value={step.action?.to ?? ''}
                    onChange={(e) => set({ action: { do: 'comment', text: '', ...step.action, to: e.target.value } })}
                  />
                </Field>
              )}
              <Field
                label={step.action?.do === 'other' ? 'What to do' : step.action?.do === 'status' ? 'Note (optional)' : 'Message'}
                htmlFor="n-msg"
                help={<>Use {'{{ … }}'} to put in what it can read.</>}
              >
                <textarea
                  id="n-msg"
                  rows={4}
                  className="ct-input h-auto py-2.5 leading-normal"
                  value={step.action?.text ?? ''}
                  onChange={(e) => set({ action: { do: 'comment', ...step.action, text: e.target.value } })}
                />
              </Field>
              <Field label="Done by" htmlFor="n-actor" help="A small agent does it with its MCP servers: give it Jira's, or Slack's.">
                <AgentSelect id="n-actor" value={step.agent} agents={agents} onChange={(agent) => set({ agent })} />
              </Field>
            </>
          )}

          {step.kind === 'condition' && (
            <>
              <Field label="Expression" htmlFor="n-expr" help="==, !=, <, >, contains, and, or, not. Text compares without case.">
                <textarea id="n-expr" rows={2} className="ct-input h-auto py-2.5 leading-normal ct-mono text-[13px]" value={step.if ?? ''} onChange={(e) => set({ if: e.target.value })} />
              </Field>
              <div className="flex flex-col gap-2">
                <span className="ct-label">Branches</span>
                {(['then', 'else'] as const).map((b) => (
                  <div key={b} className="flex min-h-10 items-center justify-between gap-2 rounded-lg border border-ct-border px-3">
                    <span className="font-semibold">{b === 'then' ? 'yes' : 'no'}</span>
                    <span className="flex items-center gap-1.5 text-[13px] text-ct-text-3">
                      {step[b][0] ? (
                        <button type="button" className="hover:underline" onClick={() => onSelect(step[b][0].id)}>
                          → {step[b][0].id}
                          {step[b].length > 1 ? ` +${step[b].length - 1}` : ''}
                        </button>
                      ) : (
                        '→ goes on'
                      )}
                      <button type="button" className="ct-btn ct-btn-ghost h-7 px-1.5" aria-label={`Add a step to ${b === 'then' ? 'yes' : 'no'}`} title="Add a step" onClick={() => addInside(b)}>
                        <Plus className="size-4" />
                      </button>
                    </span>
                  </div>
                ))}
              </div>
            </>
          )}

          {step.kind === 'forEach' && (
            <>
              <Field label="Items to loop over" htmlFor="n-items" help="A list, such as a JSON field of type list.">
                <input id="n-items" className="ct-input ct-mono text-[13px]" value={step.forEach ?? ''} onChange={(e) => set({ forEach: e.target.value })} />
              </Field>
              <Field label="Name for each item" htmlFor="n-var">
                <input
                  id="n-var"
                  className="ct-input ct-mono text-[13px]"
                  value={step.as ?? 'item'}
                  onChange={(e) => set({ as: e.target.value.toLowerCase().replace(/[^a-z0-9_-]/g, '_') })}
                />
              </Field>
              <div className="grid grid-cols-2 gap-3">
                <Field label="Run" htmlFor="n-mode">
                  <select id="n-mode" className="ct-input" value={step.parallel > 1 ? 'parallel' : 'one'} onChange={(e) => set({ parallel: e.target.value === 'parallel' ? Math.max(2, step.parallel) : 1 })}>
                    <option value="parallel">In parallel</option>
                    <option value="one">One at a time</option>
                  </select>
                </Field>
                <Field label="Max at once" htmlFor="n-par">
                  <input
                    id="n-par"
                    type="number"
                    min={1}
                    max={10}
                    className="ct-input"
                    disabled={step.parallel <= 1}
                    value={step.parallel}
                    onChange={(e) => set({ parallel: Math.min(10, Math.max(1, Number(e.target.value) || 1)) })}
                  />
                </Field>
              </div>
              {step.parallel > 1 && <p className="text-xs text-ct-text-3">The items share the ticket's worktree: run agents that edit files one at a time.</p>}
              <Field label="If an item fails" htmlFor="n-ifail">
                <select id="n-ifail" className="ct-input" value={step.onItemFailure} onChange={(e) => set({ onItemFailure: e.target.value as 'stop' | 'continue' })}>
                  <option value="continue">Continue with the rest</option>
                  <option value="stop">Stop the loop</option>
                </select>
              </Field>
              <div className="rounded-lg p-3 text-[13px] leading-normal" style={{ background: 'var(--loop-bg)' }}>
                <strong className="font-semibold">Loop body:</strong> {step.steps.map((s) => s.id).join(', ') || 'empty for now'}. Steps inside can use{' '}
                <code className="text-xs ct-mono">{`{{ ${step.as || 'item'} }}`}</code>.
              </div>
              <button type="button" className="ct-btn ct-btn-sm self-start" onClick={() => addInside('steps')}>
                <Plus className="size-3.5" /> Add a step inside
              </button>
            </>
          )}

          <Readable refs={refs} />

          <div className="flex flex-wrap gap-2 border-t border-ct-border pt-3.5">
            <button type="button" className="ct-btn ct-btn-sm" disabled={place.index === 0} onClick={() => editing.moveStep(step.id, -1)}>
              Move up
            </button>
            <button type="button" className="ct-btn ct-btn-sm" disabled={place.index === place.block.length - 1} onClick={() => editing.moveStep(step.id, 1)}>
              Move down
            </button>
            <button
              type="button"
              className="ct-btn ct-btn-sm ct-btn-danger ml-auto"
              disabled={draft.steps.length === 1 && !place.parent}
              title={draft.steps.length === 1 && !place.parent ? 'A workflow needs at least one step' : undefined}
              onClick={() => {
                editing.removeStep(step.id)
                onSelect(place.block[place.index - 1]?.id ?? place.parent?.id ?? '$trigger')
              }}
            >
              Delete node
            </button>
          </div>
        </>
      )}
    </div>
  )
}
