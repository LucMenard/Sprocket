import type { ReactNode } from 'react'
import { KindIcon, KINDS, type FlowKind } from '@/components/FlowGraph'
import type { Workflow } from '@/lib/api'

const ASK = {
  never: 'Never: runs on its own',
  'before-writes': 'Before writes: reads freely, asks before changing things',
  'every-action': 'Every action: asks before each tool call',
}

/** A field as the mockup's inspector draws it, shown read-only. */
function Field({ label, children, mono = false, tall = false }: { label: string; children: ReactNode; mono?: boolean; tall?: boolean }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="ct-label">{label}</span>
      <div
        className={`rounded-lg border border-ct-control bg-ct-subtle px-3 ${tall ? 'py-2.5 leading-normal' : 'flex min-h-10 items-center'} ${mono ? 'text-[13px] ct-mono' : ''}`}
      >
        {children}
      </div>
    </div>
  )
}

/** The selected node's settings, as the workflow file has them. */
export function WorkflowInspector({ workflow, node }: { workflow: Workflow; node: string }) {
  const step = workflow.steps.find((s) => s.id === node)
  const index = step ? workflow.steps.indexOf(step) : -1
  const kind: FlowKind = node === '$trigger' ? 'trigger' : node === '$end' ? 'end' : (step?.kind ?? 'agent')
  const title = node === '$trigger' ? 'Tracket' : node === '$end' ? 'Done' : node
  const agent = step?.agent ? workflow.agents.find((a) => a.id === step.agent) : undefined

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
          <Field label="Connector">Tracket (manual only)</Field>
          <Field label="Offer it in Tracket" tall>
            {workflow.offerInTracket ? "Yes: in the ticket's Agents menu" : 'No: start it from Sprocket'}
          </Field>
          <p className="text-xs text-ct-text-3">Jira events and schedules as triggers come later.</p>
        </>
      )}

      {node === '$end' && <Field label="Mark the run as">Passed</Field>}

      {step?.kind === 'agent' && (
        <>
          <Field label="Agent type">{step.agent}</Field>
          {agent && (
            <>
              <Field label="Model" mono>
                {agent.model}
              </Field>
              <Field label="Ask a human" tall>
                {ASK[agent.ask]}
              </Field>
            </>
          )}
          <Field label="Input" mono>
            {step.input ?? 'the ticket'}
          </Field>
          <Field label="Extra instructions for this step" tall>
            {step.instructions ?? 'None'}
          </Field>
        </>
      )}

      {step?.kind === 'approval' && (
        <>
          <Field label="Shows" mono>
            {step.input ?? 'the step name'}
          </Field>
          <Field label="Approvers">You, in Sprocket or in Tracket</Field>
          <Field label="If rejected" tall>
            {step.onReject ? `Back to ${step.onReject}, with the reason` : 'The run fails'}
          </Field>
        </>
      )}

      {step && (
        <div className="flex flex-col gap-1.5">
          <span className="ct-label">Links</span>
          <p className="text-[13px] text-ct-text-2">
            {index > 0 ? `After ${workflow.steps[index - 1].id}` : 'After the trigger'} ·{' '}
            {index + 1 < workflow.steps.length ? `then ${workflow.steps[index + 1].id}` : 'then the end'}
          </p>
        </div>
      )}
    </div>
  )
}
