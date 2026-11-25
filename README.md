# Sprocket

Runs Claude agent workflows on Jira tickets, beside [Tracket](../tracket). Tracket handles Jira; Sprocket runs
the agents. How it is built and why: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

Windows only for now.

## What works (0.1.0)

- **Runs.** A run takes a ticket through a workflow's steps:
  - An agent step starts an Agent SDK session in the ticket's worktree and goes on when it succeeds. Its result is
    available to later steps as `steps.<id>.output`, in text or as JSON fields when the step asks for them.
  - A condition takes its `yes` or its `no` branch from an expression such as
    `steps.triage.output.risky and steps.triage.output.severity == "high"`.
  - A For each takes every item of a list through its steps, one at a time or a few in parallel, and stops or goes
    on when an item fails.
  - Parallel branches run at the same time, and the step after them waits for all of them.
  - An action does one thing in another tool (a Jira comment or status change, a Slack message) through a small
    agent that has the MCP server for it.
  - An approval step waits until someone approves it. Rejecting it can send the run back to an earlier step
    (`onReject`), with the reason passed to the agent.
  - An agent whose policy asks before a tool call waits for an answer. The answer goes back to the agent.
  - Closing or crashing the app marks running jobs **Interrupted**. Approval steps keep waiting. Retry resumes the
    agent's session where it stopped.
  - Each workflow has a limit on how many runs it can have at once, and a run keeps the workflow it started with.
  - A run has a **board**: notes its agents post for later steps (`post_note`) and read (`read_notes`), through
    tools the agent host serves itself. You read and post on it from Runs or from Tracket, and a rejection's reason
    lands there for the step that redoes the work. A workflow can turn its board off, and an agent type can be
    kept to reading it.
  - An agent can **ask a colleague**: `ask_agent(agentType, question)` runs a short session of that agent type in
    the same worktree, and its answer comes back to the agent. The workflow says who may be asked (the agent types
    its steps use, by default) and how many times a step may ask (3); an agent type can be kept from asking. The
    asked one can't ask in turn. Runs shows each step's asks, with the question, the answer and the session.
- **Screens**, laid out like the mockup (canvas "Task & Agent Tracker", page V2) in Tracket's look:
  - Overview: counters, "Waiting on you" (approve, or reject with a reason), runs with their step dots, the agents
    at work and the recent events.
  - Runs: three resizable panes:
    - the run list, with filters;
    - the run's steps as a graph: the branch a condition took, a loop's items and how many are done, the arrow back
      to the step a rejection returns to;
    - the selected step's details: status, its loop items, attempts, approvals, output and JSON output, a
      timestamped log, Retry or Resume, and the agent session.

    Above them sit Cancel run, Open in Jira and Run again, and a run can be started by hand.
  - Workflows: an editor drawn like the canvas: a condition's `no` branch in a column on the right, a loop's steps
    in a dashed frame, parallel branches side by side. Drag the background to pan, scroll or use the buttons to
    zoom. Drag nodes to place them: the positions are kept in the workflow file (`layout:`), and Reset layout
    goes back to the automatic one. Drag a node's bottom dot onto another node to link them: it runs next, or
    alongside what already runs next (a parallel branch); an approval dragged onto an earlier step gets its
    rejection link. The inspector lists each node's links, deletes any of them, and links by keyboard. Deleting a
    link into a step leaves it and the steps after it "Not linked" until they are linked again. Any link can be
    deleted while editing; Save stays off until the graph is complete again, and hovering it lists what is
    missing. Click a link to select it. Keys: Delete or Backspace deletes the selected link or node, the arrows
    move the selected node (Shift: further), Escape leaves the link, Ctrl+Z and Ctrl+Y undo and redo, Ctrl+S saves. Deleting a workflow or an agent type,
    or leaving unsaved edits, asks first. Add agent steps, conditions, loops, approvals and actions, then set each one in the
    inspector: its name, agent type, input, instructions, JSON output fields, an expression, the items of a loop and
    how many at once, what an approval shows and where a rejection goes, what an action does. Steps can be moved and deleted, and you
    can edit the workflow's name, description and run limit. New, Save, Discard and Delete are there too. Save checks
    the workflow with the same rules as loading, and writes nothing when it fails.
  - Agents: an editor for the shared agent types: model, tools, MCP servers, when to ask a human, instructions and
    description. New and Delete are there too; an agent type still used by a workflow can't be deleted. Agent types
    still defined inside a workflow file are listed apart, with "Move to shared".
  - Integrations: the Tracket connection (address, token, recent calls) and the MCP servers the agents see. Rules,
    status mapping and write-back to Jira are marked as coming later.
- **Window.** Borderless, with a tray icon. Closing the window keeps runs going in the tray. A second launch brings
  the running app to the front.
- **Local API for Tracket** on `http://127.0.0.1:47821`, with `Authorization: Bearer <token>` on every request. The
  token is in Windows Credential Manager, and Overview copies it.

  | Method and path | What it does |
  | --- | --- |
  | `GET /v1/health` | Version and the agent host's state |
  | `GET /v1/workflows?offered=true` | Workflows offered in Tracket |
  | `GET /v1/runs?ticket=SIS-244` | Runs (of a ticket), newest first, with jobs, pending approvals and the board's notes |
  | `POST /v1/runs/{id}/notes` | `{body, to?, by}`: a person's note on the run's board; 201 with the note |
  | `POST /v1/runs` | `{workflow, ticket: {key, title, url}, folder, startedBy}`; 201 with the run, 409 at the workflow's limit |
  | `POST /v1/approvals/{id}` | `{decision: "approve" or "reject", reason?, by?}`; 409 if already decided |
  | `POST /v1/runs/{id}/cancel?by=` | Stops the agents and expires the approvals |
  | `POST /v1/jobs/{id}/retry` | A failed or interrupted step again (resumes the session if there is one) |

- **Workflows** are read from `%APPDATA%\com.sprocket.app\workflows\*.yaml`, and **agent types** from
  `%APPDATA%\com.sprocket.appgents\*.yaml`. A workflow's steps use the shared agent types. A workflow file may
  still define its own under `agents:`, and those win for that workflow. On first start the folders get
  [workflows/fix-a-bug.yaml](workflows/fix-a-bug.yaml) and [agents/](agents). The editors own these files: saving
  rewrites them, so comments written by hand are not kept.
- **Agent host** (`agent-host/`, Node + Claude Agent SDK). The app starts it, restarts it with a backoff if it dies,
  and kills it with its agents on quit. It also exits by itself if the app dies.
- **SQLite** at `%APPDATA%\com.sprocket.app\sprocket.sqlite`: runs, jobs, agent messages, approvals and
  the append-only activity log.

Next: the Tracket pull request (Settings › Sprocket, the Agents button and the Agents section on the ticket).
Later: notifications, the graph editor, Jira triggers.

## Development

Requirements:
- Node 22
- Rust 1.90 or later, with the MSVC toolchain
- The [Tauri prerequisites](https://tauri.app/start/prerequisites/)
- Claude Code installed and signed in (`claude --version`)

```bash
npm install
npm run host:install
npm run host:build          # agent-host/dist/host.cjs, which the app runs with node
npm run tauri dev           # the app with hot reload
npx tauri build --debug --no-bundle   # src-tauri/target/debug/sprocket.exe
```

Checks:

```bash
npx tsc -b && npm run lint
npm run typecheck --prefix agent-host
cargo test --manifest-path src-tauri/Cargo.toml
```

Environment variables:

| Variable | Effect |
| --- | --- |
| `SPROCKET_PORT` | Port of the local API (default 47821) |
| `SPROCKET_VIEW` | Debug builds only: the screen to open on (`runs`, `workflows`…) |
| `SPROCKET_DATA_DIR` | Debug builds only: another data folder (database, workflows, agent types), to try things apart from the real ones |
| `SPROCKET_CLAUDE_PATH` | The `claude.exe` the agents use (default: `%USERPROFILE%\.local\bin\claude.exe`, then the `PATH`) |

## Third-party notices

The title bar, theme toggle, theme code, `src/index.css` and `src/components/ui/button.tsx` are adapted from
Tracket, MIT, Copyright (c) 2026 Hydhen.
