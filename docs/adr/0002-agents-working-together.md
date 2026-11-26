# 0002: Agents work together through a run's board, colleague asks and team steps

Date: 2026-11-25. Status: accepted.

## Context

Today every agent in a run works alone. It gets the ticket, the input it is given (`steps.<id>.output`) and, after a
rejection, the reason; it cannot leave a remark for a later step, ask another agent type a question, or share a step
with other agents. Real work on a ticket often needs exactly that: a builder asks the planner what a line of the plan
meant, a reviewer and a builder go back and forth until the change is right.

What the engine has since the richer workflow engine (conditions, For each, actions, JSON outputs):

- several agent sessions of one run can run at the same time (a For each with `parallel` > 1), each a job with its
  own scope;
- the core works out what runs next from the job rows after every change, so new kinds of step fit in;
- agent steps can return JSON fields, which is how a step hands a real list to a For each (`steps.plan.output.tenants`):
  the agent decides the list, the engine iterates.

## Decision

Three layers, all built on one **board** per run.

### 1. The board (workflow scope)

One shared conversation per run: notes with an author (an agent job, or a person), an optional addressee, a body and
a time. Every agent of the run can `post_note(message, to?)` and `read_notes(since?)`. The user reads it and can post
to it, in Sprocket's Runs screen and in Tracket's Agents section. A rejection's reason is also posted there, so a
later step knows why the run went back.

- Storage: a `notes` table in the core's SQLite (`run_id`, `job_id` or `person`, `to`, `body`, `at`), shown in the run
  view and through `GET /v1/runs/{id}/notes` / `POST /v1/runs/{id}/notes` for Tracket.
- Tools: an in-process MCP server in the agent host (`sprocket`), created with the Agent SDK's `createSdkMcpServer`.
  The host asks the core for every call (new request/answer messages on the NDJSON protocol, with a request id), so the
  core stays the only owner of state. The host's MCP filter and ask policies let the `sprocket` server through:
  `mcp__*` is no longer disallowed wholesale when an agent has no MCP server, and posting a note is not a "write" that
  asks a human.

### 2. Ask a colleague (workflow scope)

A tool `ask_agent(agentType, question)`. The core checks the workflow allows it (who may ask whom, asks per step),
then starts a short session of that agent type as a child job of the asker: same worktree, the run's context and
board, the question as its prompt. Its answer is the tool's result.

- Every ask is a job (`kind: ask`, linked to the asking job) with its own log, cost and transcript, so Runs shows it
  under the asking step.
- No chains: a colleague's session does not get `ask_agent`. Asks per step and a cost cap are enforced by the core.
- The colleague keeps its own ask policy: if it wants to write a file and its policy says ask, you are asked.

### 3. Team step (a step type)

Several agent types work on one step at the same time, over the board:

- `post(message, to?)`, `read()`, `wait_for_reply(timeout?)`: the board tools, plus a wait that returns when a note
  addressed to the member (or to everyone) arrives;
- the lead calls `finish(summary)`: the other members are stopped and the summary is the step's output (text, or the
  step's JSON fields).
- Stop rules: maximum turns per member, a cost budget and a time limit for the step. Hitting one stops the team and
  opens a gate: continue (with a raised limit) or fail the step.
- Members keep their ask policy. Only the members allowed to edit files get Edit, Write and Bash; the others read.
  An approval step after the team checks the result, as after any agent step.

Each member is a job of the team step (scope `<team>/<member>/`), so the scheduler, retries and the Runs view treat it
like the items of a loop.

### Settings

- **Workflow:** board on or off; who may ask whom; run-wide limits (asks per step, cost budget for the run).
- **Agent type:** whether it may ask colleagues and post to the board.
- **Team step:** members, lead, goal, stop rules, who may edit files.

### Build order

1. **Board** (built 2026-11-25): notes table, host↔core request/answer messages, the `sprocket` MCP server with `post_note` and
   `read_notes`, rejection reasons posted, the board in Runs (as a chat) and in Tracket.
2. **ask_agent** (built 2026-11-25): child jobs, limits, the asks shown under their step.
3. **Concurrent sessions in one run** (built 2026-11-25, with the team step): already there for For each items; check what a team needs on top (one
   member's ask waiting for you while the others go on, stopping the others when one finishes).
4. **Team step** (built 2026-11-25): the step kind, `wait_for_reply` and `finish`, stop rules and their gate, file-editing rights.
5. **Editor and Runs UI** (built 2026-11-25): the Team node and its inspector, the members as lanes in Runs, the board as a chat.

## Settled with the user

- **Finishing a team:** the lead decides, with `finish(summary)`. A vote may come later.
- **Files:** one member may edit files; the others read and advise over the board. Branches per member may come later.
- **The user on the board:** posts mid-run, from Sprocket or Tracket; agents see the note at their next read or wait.
- **Stop rules:** both levels. The workflow sets defaults and a run budget; a Team step can tighten them.

## Consequences

- The NDJSON protocol gains request/answer messages from the host to the core (protocol 3).
- Runs get a second kind of history next to jobs: the board, which Tracket also shows.
- A team or colleague ask multiplies agent sessions and cost: limits are part of the feature, not an afterthought.
- Agents with no MCP server now load one tool server (the `sprocket` one), a small context cost; it is left out when
  the workflow turns the board off and the agent may not ask.
