## 1. Event model

- [x] 1.1 Create `src/domain/history.rs` with an event type covering `done`, `undone`, `task_added`, `task_removed`, `change_created`, `change_archived` and `status_changed`, each carrying a timestamp, a change id and, where it applies, a task identity and the task's text.
- [x] 1.2 Implement task identity as a hash of the normalised task text: checkbox dropped, leading `N.` numbering dropped, whitespace collapsed, case lowered.
- [x] 1.3 Test that renumbering and reindenting a task leave its identity unchanged, and that rewriting its wording does not.

## 2. Detecting events on reload

- [x] 2.1 Add a per-file fingerprint to `domain::watch` over size and modification time, so a caller can tell which task files moved.
- [x] 2.2 Before `App::reload` replaces the workspace, capture the task list of every change whose task file moved; after it, diff the two and produce events.
- [x] 2.3 Emit change-level events for creation, archival and status transitions.
- [x] 2.4 Report each event in the activity panel naming the change and, for a completion, the task, with its time.
- [x] 2.5 Keep the generic message only for a reload that produced no events.
- [x] 2.6 Test that un-ticking one task and ticking another in the same interval produces two events, since the task count does not move.

## 3. Durable store

- [x] 3.1 Append observed events as one record per line to a history file under the user's state directory, keyed by the workspace path.
- [x] 3.2 Load the file on startup, tolerating a truncated or corrupt final line rather than failing, in the way `Settings::load` tolerates a corrupt file.
- [x] 3.3 Test that events survive a save and load cycle, and that a malformed line is skipped rather than fatal.

## 4. Version-control recovery

- [x] 4.1 Read task-file history from git in one pass over the changes directory, requesting an explicit stable format, with renames disabled and no context lines.
- [x] 4.2 Parse checkbox transitions out of the diff hunks into completion events, dated from the author date so the timings survive a rebase.
- [x] 4.3 Queue the scan as an `app::Pending` action so it runs after the announcing frame, and only when the activity view is first opened — never while a buffer is being edited.
- [x] 4.4 Cache the result under the user's cache directory, keyed by the commit it was taken at.
- [x] 4.5 On later runs, examine only the range from the cached commit to the current head; discard the cache and rescan when the cached commit is no longer an ancestor of the head.
- [x] 4.6 Detect a shallow repository and mark the history as incomplete.
- [x] 4.7 Detect the absence of changed-path filters and, when a scan was slow, suggest the command that writes them. Do not run it.
- [x] 4.8 Degrade to the observed history alone when git is absent or the workspace is not a repository.
- [x] 4.9 Test the parser against captured diff output, including a commit that both reworks and ticks a task.

## 5. Reconciliation

- [x] 5.1 Merge the two sources on task identity, preferring the observed timestamp where both have one.
- [x] 5.2 Test that a completion observed and later committed appears once, with the observed time.
- [x] 5.3 Test that completions collapsed into a single commit keep their separate observed times where those exist.

## 6. Derived figures

- [x] 6.1 Compute per-task elapsed time from first appearance to completion.
- [x] 6.2 Compute per-change elapsed time from first completion to the most recent one, or to archival.
- [x] 6.3 Estimate outstanding work in a change from the median of its completed tasks; produce nothing when there are no completed tasks.
- [x] 6.4 Test that a change with no completed tasks yields no duration and no estimate rather than zeroes.

## 7. Activity view

- [x] 7.1 Add `View::Activity` alongside `Editor` and `Dashboard`, with the key and command to reach it.
- [x] 7.2 Render the timeline grouped by day, newest first, each entry naming its change, its task and its duration.
- [x] 7.3 Render the per-change summary with progress, median task time and the estimate for what remains.
- [x] 7.4 Label durations as elapsed time, and show the incompleteness marker when history was truncated.
- [x] 7.5 Add a rendering test in `src/tui/ui/tests.rs` against a fixture history, taking the guard from `domain::settings::redirect_for_test`.

## 8. Localisation

- [x] 8.1 Add every new string to `locales/en.yml` and `locales/es.yml` together: event lines, view title, column headings, duration units, the estimate label, the incomplete-history notice and the changed-path-filter suggestion.
- [x] 8.2 Confirm `catalogues_declare_the_same_keys` passes.

## 9. Close out

- [x] 9.1 `cargo fmt`, `cargo clippy --all-targets`, `cargo test`.
