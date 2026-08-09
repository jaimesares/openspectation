## Purpose

A workspace records what is supposed to happen — in its tasks, its proposals and its requirements — but not what did. The activity history is that missing half: when each task was completed, what else moved in the workspace, and what those timings imply about the work still outstanding.

## ADDED Requirements

### Requirement: The activity panel reports specific events

The activity panel SHALL report what changed in the workspace rather than that something changed. Each event SHALL name the change it belongs to, and a completion SHALL also name the task.

#### Scenario: a task is completed while the interface is open

- **WHEN** a task in a change becomes done by any route, including an edit made outside the interface by another tool
- **THEN** the panel records a completion naming the change and the task, with the time it was observed

#### Scenario: other workspace events

- **WHEN** a change is created, is archived, or its status moves from one value to another
- **THEN** each is recorded as its own event, naming the change and the transition

#### Scenario: nothing identifiable changed

- **WHEN** the workspace changes in a way that yields no recognisable event
- **THEN** the panel reports a generic change, and this is the only circumstance in which it does so

#### Scenario: a task is un-completed

- **WHEN** a task that was previously done becomes not done again
- **THEN** that is recorded as its own event and does not erase the earlier completion from the history

### Requirement: History survives the interface being closed

Events the interface observes SHALL be recorded durably, outside the workspace, and SHALL be present the next time the interface is opened on that workspace.

#### Scenario: reopening

- **WHEN** a user completes tasks, closes the interface, and opens it again
- **THEN** those completions are still in the history, with the times at which they were observed

#### Scenario: the workspace is committed

- **WHEN** the workspace is committed to version control and opened by another person
- **THEN** the first user's recorded history was not part of what was committed

### Requirement: History from before the interface is recovered

Where the workspace is under version control, the history SHALL include task completions made before this capability existed and completions made by other people, without anything having had to be recorded at the time.

#### Scenario: first use on an existing workspace

- **WHEN** the activity view is opened for the first time on a workspace whose task files have version-control history
- **THEN** completions recorded in that history are present, dated from the point at which they were made

#### Scenario: no version control

- **WHEN** the workspace is not under version control
- **THEN** the activity view still works, containing only what the interface observed for itself

#### Scenario: truncated version-control history

- **WHEN** the locally available version-control history is incomplete, so that earlier completions cannot be recovered
- **THEN** the view says the history is incomplete rather than presenting what it has as the whole record

### Requirement: The two sources are reconciled

A completion known both from direct observation and from version control SHALL appear once. Where the two disagree about when it happened, the observed time SHALL be preferred, because it is the moment the task was completed rather than the moment the work was recorded.

#### Scenario: a task completed and committed later

- **WHEN** a task is completed while the interface is open and the workspace is committed some time afterwards
- **THEN** the history holds a single completion for that task, timed from when it was observed

#### Scenario: several completions recorded at one moment

- **WHEN** version control attributes several completions to a single point in time, but those completions were separately observed
- **THEN** the history keeps the separate observed times rather than collapsing them

### Requirement: Durations and remaining work

The activity view SHALL show how long each task took and how long each change has been running, and SHALL derive from completed tasks an estimate of the work outstanding in a change.

#### Scenario: how long a task took

- **WHEN** a task has both a first appearance and a completion in the history
- **THEN** the view shows the time between them

#### Scenario: how long a change has taken

- **WHEN** a change has at least one completed task
- **THEN** the view shows the elapsed time from its first completion to its most recent one, or to its archival if it has been archived

#### Scenario: estimating what is left

- **WHEN** a change has both completed and outstanding tasks
- **THEN** the view shows an estimate for the outstanding ones derived from the completed ones, presented as an estimate rather than as a fact

#### Scenario: too little history to derive anything

- **WHEN** a change has no completed tasks
- **THEN** no duration and no estimate are shown for it, rather than a zero

#### Scenario: what a duration measures

- **WHEN** a duration is shown
- **THEN** the view makes clear that it measures elapsed time from the task appearing to it being completed, not time spent working

### Requirement: Reading history never blocks the interface silently

Recovering history from version control SHALL NOT freeze the interface without warning, and SHALL NOT be repeated when nothing has changed.

#### Scenario: a large repository

- **WHEN** history is recovered from a repository large enough for the work to take a noticeable time
- **THEN** the interface tells the user what it is doing on screen before the work begins

#### Scenario: opening the view again

- **WHEN** the activity view is opened again and the repository has advanced by a few commits since the last recovery
- **THEN** only those new commits are examined

#### Scenario: the repository's history was rewritten

- **WHEN** the repository no longer contains the point from which the previous recovery was made, as after a rebase or a force push
- **THEN** the recovery starts again from the beginning rather than reporting a history that no longer exists

#### Scenario: the workspace is being edited

- **WHEN** the user is editing a document
- **THEN** no history recovery is started, consistent with the interface not reloading the workspace under an open buffer
