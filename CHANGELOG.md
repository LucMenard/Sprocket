# sprocket

## 0.3.0

### Minor Changes

- [`7d57187`](https://github.com/LucMenard/Sprocket/commit/7d571870b2a7c96196296761affc02f3a3492284) Thanks [@LucMenard](https://github.com/LucMenard)! - New example agent types for development, review and design (fixer, test-writer, doc-writer, code-reviewer, security-reviewer, architect, ui-designer, researcher), and new example workflows: Build a feature, Review a change and Design a feature. Fix a bug now reviews its fix and fixes what blocks. A new agents or workflows folder gets them all; an existing one keeps what it has.

- [`e9f96e8`](https://github.com/LucMenard/Sprocket/commit/e9f96e8db532166aab114cbab8658d85172dc124) Thanks [@LucMenard](https://github.com/LucMenard)! - A new kind of step, **Launch a workflow**, runs another workflow and waits for it. You choose the workflow and what it is asked. Later steps read what its last step gave. The step fails if that run fails, and retrying it continues that run. Cancelling the outer run cancels it too.
  
  By default it runs on the same ticket, and Tracket shows the whole chain as one run. Give it a ticket (for example each subtask, in a loop) and it runs on that ticket instead, in that ticket's worktree, made the way Tracket's Start working makes it. When the launched workflow is at its limit of runs at once, the step waits for room instead of failing.

### Patch Changes

- [`5e5d620`](https://github.com/LucMenard/Sprocket/commit/5e5d6203e92732967fc4203121819f4d077f7e14) Thanks [@LucMenard](https://github.com/LucMenard)! - You can add MCP servers to an agent type again: the editor lists the servers your agents last loaded, and a server can be added by its name before any agent has used it.

## 0.2.0

### Minor Changes

- [`7fb3ead`](https://github.com/LucMenard/Sprocket/commit/7fb3ead4898314c7188f51a381b29e9c8eba936d) Thanks [@LucMenard](https://github.com/LucMenard)! - Sprocket can be installed from its GitHub releases and updates itself: when a new version is out, a band under the header offers to install it and restart. The installer brings the agent host with it; Claude Code must be installed on the computer.
