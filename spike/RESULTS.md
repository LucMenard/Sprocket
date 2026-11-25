# Spike results (2026-11-24)

Setup: Node 22.14, `@anthropic-ai/claude-agent-sdk` 0.3.331, the installed Claude Code 2.1.331, Windows 11. The script
is `agent-host/spike.mjs`. It runs against `sandbox-repo/`, a git repository whose `CLAUDE.md` holds a codename.

| # | Question | Result |
| --- | --- | --- |
| 1 | Does the SDK run from a single-file Node executable? | **Yes, if it is given the path to Claude Code.** See below. |
| 2 | With `settingSources: ['user', 'project']`, does the agent get the Claude login, `CLAUDE.md` and MCP servers? | **Yes.** No API key was needed. The agent quoted the codename from `CLAUDE.md` and listed the user's MCP servers (Atlassian2, github, obsidian, the claude.ai connectors). |
| 3 | Can `canUseTool` wait minutes for a human? | **Yes.** The Write call waited 300 seconds for the simulated approval and then completed normally. The session didn't time out, and the run finished on the third turn. |
| 4 | Can a session resume after the host restarts? | **Yes.** A new process with `resume: <sessionId>` remembered the file it had created and the codename. This also worked from the bundled exe. |
| 5 | Can an axum server inside the Tauri app answer while the app sits in the tray? | **Yes.** `tray-app/` is a Tauri 2.12 app that serves axum on the Tauri tokio runtime. Its window closes 2 s after start and hides to the tray. `/v1/health` answered at 1 s (window visible), 6 s (hidden) and 70 s (hidden for over a minute). Without the token it returned 401. It listens on `127.0.0.1:47821` only. |

## 1. Packaging the agent host

- The SDK is a 5 MB JavaScript layer. The agent itself is a native `claude.exe`, which the SDK takes from the optional
  package `@anthropic-ai/claude-agent-sdk-win32-x64` (388 MB). The SDK spawns it and talks to it over stdio.
- Build steps:
  1. esbuild turns the host into one CommonJS file (1.4 MB). This needs two flags,
     `--define:import.meta.url=__importMetaUrl` and a banner that sets `__importMetaUrl` from `__filename`.
  2. Node SEA and `postject` turn that file into `ct-agent-host.exe` (85 MB, mostly the Node runtime).
- Run from a folder without `node_modules`, the exe fails with "Native CLI binary for win32-x64 not found" unless
  `pathToClaudeCodeExecutable` is set. Pointed at the installed `C:\Users\<you>\.local\bin\claude.exe`, it works,
  including `resume`.
- **Decision to take.** The options are:
  - Use the user's installed Claude Code. This needs `claude` installed, and its version has to be at least the
    SDK's.
  - Ship the SDK's `claude.exe` (388 MB) beside the sidecar. This pins the version, but makes a very large installer.

  The first fits a tool for developers who already use Claude Code. Control Tower can check `claude --version` at
  startup.
- `postject` breaks the Authenticode signature of `node.exe` ("The signature seems corrupted"). The installer would
  have to sign the exe again, like Tracket's unsigned NSIS build.

## Other findings

- **The user's hooks run for agent sessions.** `settingSources: ['user']` loads `~/.claude/settings.json`, including
  its hooks: the ClaudeQuest `guild-hook.js` on SessionStart/SessionEnd and `session_report.ps1`. Agents started by
  Control Tower will appear there too. Decide whether Control Tower passes its own settings instead of `'user'`, or
  accepts this.
- **Model.** Without `model`, the session took `opus` from the user's settings (`claude-opus-5-5`). Agent types must
  set the model explicitly.
- **Cost of a trivial run.** Three turns cost about $0.41. Most of that is the context: the tool definitions of every
  MCP server the user has. Agent types should list the MCP servers they need (`mcpServers` / strict MCP config)
  instead of inheriting all of them.
- **Permission prompts have no time limit.** `sdk.d.ts` says "permission prompts have no park deadline" for
  `canUseTool`. A tool ask can therefore wait until the user answers or the job is stopped. The `signal` passed to
  `canUseTool` reports a stop.
- `permissionMode` must be set to `'default'`. Without it, the CLI may pick `auto` mode from the settings, and
  `canUseTool` would then only be asked about calls the classifier can't decide.
- The SDK will soon stop putting MCP server settings on the command line. Set `mcpServersDelivery: 'initialize'` when
  using the installed Claude Code (needs 2.1.261 or later).

## 5. The tray app

- Closing the window is caught in `on_window_event` (`CloseRequested`: `prevent_close`, then `hide`). The tray menu
  has Show and Quit.
- axum 0.8 runs on `tauri::async_runtime`, so there is no second runtime and no extra thread to manage.
- A debug build takes about 3 minutes the first time.
- Setting up this machine took rustup (Rust 1.100) and the "Desktop development with C++" workload in Visual Studio
  2026 (MSVC 14.50, Windows SDK 10.0.26100). Git Bash puts its own `link` on the `PATH`, but cargo finds the MSVC
  linker by itself once the workload is installed.

## Next

Scaffold `control-tower/` as described in `docs/ARCHITECTURE.md`.
