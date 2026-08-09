## Why

The activity panel announces that the workspace changed, which is the one thing the user can already see for themselves. It never says what changed, so the panel costs a pane and returns nothing.

Meanwhile the workspace holds the answer to questions nobody can currently ask. Tasks are ticked, changes move through statuses and get archived, and none of it is recorded. When a task was finished, how long a change has been running, and how much is left are all knowable and all discarded.

## What Changes

- The activity panel reports what actually happened — tasks completed, changes created, statuses moved, changes archived — with the time of each, naming the change and the task.
- The generic "the workspace changed" message becomes the fallback for the case where nothing specific can be identified, instead of being the only message.
- Observed events are recorded durably outside the workspace, so history survives closing the interface.
- History that predates this change, and work done by other people, is recovered from version control where the workspace is under it.
- A new activity view presents the history as a timeline, derives how long each task took and how long each change has been running, and estimates the work remaining in a change from the tasks already completed.

## Capabilities

### New Capabilities

- `activity-history`: the record of what happened in a workspace over time, where it comes from, and how it is presented.

### Modified Capabilities

None. No existing capability has a spec yet.

## Impact

- New `src/domain/history.rs` — the event model, the durable store, the version-control reader, and the reconciliation between them
- New activity view under `src/tui/ui/`, alongside the editor and the dashboard
- `src/tui/app.rs` — diffing on reload, a third top-level view, a deferred action for the version-control scan
- `src/domain/watch.rs` — fingerprints at file granularity so the diff can be exact without re-reading every task file every second
- A history file under the user's state directory. Nothing is written into the workspace or into the repository.
- Reads `git` when it is present; the interface works without it, with a smaller history
- New user-facing strings, so both `locales/en.yml` and `locales/es.yml` move together
- No new dependencies
