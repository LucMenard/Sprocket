# Sprocket: architecture

Sprocket is a desktop app that runs Claude agents on Jira tickets. It is a companion to [Tracket](../../tracket),
which stays the Jira client. You start work on a ticket from Tracket. Sprocket runs it as a **workflow**: a
sequence of steps, each one done by an agent or waiting for your approval. Tracket shows how the run is going on the
ticket.

The targeted UI is the canvas "Task & Agent Tracker", page V2
(<https://claude.ai/artifact/S7Nf5NYeo2puioSakzyojM>). This document says what we build first and how. The canvas
shows where we are heading.

## Decisions (2026-11-24)

| Topic | Decision |
| --- | --- |
| Shell | Tauri 2 (Rust) + React 19 + TypeScript, the same stack as Tracket (Vite, Tailwind, shadcn, Zustand, TanStack Query). |
| Where the engine runs | **In the desktop app.** The Rust core owns runs and their state. Runs only progress while the app is open; it keeps running in the tray when its window is closed. |
| Agent runtime | Claude Agent SDK (TypeScript) in a Node sidecar supervised by the Rust core. |
| How Tracket talks to it | HTTP on `127.0.0.1`, from Tracket's Rust side, with a token. This is the "Address" and "Sign in" of the mockup's Settings › Sprocket. |
| First slice | Runs started by hand from Tracket, workflows written as YAML files, a Runs view, approvals from either app. |
| Jira writes | None from Sprocket in the first slice. Tracket shows run state by asking Sprocket. Agents can still read Jira through your own MCP servers. |
| Tracket changes | Staged pull requests on `Hydhen/tracket`. The first one is opt-in and hidden until you sign in to Sprocket. |
| Platform | Windows only, like Tracket. |

What went wrong with ClaudeQuest and the first Sprocket attempt, and how this design avoids it:

- **Costly UI.** The first slice builds three of the mockup's screens. The graph editor, the Agents editor and the
  Integrations screen come later, once runs work.
- **Losing control of how things work.** Workflows are files you can read and keep in git. Every side effect is a row
  in an activity log, and each process has one job.
- **Too big, tangled.** Jira editing stays in Tracket. Files stay under about 400 lines.

## The words we use

These come from the mockup, so the code, the UI and this document use the same names.

- **Agent type**: a named role such as Planner, Builder or Reviewer. It has a model, the tools it may use, an
  approval policy (*Never*, *Before writes* or *Every action*) and instructions.
- **Workflow**: a named, versioned graph of steps. These kinds of steps exist:
  - *Trigger*: manual only for now, "offered in Tracket".
  - *Agent step*: hands work to an agent type, with an input and extra instructions. It answers in text, or with
    JSON fields (text, number, yes/no, list) that later steps can read.
  - *Condition*: takes its `yes` branch or its `no` branch, from an expression on earlier results.
  - *For each*: takes each item of a list through its steps, one at a time or a few in parallel.
  - *Human approval*: pauses until someone approves or rejects. Rejecting can loop back to an earlier step.
  - *Parallel branches*: lists of steps that run at the same time; the step after them waits for all of them.
  - *Action*: one thing done in another tool (a Jira comment or status, a Slack message), by a small agent that has
    the MCP server for it.
  - *End*: marks the run Passed or Failed.

  Schedules and Jira triggers come later.
- **Run**: one execution of a workflow on one ticket, numbered per workflow (`Chart image bump #128`). It records who
  started it and from where ("Tracket: started by Luc"). A ticket can have several runs, and Tracket shows the latest.
- **Job**: one step done once inside a run: in the workflow itself, or for one item of a loop (its *scope*, such as
  `each[2]/`). It has a state (Running, Waiting, Done, Failed, Rejected, Stopped, Interrupted), an attempt count, a
  log, an output (and a JSON output), and for agent steps, its agent session. Jobs redone after a rejection or a
  retry stay in the history, marked superseded.
- **Agent session**: the Agent SDK conversation behind an agent job: its transcript, tool calls and cost.
- **Approval**: there are two kinds, and they work differently.
  - **Gate**: a *Human approval* step. It is run state kept in SQLite and can wait hours. Nothing is held open while
    it waits.
  - **Tool ask**: an agent wants to use a tool and its policy says to ask first. The SDK's `canUseTool` is waiting
    in a live session. It has no time limit (the spike waited 5 minutes), but the ask lasts only as long as the agent
    host does. When the host stops, the ask expires and the job becomes Interrupted.

## Components

```
 ┌──────────── Tracket (existing, Hydhen/tracket) ────────────┐
 │ React UI ──invoke──> Rust                                  │
 │                       ├─ Jira API (Jira token in keychain) │
 │                       ├─ create_worktree (existing)        │
 │                       └─ Sprocket client ─────────────┼──┐ HTTP 127.0.0.1:<port>
 └────────────────────────────────────────────────────────────┘  │ Authorization: Bearer <token>
                                                                 ▼   (token in keychain)
 ┌──────────────────────── Sprocket (new) ─────────────────────────────────────────┐
 │  React UI <─events/invoke─> Rust core                                                │
 │                             ├─ local HTTP API (axum, loopback only)                  │
 │                             ├─ workflow engine: loads YAML, advances runs, gates     │
 │                             ├─ SQLite: runs, jobs, approvals, sessions, activity     │
 │                             ├─ tray icon, single instance                            │
 │                             └─ sidecar supervisor ──stdin/stdout NDJSON──> Agent host (Node)
 │                                                                               ├─ Agent SDK query()
 │                                                                               └─ canUseTool → tool asks
 └──────────────────────────────────────────────────────────────────────────────│───────┘
                                                                                 ▼
                                                     Claude Code, spawned by the SDK, in the ticket's
                                                     worktree; uses your Claude login and MCP servers
```

### Rust core (`src-tauri`)

Owns all state, so the UI can reload and the sidecar can crash without losing anything.

- **Workflow engine.** It reads workflow files from `workflows/*.yaml` in the app's data folder (and from a
  repository's `.sprocket/workflows/` later). It walks a run's graph: it starts agent jobs on the sidecar, opens
  gates, follows *approved*/*rejected* edges and marks the run finished. Each step forward is one SQLite transaction
  plus one activity row.
- **Restart behaviour.** When the app starts, runs that were Waiting at a gate are still waiting. Jobs that were
  Running are marked *Interrupted* and can be retried. A retry resumes the SDK session when it can.
- **Local HTTP API** (`axum`). It listens only on `127.0.0.1` and on a fixed port you can change in settings.
  - Every request needs the bearer token. Sprocket generates the token on first start and you copy it into
    Tracket once.
  - The API is the same one a shared server would expose later, so team mode wouldn't change Tracket's side.
- **Tray.** Closing the window leaves the app in the tray so runs continue. Windows notifications for "a run needs
  you" or "a run failed" are not built yet.

### Agent host (`agent-host/`, Node sidecar)

A small TypeScript program, bundled into one executable and shipped as a Tauri `externalBin`. It keeps no state.

- It runs one SDK `query()` per agent job. `cwd` is the ticket's worktree. The model, allowed tools and system prompt
  come from the agent type, and `settingSources` is user and project settings.
- `canUseTool` applies the agent type's policy:
  - With *Never* it allows.
  - With *Before writes* it allows read-only tools and asks about everything else. MCP tools whose names start with
    `get`, `list`, `search` and the like count as read-only.

  Each ask goes to the core as a tool ask.
- **Every action** can't rely on `canUseTool` alone. Claude Code calls it only for calls that would prompt, and allow
  rules or read-only tools skip it; in the spike, `ToolSearch` ran without asking. A `PreToolUse` hook therefore
  returns `permissionDecision: 'ask'` for every call, which sends each one through `canUseTool`. A real session
  confirmed it: a `Read` was asked about.
- **MCP servers.** `tools` only limits Claude Code's built-in tools: every MCP server in the user's setup stays
  available, Jira writes included, and each one's tool definitions cost context. So an agent type lists the servers
  it may use (`mcp: [Atlassian2]`):
  - With an empty list, `disallowedTools: ['mcp__*']` removes them all from the session. A trivial session then
    cost $0.001, against $0.41 in the spike.
  - With a list, the same `PreToolUse` hook refuses the other servers, even when the user's own allow rules would let
    them through. Their tool definitions still load, though. The SDK can't remove "every server but these" without
    knowing their names.
- The host always gets the path to Claude Code from the core (`--claude`), so the development build and the bundled
  exe run the same Claude Code. It sets `permissionMode: 'default'` and the agent type's model explicitly.
- stdout carries protocol lines only, and logs go to stderr. The host stops its agents and exits when stdin closes,
  so a crash or a forced kill of the app doesn't leave `claude.exe` running.
- The job's output is the agent's final message plus any structured result it returns. The engine stores it as
  `steps.<id>.output` so later steps can use it.

### UI (`src`)

The mockup's header nav is `Overview / Runs / Workflows / Agents / Integrations`. The first slice builds:

1. **Overview**: the stat tiles, "Waiting on you" (gates and tool asks, with Approve and Reject), a runs table with
   pipeline dots, and agent types with their current ticket.
2. **Runs**: the mockup's three panes:
   - the run list with filters;
   - the stage graph with jobs and their states;
   - the selected job's details, with its log, Approve/Reject, Retry and "Open agent session".

   The agent session opens the transcript with its tool calls.
3. **Workflows** and **Agents** as read-only lists of what the YAML files define, with "Open file". The graph editor
   and the agent type editor come later.

Integrations is left out, because it is all about Jira triggers and write-back, which come later.

## Workflow files

A workflow, for example:

```yaml
name: Fix a bug
description: Plan, get approval, implement, open a PR
offerInTracket: true
maxParallelRuns: 2
agents:
  planner:  { model: claude-opus-5-5,   ask: never,         tools: [Read, Grep, Glob, WebSearch] }
  builder:  { model: claude-sonnet-5-5, ask: before-writes, tools: [Read, Edit, Write, Bash] }
steps:
  - id: plan
    agent: planner
    instructions: Read the ticket and propose a plan. Do not change code.
  - id: approve
    approval: { summary: steps.plan.output }
    onReject: plan
  - id: build
    agent: builder
    input: steps.plan.output
    instructions: Implement the plan, run the tests, push a branch and open a draft PR.
```

Steps can nest. A condition holds its branches, a loop its steps:

```yaml
steps:
  - id: triage
    agent: planner
    output: { files: list, risky: yes/no }      # JSON output, which later steps can read
  - id: route
    if: steps.triage.output.risky and steps.triage.output.files contains "Cargo.toml"
    then:
      - id: check
        approval: { summary: "Risky change: {{ steps.triage.output.files }}" }
    else:
      - id: note
        agent: notifier                           # an action is done by a small agent with the right MCP server
        action: { do: comment, text: "Low risk, going ahead" }   # do: comment | status | slack | other, `to` for status/slack
  - id: each
    forEach: steps.triage.output.files
    as: file
    parallel: 2                                   # 1 by default: the items share the ticket's worktree
    onItemFailure: continue                       # or stop (the default)
    steps:
      - id: fix
        agent: builder
        instructions: Fix {{ file }}
  - id: report
    agent: planner
    input: steps.each.output                      # a loop gives the list of its items' results
```

Parallel branches have no node of their own in the editor: a node linked after another one that already has a
next step makes them branches of a group, written as

```yaml
  - id: together
    branches:
      - [ { id: code, agent: builder } ]
      - [ { id: docs, agent: planner }, { id: review, approval: { summary: steps.docs.output }, onReject: docs } ]
```

A branch can't read another one (they run at the same time); the steps after the group read them all. The group
fails once its branches have ended if one of them failed. The branches share the ticket's worktree.

Expressions read `steps.<id>.output` (and its JSON fields), and the item of a loop around them. They have `==`, `!=`,
`<`, `<=`, `>`, `>=`, `contains`, `and`, `or`, `not` and parentheses; text compares without case, and something
missing is null. `{{ … }}` puts a value into instructions, summaries and messages. Step names are unique in the
whole workflow. Loading checks that every reference is to a step that has run by then: not one in another branch,
nor, from outside, one inside a loop. `onReject` goes back to an earlier step next to the approval.

The engine works out what runs next from the jobs written so far, after every change: a condition records which
branch it took, a loop records its items and starts the next ones as earlier ones finish. Loops are capped at 50
items and 10 in parallel.

### The board

Every run has a board (`board: false` in a workflow turns it off): notes from its agents and from people, kept in
the core's `notes` table and shown in Runs and in Tracket's Agents section. Agents use two tools of an MCP server
the agent host runs in its own process, `sprocket`: `read_notes(since?)` and `post_note(message, to?)` (`to` names
a step or an agent type). The host sends each call to the core as `board.read` / `board.post` and returns the
core's `board.answer`. These tools never wait for a person, whatever the agent's ask policy, and an agent type with
`board: false` only reads. An agent with no MCP server of its own gets this one alone (`strictMcpConfig`). A
rejection's reason is posted for the step it sends the run back to. See docs/adr/0002-agents-working-together.md.

Agent types are shared between workflows: each is a file, `agents/<id>.yaml`, and steps name it. A workflow file may
also define its own under `agents:`, which win for that workflow; the Agents screen moves such ones to the shared files
("Move to shared"), folding them into a shared one of the same name when they do the same thing.

The Workflows and Agents screens edit these files. A save is checked with the same rules as loading before anything is
written, and the file is replaced in one step (written beside it, then moved). Saving rewrites the YAML, so comments
written by hand are lost: the editor owns the files.

## HTTP API (first slice)

| Method and path | What it does | Used by Tracket for |
| --- | --- | --- |
| `GET /v1/health` | Version, and whether the token is valid | Settings › Sprocket: "Connected" |
| `GET /v1/workflows?offered=true` | Name, description, id | The Agents button menu "Run with Sprocket" |
| `POST /v1/runs` | `{workflow, ticket: {key, title, url}, folder, startedBy}` → run | Starting a run |
| `GET /v1/runs?ticket=SIS-244` | Runs for a ticket, latest first, with stages, jobs, agent states and pending approvals | The Agents section on the ticket |
| `POST /v1/approvals/{id}` | `{decision: approve or reject, reason}` | Approve / Reject on the ticket |
| `POST /v1/runs/{id}/cancel`, `POST /v1/jobs/{id}/retry` | Stop a run, retry a failed job | Run actions |

- **Worktree.** `folder` is the worktree Tracket already makes with Start working (`create_worktree`). Tracket keeps
  doing that part. Sprocket doesn't create worktrees.
- **Updates.** Tracket polls `GET /v1/runs?ticket=` every few seconds while a ticket with an active run is open. A
  push stream (SSE) can come later for the Agents tab and notifications.
- **Deep links.** Run objects carry `id` and `number`, and "Open in Sprocket" calls a Tracket Rust command that
  focuses Sprocket on that run. That handoff goes through Sprocket's single-instance arguments, not a URL
  scheme.

## Agent tool asks (permission round trip)

```
Agent (SDK)        Agent host              Rust core                     UI / Tracket
    │ wants Bash        │                       │                              │
    ├──canUseTool──────>│                       │                              │
    │                   ├─tool.ask─────────────>│ insert approvals row         │
    │   (waits)         │                       │ activity row, notification   │
    │                   │                       ├─event / GET /v1/runs────────>│ card
    │                   │                       │<─decide (invoke or POST)─────┤ Approve / Reject
    │                   │<─tool.answer──────────┤ update row, activity row     │
    │<──allow / deny────┤                       │                              │
```

Gates use the same approvals table and the same cards. When decided, the engine moves the run forward, and nothing
goes to the sidecar.

## Protocol between the core and the agent host

NDJSON over stdio, one JSON object per line, versioned in `host.ready`.

| Direction | Types |
| --- | --- |
| Core → host | `job.start {jobId, cwd, prompt, agent}`, `job.resume {jobId, cwd, sdkSessionId, prompt, agent}`, `job.stop {jobId}`, `tool.answer {askId, behavior, message}` |
| Host → core | `host.ready {protocol, sdk, claudeCode}`, `host.error {message}`, `job.started {jobId, sdkSessionId}`, `job.message {jobId, message}`, `job.ended {jobId, result, output, costUsd, error}`, `tool.ask {askId, jobId, toolName, input}` |

`agent` is `{model, ask, tools, mcp, instructions}`. The definitions live in `agent-host/src/protocol.ts` and
`src-tauri/src/host/protocol.rs`.

## Tracket integration, staged

All of it is opt-in. Nothing shows until you fill in Settings › Sprocket.

**PR 1, first slice:**
- **Settings › Sprocket:** the address (default `http://127.0.0.1:<port>`) and the token, kept in Credential
  Manager next to the Jira one. Also the toggles "Agents section on tickets" and "Agents button next to Start
  working".
- **Rust:** a small Sprocket client.
  - It allows `http://` for loopback addresses only and `https://` everywhere else. Tracket's interface still makes no
    network calls, so its CSP is unchanged.
  - The client calls the endpoints listed above.
- **Ticket header:** the "Agents" button with the menu "Run with Sprocket". Picking a workflow runs the existing
  worktree step, then `POST /v1/runs`.
- **Ticket:** the "Agents · Sprocket" section. It shows the stages and jobs, the approval banner (Approve /
  Reject…) and per-agent state, or "No agent is working on this ticket" with a "Run a workflow" button.
- **README:** the "Your data" table gets two rows. The ticket key, title, link and worktree folder are sent to Sprocket
   when you start a run. Runs, stages and approvals are fetched from Sprocket when displayed and kept in
  memory.
- **Changeset:** one entry.

**Later PRs:** the sidebar's Agents tab with its dot, the "Hand to agents" drag band, notifications, and a default
workflow per project.

## Open decisions

- **Which Claude Code.** For now Sprocket uses the installed one (`%USERPROFILE%\.local\bin\claude.exe`, the
  `PATH`, or `SPROCKET_CLAUDE_PATH`). The alternative is shipping the SDK's 388 MB `claude.exe` in the installer.
- **The user's settings.** Agents load the user and project settings (`--settings user,project`), so they get your
  MCP servers and `CLAUDE.md`, and also your hooks: ClaudeQuest's `guild-hook.js` and `session_report.ps1` fire for
  agent sessions. The alternative is giving Sprocket its own settings.
- **Which MCP tools an agent may write with.** The `mcp` list decides which servers an agent can use. Inside an
  allowed server, an agent set to *Never* can also write (edit Jira, comment), unasked. If that is too much, the next
  step would be a list of tools per server.

## Things proved first (spike, see spike/RESULTS.md)

1. The Agent SDK runs from a single-file Node executable shipped as a Tauri sidecar, and finds Claude Code.
2. With `settingSources: ['user', 'project']`, the agent gets your Claude login, `CLAUDE.md` and MCP servers.
3. `canUseTool` can wait several minutes for an answer.
4. A session resumes with its SDK session id after the host has restarted.
5. An `axum` server inside the Tauri process, on loopback, serves Tracket's calls while the window is closed and the
   app sits in the tray.

## Repository layout (planned)

```
sprocket/
  docs/          ARCHITECTURE.md, adr/
  src/           React UI
  src-tauri/     Rust core: engine, HTTP API, SQLite, sidecar supervisor
  agent-host/    Node sidecar (TypeScript) and the protocol types
  workflows/     example workflow files
  scripts/       sidecar build, dev runner
```

## Later

- The graph editor (Condition, For each, Action, loop bodies) and the agent type editor, both writing the YAML files.
- Triggers: Jira events, schedules, Slack. Jira write-back: comments per stage, status mapping.
- In Tracket: the Agents tab, the drag band, notifications.
- Team mode: the same HTTP API on a shared server, with agents still running on each developer's PC.
