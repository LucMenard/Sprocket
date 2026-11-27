import { Plus, X } from 'lucide-react'
import type { ReactNode } from 'react'
import { KindIcon } from '@/components/FlowGraph'
import { edgeKey, KINDS, type Flow, type FlowEdge, type FlowKind } from '@/lib/flow'
import type { ActionKind, AgentType, OutputType, WorkflowDraft, WorkflowStep } from '@/lib/api'
import { findPlace, readable } from '@/lib/steps'
import { TriggersPanel } from './TriggersPanel'
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
 * The node's links, as the canvas's inspector lists them: in and out, with the other node and the label. The
 * ones that can go have a delete button (a rejection, a parallel branch, a condition's branch); the others follow
 * from the order of the steps. "Link to" does what dragging a port does.
 */
function Links({
  flow,
  node,
  editing,
  onSelect,
  onSelectLink,
  onLinkError,
}: {
  flow: Flow
  node: string
  editing: WorkflowEditing
  onSelect: (id: string) => void
  onSelectLink: (key: string) => void
  onLinkError: (why: string | null) => void
}) {
  const title = (id: string) => flow.nodes.find((n) => n.id === id)?.title ?? id
  const links = flow.edges.filter((e) => e.from === node || e.to === node)
  const targets = flow.nodes.filter((n) => n.id !== node && n.id !== '$trigger' && n.id !== '$end')
  return (
    <div className="flex flex-col gap-2 border-t border-ct-border pt-3.5">
      <span className="text-[13px] font-semibold">Links</span>
      {links.length === 0 && <p className="text-[13px] text-ct-text-3">Not connected yet. Drag from a port to link it.</p>}
      <ul className="flex flex-col gap-1">
        {links.map((e, i) => {
          const out = e.from === node
          const other = out ? e.to : e.from
          return (
            <li key={`${e.from}-${e.to}-${i}`} className="flex min-h-8 items-center gap-2 text-[13px]">
              <span className="rounded-[10px] bg-ct-subtle px-2 py-px text-[11px] font-semibold text-ct-text-3">{out ? 'out' : 'in'}</span>
              <button type="button" className="min-w-0 flex-1 truncate text-left hover:underline" title="Select this link" onClick={() => onSelectLink(edgeKey(e))}>
                {title(other)}
              </button>
              {e.label && <span className="text-ct-text-3">{e.label}</span>}
              <button
                type="button"
                className="ct-btn ct-btn-ghost h-7 px-1.5"
                aria-label={`Delete the link ${out ? 'to' : 'from'} ${title(other)}`}
                title={
                  e.link?.type === 'reject'
                    ? 'No going back on a rejection'
                    : e.link?.type === 'next'
                      ? 'Unlink it: it and the steps after it wait until you link them again'
                      : e.link
                        ? 'Run it after, instead of alongside'
                        : 'Delete it: link them again before saving'
                }
                onClick={() => onLinkError(editing.deleteLink(e))}
              >
                <X className="size-3.5" />
              </button>
            </li>
          )
        })}
      </ul>
      {node !== '$end' && (
        <select
          aria-label="Link to another node"
          className="ct-input h-9"
          value=""
          onChange={(ev) => {
            if (!ev.target.value) return
            const why = editing.connect(node, ev.target.value)
            onLinkError(why)
            if (!why) onSelect(ev.target.value)
          }}
        >
          <option value="">Link to…</option>
          {targets.map((n) => (
            <option key={n.id} value={n.id}>
              {n.title}
            </option>
          ))}
        </select>
      )}
      <span className="text-xs text-ct-text-3">
        A node linked after this one runs next; with two or more, they run at the same time and the next step waits for all of them.
      </span>
    </div>
  )
}

/** The selected link: where it goes, what it is, and deleting it. */
function LinkPanel({ flow, link, onSelect, onDelete }: { flow: Flow; link: FlowEdge; onSelect: (id: string) => void; onDelete: () => void }) {
  const title = (id: string) => flow.nodes.find((n) => n.id === id)?.title ?? id
  const what = link.invalid
    ? "The workflow can't hold this link, so it can't be saved while it is here. Delete it."
    : link.link?.type === 'reject'
      ? 'Where a rejection sends the run back to.'
      : link.link?.type === 'lane'
        ? 'A parallel branch: it runs at the same time as the others. Deleting it runs it after them instead.'
        : link.link?.type === 'branch'
          ? "A condition's branch. Deleting it runs the branch's steps right after the condition."
          : link.link
            ? 'Deleting it unlinks the next node and the ones after it, until you link them again.'
            : 'It follows from the other links: when the steps before it are done, the run goes on. Deleting it leaves a gap to link again before saving.'
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-0.5">
        <span className="ct-eyebrow">Link</span>
        <span className="text-base font-bold">
          {title(link.from)} → {title(link.to)}
        </span>
        {link.label && <span className="text-[13px] text-ct-text-3">{link.label}</span>}
      </div>
      <p className="text-[13px] leading-normal text-ct-text-2">{what}</p>
      <div className="flex flex-wrap gap-2">
        <button type="button" className="ct-btn ct-btn-sm" onClick={() => onSelect(link.from)}>
          Go to {title(link.from)}
        </button>
        <button type="button" className="ct-btn ct-btn-sm" onClick={() => onSelect(link.to)}>
          Go to {title(link.to)}
        </button>
        <button type="button" className="ct-btn ct-btn-sm ct-btn-danger ml-auto" onClick={onDelete}>
          Delete link
        </button>
      </div>
      <p className="text-xs text-ct-text-3">Del deletes it · Esc goes back to the node · Ctrl+Z undoes</p>
    </div>
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
  launchable,
  workflowId,
  dirty,
  flow,
  link,
  onSelect,
  onSelectLink,
  onDeleteLink,
  onLinkError,
}: {
  draft: WorkflowDraft
  editing: WorkflowEditing
  node: string
  /** The agent types a step can use: shared ones and this workflow's own */
  agents: AgentType[]
  /** The workflows a launch step can run: all but this one */
  launchable: { id: string; name: string; forRequests: boolean }[]
  /** The saved workflow's id, for Run now and the triggers' last runs */
  workflowId: string | null
  dirty: boolean
  /** The graph as drawn, for the node's links */
  flow: Flow
  /** A selected link: shown instead of the node */
  link?: FlowEdge
  onSelect: (id: string) => void
  onSelectLink: (key: string) => void
  onDeleteLink: () => void
  onLinkError: (why: string | null) => void
}) {
  if (link) return <LinkPanel flow={flow} link={link} onSelect={onSelect} onDelete={onDeleteLink} />
  // Linked or not: a node waiting to be linked again has its settings too
  const findAnywhere = (id: string) => findPlace(draft.steps, id) ?? draft.loose.reduce<ReturnType<typeof findPlace>>((f, chain) => f ?? findPlace(chain, id), null)
  const place = findAnywhere(node)
  const step = place?.step
  const kind: FlowKind = node === '$trigger' ? 'trigger' : node === '$end' ? 'end' : (step?.kind ?? 'agent')
  const title = node === '$trigger' ? 'Start' : node === '$end' ? 'Done' : node
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
          <span className="ct-label">From Tracket</span>
          <label className="flex cursor-pointer items-start gap-2.5">
            <input type="checkbox" className="mt-0.5 size-4" checked={draft.offerInTracket} onChange={(e) => editing.setField('offerInTracket', e.target.checked)} />
            <span className="flex flex-col gap-0.5">
              <span className="font-medium">Offer it in Tracket</span>
              <span className="text-xs text-ct-text-3">In the ticket's Agents menu</span>
            </span>
          </label>
          <label className="flex cursor-pointer items-start gap-2.5">
            <input type="checkbox" className="mt-0.5 size-4" checked={draft.forRequests} onChange={(e) => editing.setField('forRequests', e.target.checked)} />
            <span className="flex flex-col gap-0.5">
              <span className="font-medium">Runs what you ask</span>
              <span className="text-xs text-ct-text-3">Tracket's "Ask an agent" starts it with what you typed; steps read it as {'{{ request }}'}</span>
            </span>
          </label>
          <TriggersPanel draft={draft} editing={editing} workflowId={workflowId} dirty={dirty} launchable={launchable} />
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
                if (!id || findAnywhere(id)) return
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

          {step.kind === 'launch' && (
            <>
              <Field
                label="Workflow"
                htmlFor="n-launch"
                help="This step waits for it, and fails if it fails; a retry goes on with it. At that workflow's limit of runs, it waits for room."
              >
                <select id="n-launch" className="ct-input" value={step.launch ?? ''} onChange={(e) => set({ launch: e.target.value || null })}>
                  <option value="">Choose a workflow</option>
                  {step.launch && !launchable.some((w) => w.id === step.launch) && <option value={step.launch}>{step.launch} (not found)</option>}
                  {launchable.map((w) => (
                    <option key={w.id} value={w.id}>
                      {w.name}
                    </option>
                  ))}
                </select>
              </Field>
              <Field
                label="Ticket"
                htmlFor="n-ticket"
                help={
                  <>
                    Optional. Left empty, it runs on this run's ticket and worktree. Set, for example {'{{ subtask }}'} in a loop, it runs on that ticket
                    (its key first: SIS-301, or SIS-301 and its title) in that ticket's worktree, made as Tracket makes it.
                  </>
                }
              >
                <input id="n-ticket" className="ct-input ct-mono" value={step.ticket ?? ''} onChange={(e) => set({ ticket: e.target.value || null })} />
              </Field>
              <Field
                label="What it is asked"
                htmlFor="n-ask"
                help={
                  <>
                    Its steps read it as {'{{ request }}'}. Use {'{{ … }}'} to put in what this step can read; left empty, it gets this run's request,
                    if any.
                    {launchable.find((w) => w.id === step.launch)?.forRequests && ' That workflow runs what it is asked: it needs one.'}
                  </>
                }
              >
                <textarea id="n-ask" rows={4} className="ct-input h-auto py-2.5 leading-normal" value={step.input ?? ''} onChange={(e) => set({ input: e.target.value || null })} />
              </Field>
              <p className="text-xs text-ct-text-3">Later steps read what its last step gave as {output(step.id)}.</p>
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

          {step.kind === 'team' && step.team && (
            <>
              <div className="flex flex-col gap-1.5">
                <span className="ct-label">Members</span>
                <div role="group" aria-label="Members" className="flex flex-wrap gap-1.5">
                  {agents.map((a) => {
                    const t = step.team!
                    const on = t.members.includes(a.id)
                    return (
                      <button
                        key={a.id}
                        type="button"
                        className="ct-chip"
                        aria-pressed={on}
                        onClick={() => {
                          const members = on ? t.members.filter((m) => m !== a.id) : [...t.members, a.id]
                          set({
                            team: {
                              ...t,
                              members,
                              lead: members.includes(t.lead) ? t.lead : (members[0] ?? ''),
                              writer: t.writer && members.includes(t.writer) ? t.writer : null,
                            },
                          })
                        }}
                      >
                        {a.id}
                      </button>
                    )
                  })}
                </div>
                <span className="text-xs text-ct-text-3">Two or more. They work at the same time and talk on the run's board.</span>
              </div>
              <div className="grid grid-cols-2 gap-3">
                <Field label="Lead" htmlFor="n-lead" help="Ends the step with its result.">
                  <select id="n-lead" className="ct-input" value={step.team.lead} onChange={(e) => set({ team: { ...step.team!, lead: e.target.value } })}>
                    {step.team.members.map((m) => (
                      <option key={m}>{m}</option>
                    ))}
                  </select>
                </Field>
                <Field label="Changes files" htmlFor="n-writer" help="The others read and advise.">
                  <select
                    id="n-writer"
                    className="ct-input"
                    value={step.team.writer ?? ''}
                    onChange={(e) => set({ team: { ...step.team!, writer: e.target.value || null } })}
                  >
                    <option value="">Nobody</option>
                    {step.team.members.map((m) => (
                      <option key={m}>{m}</option>
                    ))}
                  </select>
                </Field>
              </div>
              <Field label="Goal" htmlFor="n-goal" help={<>What the team works out. Use {'{{ … }}'} to put in what it can read.</>}>
                <textarea
                  id="n-goal"
                  rows={4}
                  className="ct-input h-auto py-2.5 leading-normal"
                  value={step.team.goal}
                  onChange={(e) => set({ team: { ...step.team!, goal: e.target.value } })}
                />
              </Field>
              <div className="flex flex-col gap-1.5">
                <span className="ct-label">Stop rules, for each member</span>
                <div className="grid grid-cols-3 gap-2">
                  {(
                    [
                      ['maxTurns', 'turns', 1],
                      ['budgetUsd', '$', 0.1],
                      ['minutes', 'minutes', 1],
                    ] as const
                  ).map(([key, label, min]) => (
                    <label key={key} className="flex flex-col gap-1 text-xs text-ct-text-3">
                      <input
                        type="number"
                        min={min}
                        max={draft.teamLimits[key]}
                        step={min}
                        className="ct-input h-9"
                        value={step.team!.limits[key]}
                        onChange={(e) =>
                          set({ team: { ...step.team!, limits: { ...step.team!.limits, [key]: Math.min(draft.teamLimits[key], Math.max(min, Number(e.target.value) || min)) } } })
                        }
                      />
                      {label} (≤ {draft.teamLimits[key]})
                    </label>
                  ))}
                </div>
                <span className="text-xs text-ct-text-3">
                  Reaching one stops the team and asks you: go on with the same limits again, or fail the step. Time waiting for you doesn't count.
                </span>
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
          <Links flow={flow} node={node} editing={editing} onSelect={onSelect} onSelectLink={onSelectLink} onLinkError={onLinkError} />

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
