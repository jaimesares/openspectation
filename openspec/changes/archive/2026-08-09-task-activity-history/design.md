## Context

`App::on_tick` recomputes a whole-workspace fingerprint every fourth tick — one second at the default tick rate of 250 ms — and calls `reload()` when it differs. `reload()` replaces the workspace wholesale, and the only thing it reports is `tui.log.detected`. The fingerprint is a single `u64` over the entire tree, so it can say that something moved and nothing about what, which is exactly the message the user currently receives.

Two sources of history were considered, and both were measured before choosing.

## Goals / Non-Goals

**Goals:**

- The activity panel names what happened.
- History outlives the session.
- History that already exists in version control is recovered, including other people's.
- Durations at both granularities the user asked for: per task and per change.
- No new dependencies, and no blocking of the event loop.

**Non-Goals:**

- Measuring time actually spent working. That needs explicit start and stop and the discipline to use them; what is delivered is elapsed time, and the view says so.
- Writing anything into the user's workspace or into their repository, including version-control cache files.
- Attributing work to people beyond what version control already records.
- A filesystem-notification dependency. The fingerprint timer stays.

## Decisions

### Two sources, and observed time wins

| | in-session observation | version control |
|---|---|---|
| retroactive over existing history | no | yes |
| covers the whole team | no | yes |
| covers uncommitted work | yes | no |
| covers work done with the interface closed | no | yes |
| survives squash merges | yes | loses granularity |
| needs git | no | yes |

Neither is sufficient and each covers precisely the other's blind spot, which is why both are used rather than one with the other as a fallback.

Precedence goes to the observed time. A task is finished at 14:32 and committed at 18:00; the observed timestamp is the moment of completion, the commit timestamp is the moment of recording. Version control is therefore not the authority but the filler: everything from before this capability existed, everything done on other machines, and everything done while the interface was closed.

This ordering also repairs version control's worst weakness. A squash merge attributes eight completions to one commit, giving eight durations of zero — but if those eight were ticked with the interface open, the observed record already holds eight real timestamps and they win. The repair only extends to this machine; a teammate's squashed branch stays squashed.

### Task identity is a hash of the normalised task text

Reconciling the two sources requires knowing that a line seen in one is the same task as a line seen in the other.

| key | breaks when |
|---|---|
| literal text | the task is reworded, which typically happens in the same commit that ticks it |
| index within the file | tasks are inserted or reordered |
| an identifier written into the file | never, but it pollutes a file that belongs to the user |
| hash of normalised text | the task is rewritten substantially |

Normalising means dropping the checkbox, dropping the leading `N.` numbering, collapsing whitespace and lowering case. A substantially rewritten task reads as one task removed and another added. That is a bounded and explicable inaccuracy; writing identifiers into `tasks.md` is not acceptable, because that file is the user's and is committed.

### Events are stored, not states

`done`, `undone`, `task_added`, `task_removed`, `change_created`, `change_archived`, `status_changed`. Storing the state would make a tick-untick-retick sequence indistinguishable from a single tick and would lose the very timings the view exists to show. The view takes the most recent `done` per task and treats the first appearance as the start.

### Fingerprints at file granularity

Counting tasks is not enough to detect every transition: un-ticking one task and ticking another leaves the count identical. Being exact means diffing full task lists, and doing that for every change every second means reading every task file every second.

Instead the fingerprint gains a per-file variant over size and modification time, so only the task files that actually moved are re-read. This is the machinery already in `domain::watch`, applied at a finer grain.

### Version control is read through a cached incremental scan

Measured on synthetic repositories, with `openspec/changes` touched only in the last 120 commits — the shape of a repository that adopted OpenSpec recently:

| commits | plain | with commit-graph | with changed-path filters | incremental |
|---|---|---|---|---|
| 1,000 | 8 ms | | | |
| 10,000 | 44 ms | | 11 ms | 3 ms |
| 100,000 | 468 ms | 206 ms | 80 ms | 4 ms |
| 300,000 | 1,588 ms | | 278 ms | |

Two findings shaped the design.

The cost is walking the commit graph, not producing the diff: at 100,000 commits a plain log with no path filter and no patch already costs 407 ms of the 567 ms that the full command takes. Restricting to a path does not save the walk, because every commit must be visited to know whether it touched that path. It follows that a young `openspec/` directory with four files in it is not cheap to read — the price is paid for the commits that did *not* touch it.

The width of the root tree roughly doubles the per-commit cost: 44 ms at 10,000 commits with two root entries against 84 ms with 251, because each commit whose root tree changed requires that tree to be loaded and searched for the `openspec` entry.

The answer to both is to pay once. The full scan runs on first use, its result is cached under the user's cache directory keyed by the commit it was taken at, and subsequent runs examine only the range from that commit to the current head — 4 ms at 100,000 commits. If the recorded commit is no longer an ancestor of the current head, as after a rebase or a force push, the cache is discarded and the full scan runs again.

### The interface is never allowed to freeze silently

The scan is a subprocess, so it is queued through `app::Pending` and executed by the event loop after the announcing frame has been drawn, in the same way validation and archiving already are. At 250 ms per tick, a 1.6-second scan run inline would drop roughly six frames with no explanation on screen.

It also runs on demand, when the activity view is first opened, rather than at startup. A user who never opens the view never pays for it.

### Nothing is written to the user's repository

Changed-path filters are the difference between 206 ms and 80 ms at 100,000 commits, and they are not written by default — `gc.writeCommitGraph` defaults to true, but the filters do not come with it. Writing them costs about 1.9 seconds and 6.5 MB inside `.git`. It is only a cache file, but it is a cache file in someone else's repository. Their absence is detected and, when a scan was slow, the command that would write them is suggested in the activity panel. Whether to run it belongs to whoever owns the repository.

### The activity view is a third top-level view

`View` already holds `Editor` and `Dashboard`. The timeline and its derived figures do not fit in the editor's log pane, and the dashboard is a summary of the present rather than of the past.

### Layering

The event model, the durable store, the version-control reader and the reconciliation all live in `domain::history`, which never touches the terminal. That module runs a subprocess, which `domain::openspec` already does for validation and archiving; what stays in the TUI layer is deciding *when* it runs, through `Pending`.

## Risks / Trade-offs

- A substantially reworded task appears as one task ending and another beginning. Accepted, for the reason given above.
- Squash merges still flatten a teammate's timings. Only the local record can repair them.
- Shallow clones truncate what can be recovered. Detected, and reported rather than hidden, because a partial history presented as complete is worse than an acknowledged gap.
- Durations measure elapsed time, not effort. A task that sat untouched for a week reads as a week. The view labels this rather than trying to correct it, since any correction would be a guess.
- The history file grows without bound. Each toggle is one line, so years of work amount to a few hundred kilobytes; compaction on archive is available later if it is ever needed.
- The history file is keyed by the workspace's path, so moving the workspace orphans its history. The alternative key — the repository's first commit — would make the store depend on git, which it must not.
- Reading version control gives the view a dependency on an external program whose output format is being parsed. It is pinned to a stable, explicitly requested format rather than to whatever the user's configuration produces.
