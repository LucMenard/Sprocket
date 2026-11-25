# 0001: Sprocket is a desktop workflow engine for agents, beside Tracket

Date: 2026-11-24. Status: accepted. The app was called Control Tower until 2026-11-25, when it was renamed Sprocket.

## Context

A first attempt, then called Control Tower (Electron + TypeScript), did Jira and agents in one app. It was dropped. Tracket, a Tauri 2
Jira client, already does the Jira part well and already creates a git worktree per ticket. The targeted UI (canvas
"Task & Agent Tracker", page V2) has Sprocket run workflows of agent steps and approvals on tickets. Tracket
starts them and shows their state through an address and a token.

## Decision

- Sprocket is a Tauri 2 desktop app. Its Rust core is the workflow engine and owns state in SQLite. Runs progress
  while the app is open, including when it sits in the tray.
- Agents run through the Claude Agent SDK (TypeScript) in a Node sidecar, over NDJSON on stdio.
- Tracket talks to Sprocket over a loopback HTTP API with a bearer token. A later shared server can expose the
  same API.
- Workflows are YAML files. The first slice has manual runs from Tracket only, and Sprocket makes no Jira writes.
- Two kinds of approval: gates are run state and can wait for hours, while tool asks are a pending `canUseTool` call
  and need an answer within minutes.
- Tracket changes go in staged pull requests on `Hydhen/tracket`. The first PR is opt-in.

## Consequences

- Runs stop progressing while the app is closed. Gates survive; running jobs come back Interrupted and can be retried.
- Two languages and three processes (Rust core, Node sidecar, Claude Code). The Agent SDK sidecar and the HTTP server
  in the tray are unproven, so a spike comes first.
- Tracket gets a second network destination, loopback by default, which its README must state.
- Moving the engine to a server later means moving the Rust core's engine. The HTTP API is meant to make that
  invisible to Tracket.
