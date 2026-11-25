import type { ReactNode } from 'react'
import { KindIcon, KINDS, type FlowKind } from '@/components/FlowGraph'
import type { AgentType, WorkflowDraft } from '@/lib/api'
import type { WorkflowEditing } from './useWorkflowDraft'

function Field({ label, htmlFor, help, children }: { label: string; htmlFor?: string; help?: string; children: ReactNode }) {
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

/**
 * The selected node's settings, editable as in the mockup's inspector. Changes go to the draft;
 * Save in the toolbar writes the file.
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
  const index = draft.steps.findIndex((s) => s.id === node)
  const step = draft.steps[index]
  const earlier = draft.steps.slice(0, Math.max(index, 0))
  const earlierAgents = earlier.filter((s) => s.kind === 'agent')
  const kind: FlowKind = node === '$trigger' ? 'trigger' : node === '$end' ? 'end' : (step?.kind ?? 'agent')
  const title = node === '$trigger' ? 'Tracket' : node === '$end' ? 'Done' : node

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
            <input
              type="checkbox"
              className="mt-0.5 size-4"
              checked={draft.offerInTracket}
              onChange={(e) => editing.setField('offerInTracket', e.target.checked)}
            />
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

      {step && (
        <>
          <Field label="Name" htmlFor="n-id" help="Lowercase letters, digits, - or _. Other steps refer to it by this name.">
            <input
              id="n-id"
              className="ct-input ct-mono"
              value={step.id}
              onChange={(e) => {
                const id = e.target.value.toLowerCase().replace(/[^a-z0-9_-]/g, '-')
                if (!id || draft.steps.some((s) => s.id === id && s !== step)) return
                editing.setStep(step.id, { id })
                onSelect(id)
              }}
            />
          </Field>

          {step.kind === 'agent' ? (
            <>
              <Field label="Agent type" htmlFor="n-agent">
                <select id="n-agent" className="ct-input" value={step.agent ?? ''} onChange={(e) => editing.setStep(step.id, { agent: e.target.value })}>
                  {!agents.some((a) => a.id === step.agent) && <option value={step.agent ?? ''}>{step.agent ?? 'Choose one'}</option>}
                  {agents.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.id} · {a.model}
                      {a.shared ? '' : ' (this workflow)'}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label="Input" htmlFor="n-input" help="What the agent gets besides the ticket.">
                <select
                  id="n-input"
                  className="ct-input"
                  value={step.input ?? ''}
                  onChange={(e) => editing.setStep(step.id, { input: e.target.value || null })}
                >
                  <option value="">The ticket only</option>
                  {earlierAgents.map((s) => (
                    <option key={s.id} value={output(s.id)}>
                      The output of {s.id}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label="Extra instructions for this step" htmlFor="n-instr">
                <textarea
                  id="n-instr"
                  rows={5}
                  className="ct-input h-auto py-2.5 leading-normal"
                  value={step.instructions ?? ''}
                  onChange={(e) => editing.setStep(step.id, { instructions: e.target.value })}
                />
              </Field>
            </>
          ) : (
            <>
              <Field label="Shows" htmlFor="n-shows" help="What you read before approving.">
                <select id="n-shows" className="ct-input" value={step.input ?? ''} onChange={(e) => editing.setStep(step.id, { input: e.target.value || null })}>
                  <option value="">Just the step's name</option>
                  {earlierAgents.map((s) => (
                    <option key={s.id} value={output(s.id)}>
                      The output of {s.id}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label="Approvers">
                <input className="ct-input" readOnly value="You, in Sprocket or in Tracket" aria-label="Approvers" />
              </Field>
              <Field label="If rejected" htmlFor="n-reject" help="Going back sends your reason to that step's agent.">
                <select id="n-reject" className="ct-input" value={step.onReject ?? ''} onChange={(e) => editing.setStep(step.id, { onReject: e.target.value || null })}>
                  <option value="">The run fails</option>
                  {earlier.map((s) => (
                    <option key={s.id} value={s.id}>
                      Back to {s.id}
                    </option>
                  ))}
                </select>
              </Field>
            </>
          )}

          <div className="flex flex-wrap gap-2 border-t border-ct-border pt-3.5">
            <button type="button" className="ct-btn ct-btn-sm" disabled={index === 0} onClick={() => editing.moveStep(step.id, -1)}>
              Move up
            </button>
            <button type="button" className="ct-btn ct-btn-sm" disabled={index === draft.steps.length - 1} onClick={() => editing.moveStep(step.id, 1)}>
              Move down
            </button>
            <button
              type="button"
              className="ct-btn ct-btn-sm ct-btn-danger ml-auto"
              disabled={draft.steps.length === 1}
              title={draft.steps.length === 1 ? 'A workflow needs at least one step' : undefined}
              onClick={() => {
                editing.removeStep(step.id)
                onSelect(draft.steps[index - 1]?.id ?? '$trigger')
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
