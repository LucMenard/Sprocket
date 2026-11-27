---
"sprocket": minor
---

A new kind of step, **Launch a workflow**, runs another workflow and waits for it. You choose the workflow and what it is asked. Later steps read what its last step gave. The step fails if that run fails, and retrying it continues that run. Cancelling the outer run cancels it too.

By default it runs on the same ticket, and Tracket shows the whole chain as one run. Give it a ticket (for example each subtask, in a loop) and it runs on that ticket instead, in that ticket's worktree, made the way Tracket's Start working makes it. When the launched workflow is at its limit of runs at once, the step waits for room instead of failing.
