---
"sprocket": minor
---

Workflows have new **triggers**, set on their Start node in Workflows: a **schedule** (cron, with presets and the next times shown), **after another workflow** (when a run of it passes, fails or either), a **webhook** on the local API, and **Run now**. A run from a schedule, a webhook or Run now works in a repository folder with no Jira ticket; Start a run in Runs takes no ticket too. Each trigger shows its last run, or why its last try started nothing.
