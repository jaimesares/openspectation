//! What happened in a workspace, and when.
//!
//! A workspace records what is supposed to happen. Nothing records what did:
//! tasks are ticked, changes move through statuses and get archived, and all of
//! it is discarded. This module is the missing half, and it has to build that
//! record from two sources that cover precisely each other's blind spots.
//!
//! Direct observation — the interface noticing a task file move under it —
//! knows about uncommitted work and survives a squash merge, but knows nothing
//! from before it was watching. Version control knows everything that was ever
//! committed, by anyone, but dates a completion from the moment it was recorded
//! rather than from the moment it happened, and a squash merge flattens eight
//! completions into one instant. Both are read, and where they disagree the
//! observed time wins: it is the moment the task was finished.
//!
//! Events are stored rather than states. A tick, an untick and a re-tick have
//! to stay three events, because collapsing them to "done" would lose exactly
//! the timings the activity view exists to show.
//!
//! Nothing here touches the terminal, and nothing is written into the user's
//! workspace or repository: the observed record lives under the user's state
//! directory and the version-control scan is cached under their cache
//! directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::change::ChangeProposal;
use crate::domain::spec::{SpecStatus, checkbox_state};
use crate::domain::watch::{self, Fingerprint};
use crate::domain::workspace::Workspace;

/// Directory created under the state and cache roots.
pub const DIRECTORY: &str = "openspectation";

/// Environment variable that relocates the observed history file wholesale.
///
/// Exists so a test — or a throwaway invocation — is given a file of its own
/// instead of appending to the caller's real record.
pub const PATH_ENV: &str = "OPENSPECTATION_HISTORY";

/// Environment variable that relocates the version-control scan cache.
pub const CACHE_ENV: &str = "OPENSPECTATION_HISTORY_CACHE";

/// Field separator of a stored record.
const FIELD: char = '\t';

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventKind {
    Done,
    Undone,
    TaskAdded,
    TaskRemoved,
    ChangeCreated,
    ChangeArchived,
    StatusChanged,
}

impl EventKind {
    /// Stable identifier written to the history file.
    ///
    /// Deliberately not the variant name: the file outlives any given build, so
    /// renaming a variant must not orphan records already on disk.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Undone => "undone",
            Self::TaskAdded => "task_added",
            Self::TaskRemoved => "task_removed",
            Self::ChangeCreated => "change_created",
            Self::ChangeArchived => "change_archived",
            Self::StatusChanged => "status_changed",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        [
            Self::Done,
            Self::Undone,
            Self::TaskAdded,
            Self::TaskRemoved,
            Self::ChangeCreated,
            Self::ChangeArchived,
            Self::StatusChanged,
        ]
        .into_iter()
        .find(|kind| kind.code() == raw)
    }

    /// Translation key of the line reporting this event.
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Done => "tui.event.done",
            Self::Undone => "tui.event.undone",
            Self::TaskAdded => "tui.event.task_added",
            Self::TaskRemoved => "tui.event.task_removed",
            Self::ChangeCreated => "tui.event.change_created",
            Self::ChangeArchived => "tui.event.change_archived",
            Self::StatusChanged => "tui.event.status_changed",
        }
    }

    /// Whether the event is about one task rather than about a whole change.
    pub const fn is_task(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Undone | Self::TaskAdded | Self::TaskRemoved
        )
    }
}

/// Which task an event is about.
///
/// A hash of the task's normalised text rather than its position or its literal
/// wording. Position breaks the moment a task is inserted above it; literal
/// text breaks on the reword that so often shares a commit with the tick; and
/// an identifier written into `tasks.md` would pollute a file that belongs to
/// the user and is committed with their repository.
///
/// The cost is that a substantially rewritten task reads as one task removed
/// and another added. That is bounded and explicable; the alternatives are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TaskIdentity(u64);

impl TaskIdentity {
    /// Identity of a raw task line, checkbox and numbering included.
    pub fn of(line: &str) -> Self {
        Self(fnv(normalise(line).as_bytes()))
    }

    pub fn to_hex(self) -> String {
        format!("{:016x}", self.0)
    }

    pub fn parse(raw: &str) -> Option<Self> {
        u64::from_str_radix(raw, 16).ok().map(Self)
    }
}

/// One thing that happened, at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Seconds since the Unix epoch.
    ///
    /// A number rather than a `SystemTime` because every one of these is
    /// written to a file and read back, and because the two sources hand over
    /// epoch seconds in the first place.
    pub at: u64,
    pub kind: EventKind,
    /// Identifier of the change the event belongs to.
    pub change: String,
    /// The task, for the events that have one.
    pub task: Option<TaskIdentity>,
    /// Task text as it read when the event was seen, or the status transition.
    pub label: String,
}

impl Event {
    pub fn new(at: u64, kind: EventKind, change: impl Into<String>) -> Self {
        Self {
            at,
            kind,
            change: change.into(),
            task: None,
            label: String::new(),
        }
    }

    #[must_use]
    pub fn about(mut self, task: TaskIdentity, label: impl Into<String>) -> Self {
        self.task = Some(task);
        self.label = label.into();
        self
    }

    #[must_use]
    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn time(&self) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::from_secs(self.at)
    }

    /// One record, one line.
    pub fn encode(&self) -> String {
        format!(
            "{}{FIELD}{}{FIELD}{}{FIELD}{}{FIELD}{}",
            self.at,
            self.kind.code(),
            escape(&self.change),
            self.task.map(TaskIdentity::to_hex).unwrap_or_default(),
            escape(&self.label)
        )
    }

    /// Reads one record back, returning `None` for anything malformed.
    ///
    /// A truncated final line is what a crash mid-append leaves behind, so it is
    /// a line to skip rather than a reason to refuse the whole history.
    pub fn decode(line: &str) -> Option<Self> {
        let mut fields = line.split(FIELD);
        let at = fields.next()?.trim().parse().ok()?;
        let kind = EventKind::parse(fields.next()?.trim())?;
        let change = unescape(fields.next()?);
        let task = fields.next()?;
        let label = unescape(fields.next()?);

        Some(Self {
            at,
            kind,
            change,
            task: if task.is_empty() {
                None
            } else {
                Some(TaskIdentity::parse(task)?)
            },
            label,
        })
    }
}

/// Seconds since the Unix epoch, for a clock that may be before it.
pub fn epoch_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

pub fn now() -> u64 {
    epoch_seconds(SystemTime::now())
}

// ---------------------------------------------------------------------------
// Task identity
// ---------------------------------------------------------------------------

/// Task text reduced to what identifies it.
///
/// The checkbox goes because ticking a task must not change what it is; the
/// leading `N.` numbering goes because inserting a task above renumbers every
/// task below it; whitespace is collapsed because reindenting a nested list
/// rewrites the indentation of everything under it; and case is lowered because
/// capitalising the first word is an edit to the prose, not to the task.
fn normalise(line: &str) -> String {
    let trimmed = line.trim();
    let body = trimmed
        .strip_prefix('-')
        .or_else(|| trimmed.strip_prefix('*'))
        .or_else(|| trimmed.strip_prefix('+'))
        .unwrap_or(trimmed)
        .trim_start();

    let body = ["[ ]", "[x]", "[X]", "[✓]"]
        .iter()
        .find_map(|marker| body.strip_prefix(*marker))
        .unwrap_or(body)
        .trim_start();

    let mut words = body.split_whitespace().peekable();
    if words.peek().is_some_and(|word| is_numbering(word)) {
        words.next();
    }

    words
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .trim()
        .to_string()
}

/// Whether a word is an outline number such as `1.`, `2.3` or `4.1.2)`.
///
/// A bare run of digits is not one. `2020 was the year` opens with a number
/// that is part of what the task says, and there is nothing in the text to tell
/// the two apart beyond the punctuation or the dotted shape that numbering
/// carries and prose does not.
fn is_numbering(word: &str) -> bool {
    let trimmed = word.trim_end_matches([')', '.', ':']);
    if trimmed.is_empty() || trimmed == word && !word.contains('.') {
        return false;
    }
    trimmed
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// FNV-1a, as everywhere else in this crate: no dependency, and a collision
/// here merges two tasks in a timeline rather than corrupting anything.
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();

    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Observing the workspace
// ---------------------------------------------------------------------------

/// One task, as it stood at the moment a snapshot was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskState {
    identity: TaskIdentity,
    label: String,
    done: bool,
}

/// One change, as it stood at the moment a snapshot was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ChangeState {
    /// Per-file fingerprint over the change's documents, so a diff can skip
    /// every change whose files did not move.
    stamp: Fingerprint,
    status: SpecStatus,
    archived: bool,
    tasks: Vec<TaskState>,
}

/// The task-bearing state of a whole workspace at one moment.
///
/// Taking one costs no I/O: the workspace already holds every document body
/// and every document's size and modification time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    changes: BTreeMap<String, ChangeState>,
}

impl Snapshot {
    pub fn capture(workspace: &Workspace) -> Self {
        Self {
            changes: workspace
                .changes
                .iter()
                .map(|change| (change.id.clone(), ChangeState::capture(change)))
                .collect(),
        }
    }

    /// Everything that happened between this snapshot and a later one.
    ///
    /// Counting tasks would not do: un-ticking one task and ticking another in
    /// the same interval leaves the count exactly where it was. The task lists
    /// are therefore compared by identity, but only for the changes whose files
    /// actually moved.
    pub fn diff(&self, next: &Self, at: u64) -> Vec<Event> {
        let mut events = Vec::new();

        // Archiving is not a flag being flipped: the directory moves under
        // `changes/archive/` and takes a date on its name, so the change leaves
        // under one identifier and arrives under another.
        let mut archives = std::collections::BTreeSet::new();
        for id in self.changes.keys() {
            if next.changes.contains_key(id) {
                continue;
            }
            if let Some((archived, _)) = next
                .changes
                .iter()
                .find(|(candidate, state)| state.archived && is_archive_of(candidate, id))
            {
                archives.insert(archived.clone());
                events.push(Event::new(at, EventKind::ChangeArchived, id));
            }
        }

        for (id, after) in &next.changes {
            if archives.contains(id) {
                continue;
            }

            let Some(before) = self.changes.get(id) else {
                events.push(
                    Event::new(at, EventKind::ChangeCreated, id).labelled(after.status.key()),
                );
                continue;
            };

            if !before.archived && after.archived {
                events.push(Event::new(at, EventKind::ChangeArchived, id));
            } else if before.status != after.status {
                events.push(
                    Event::new(at, EventKind::StatusChanged, id).labelled(format!(
                        "{} → {}",
                        before.status.key(),
                        after.status.key()
                    )),
                );
            }

            if before.stamp != after.stamp {
                events.extend(diff_tasks(id, &before.tasks, &after.tasks, at));
            }
        }

        events
    }
}

impl ChangeState {
    fn capture(change: &ChangeProposal) -> Self {
        let stamp =
            change
                .documents
                .iter()
                .fold(Fingerprint::default(), |accumulated, document| {
                    accumulated.combined(watch::stamp_of(document.size, document.modified))
                });

        Self {
            stamp,
            status: change.status,
            archived: change.archived,
            tasks: crate::domain::tasks::collect(change)
                .into_iter()
                .map(|task| TaskState {
                    identity: TaskIdentity::of(&task.label),
                    label: task.label,
                    done: task.done,
                })
                .collect(),
        }
    }
}

/// Whether `candidate` is the archived form of `id`, which the upstream CLI
/// writes as the identifier behind the date it shipped on.
fn is_archive_of(candidate: &str, id: &str) -> bool {
    candidate == id || candidate.ends_with(&format!("-{id}"))
}

fn diff_tasks(change: &str, before: &[TaskState], after: &[TaskState], at: u64) -> Vec<Event> {
    let index: BTreeMap<TaskIdentity, &TaskState> =
        before.iter().map(|task| (task.identity, task)).collect();
    let surviving: BTreeMap<TaskIdentity, &TaskState> =
        after.iter().map(|task| (task.identity, task)).collect();

    let mut events = Vec::new();

    for task in after {
        match index.get(&task.identity) {
            Some(previous) if previous.done == task.done => {}
            Some(_) => events.push(
                Event::new(
                    at,
                    if task.done {
                        EventKind::Done
                    } else {
                        EventKind::Undone
                    },
                    change,
                )
                .about(task.identity, &task.label),
            ),
            None => {
                events.push(
                    Event::new(at, EventKind::TaskAdded, change).about(task.identity, &task.label),
                );
                // A task that arrives already ticked was completed, whatever
                // wrote it: an agent rewriting the file is still work done.
                if task.done {
                    events.push(
                        Event::new(at, EventKind::Done, change).about(task.identity, &task.label),
                    );
                }
            }
        }
    }

    for task in before {
        if !surviving.contains_key(&task.identity) {
            events.push(
                Event::new(at, EventKind::TaskRemoved, change).about(task.identity, &task.label),
            );
        }
    }

    events
}

// ---------------------------------------------------------------------------
// The durable store
// ---------------------------------------------------------------------------

/// The observed record of one workspace, appended to as events are seen.
///
/// Keyed by the workspace's path, which means moving a workspace orphans its
/// history. The alternative key — the repository's first commit — would make
/// the record depend on version control, which this half of the history exists
/// precisely not to.
#[derive(Debug, Clone, Default)]
pub struct Store {
    path: Option<PathBuf>,
    events: Vec<Event>,
}

impl Store {
    /// Reads the record for a workspace. Never fails.
    ///
    /// A file that cannot be read is a history lost, not a session refused, in
    /// the way a corrupt settings file yields the defaults.
    pub fn open(workspace_root: &Path) -> Self {
        let path = Self::path_for(workspace_root);
        let events = path
            .as_deref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|raw| decode_all(&raw))
            .unwrap_or_default();

        Self { path, events }
    }

    /// Where the record for a workspace is kept.
    ///
    /// `None` when neither the override nor a home directory is known, which is
    /// the one case where nothing can be persisted.
    pub fn path_for(workspace_root: &Path) -> Option<PathBuf> {
        if let Ok(explicit) = std::env::var(PATH_ENV)
            && !explicit.trim().is_empty()
        {
            return Some(PathBuf::from(explicit));
        }
        Some(state_root()?.join(DIRECTORY).join(key_for(workspace_root)))
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Records events and appends them to the file.
    ///
    /// Best effort: a read-only state directory costs the durability of the
    /// record, not the session that produced it.
    pub fn append(&mut self, events: &[Event]) {
        if events.is_empty() {
            return;
        }
        self.events.extend_from_slice(events);

        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut body = String::new();
        for event in events {
            body.push_str(&event.encode());
            body.push('\n');
        }

        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = file.write_all(body.as_bytes());
        }
    }
}

/// Records from a file, skipping every line that does not read as one.
fn decode_all(raw: &str) -> Vec<Event> {
    raw.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(Event::decode)
        .collect()
}

/// File name a workspace's record is kept under.
///
/// A readable stem so a directory of these can be made sense of, plus a hash of
/// the whole path so two workspaces with the same last component do not share
/// a file.
fn key_for(workspace_root: &Path) -> String {
    let stem = workspace_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(|name| {
            name.chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() {
                        character.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        })
        .filter(|name| !name.trim_matches('-').is_empty())
        .unwrap_or_else(|| "workspace".to_string());

    format!(
        "{stem}-{:016x}.log",
        fnv(workspace_root.as_os_str().as_encoded_bytes())
    )
}

fn state_root() -> Option<PathBuf> {
    match std::env::var("XDG_STATE_HOME") {
        Ok(value) if !value.trim().is_empty() => Some(PathBuf::from(value)),
        _ => Some(PathBuf::from(std::env::var("HOME").ok()?).join(".local/state")),
    }
}

fn cache_root() -> Option<PathBuf> {
    match std::env::var("XDG_CACHE_HOME") {
        Ok(value) if !value.trim().is_empty() => Some(PathBuf::from(value)),
        _ => Some(PathBuf::from(std::env::var("HOME").ok()?).join(".cache")),
    }
}

// ---------------------------------------------------------------------------
// Recovering history from version control
// ---------------------------------------------------------------------------

/// Executable looked up on `PATH`.
const GIT: &str = "git";

/// Byte prefacing a commit header in the requested log format.
///
/// A control character rather than a word, because whatever separates commits
/// from diff content has to be something the diff content cannot contain.
const COMMIT_MARK: char = '\u{1}';

/// Field separator inside a commit header.
const HEADER_FIELD: char = '\u{1f}';

/// A scan slower than this earns the suggestion to write changed-path filters.
///
/// Below it the advice would cost the user more attention than the scan costs
/// them time.
const SLOW_SCAN_MS: u64 = 250;

/// What a pass over version control turned up.
#[derive(Debug, Clone, Default)]
pub struct Recovered {
    pub events: Vec<Event>,
    /// Commit the scan reached, so the next one can start from it.
    pub head: Option<String>,
    /// Whether git was present and the workspace was inside a repository.
    pub available: bool,
    /// Whether the locally available history stops short of the beginning, as
    /// in a shallow clone.
    pub truncated: bool,
    /// Whether the repository carries changed-path Bloom filters.
    pub filtered: bool,
    pub duration_ms: u64,
}

impl Recovered {
    /// Whether the user should be told about `git commit-graph write`.
    ///
    /// Only when the scan was actually slow: the filters cost time and space
    /// inside someone else's repository, so the advice has to be earned.
    pub fn suggests_filters(&self) -> bool {
        self.available && !self.filtered && self.duration_ms >= SLOW_SCAN_MS
    }
}

/// Reads task history out of version control, degrading to nothing when there
/// is none to read.
///
/// This spawns subprocesses and walks a commit graph, so it belongs to a
/// deferred action rather than to a key handler or a frame.
pub fn recover(workspace_root: &Path) -> Recovered {
    let Some(repository) = Repository::locate(workspace_root) else {
        return Recovered::default();
    };

    let cache_path = cache_path_for(workspace_root);
    let cached = cache_path.as_deref().and_then(read_cache);

    // A cached commit that is no longer an ancestor of the head is a history
    // that was rewritten under us; the range would silently miss everything the
    // rebase moved, so the scan starts again from the beginning.
    let since = cached
        .as_ref()
        .map(|(commit, _)| commit.clone())
        .filter(|commit| repository.is_ancestor(commit));
    let mut events = match &since {
        Some(_) => cached.map(|(_, events)| events).unwrap_or_default(),
        None => Vec::new(),
    };

    let started = SystemTime::now();
    let scanned = repository.log(since.as_deref());
    let duration_ms = started
        .elapsed()
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default();

    match scanned {
        Some(raw) => events.extend(parse_log(&raw, &repository.changes_prefix)),
        // The scan failed after the repository was found — an unreadable object
        // database, a broken pathspec. The cached events are still true.
        None => {
            return Recovered {
                events,
                head: since,
                available: true,
                truncated: repository.shallow,
                filtered: repository.filtered,
                duration_ms,
            };
        }
    }

    let head = repository.head();
    if let (Some(path), Some(head)) = (cache_path.as_deref(), head.as_deref()) {
        write_cache(path, head, &events);
    }

    Recovered {
        events,
        head,
        available: true,
        truncated: repository.shallow,
        filtered: repository.filtered,
        duration_ms,
    }
}

/// The repository a workspace sits in, and what is known about its shape.
struct Repository {
    root: PathBuf,
    /// Path of the changes directory relative to the repository root, which is
    /// both the pathspec of the scan and the prefix stripped from every diff
    /// header it produces.
    changes_prefix: String,
    shallow: bool,
    filtered: bool,
}

impl Repository {
    fn locate(workspace_root: &Path) -> Option<Self> {
        let output = capture(
            workspace_root,
            &["rev-parse", "--show-toplevel", "--absolute-git-dir"],
        )?;

        let mut lines = output.lines();
        let root = PathBuf::from(lines.next()?.trim());
        let git_dir = PathBuf::from(lines.next()?.trim());
        // The grafted-commit file is how git itself records a shallow clone,
        // and reading it works whatever version is installed.
        let shallow = git_dir.join("shallow").exists();

        // Both sides have to be resolved the same way before one can be a
        // prefix of the other: git answers with a real path, and the caller may
        // have been given a relative one or reached the workspace by symlink.
        let changes = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf())
            .join("changes");
        let relative = changes
            .strip_prefix(root.canonicalize().unwrap_or_else(|_| root.clone()))
            .ok()?;
        let changes_prefix = relative.to_str()?.replace('\\', "/");

        Some(Self {
            root,
            changes_prefix,
            shallow,
            filtered: has_changed_path_filters(&git_dir),
        })
    }

    fn head(&self) -> Option<String> {
        capture(&self.root, &["rev-parse", "HEAD"]).map(|value| value.trim().to_string())
    }

    fn is_ancestor(&self, commit: &str) -> bool {
        std::process::Command::new(GIT)
            .args(["merge-base", "--is-ancestor", commit, "HEAD"])
            .current_dir(&self.root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// One pass over the changes directory, in a format this crate pins rather
    /// than inherits: the user's own diff configuration must not decide whether
    /// their history can be read.
    fn log(&self, since: Option<&str>) -> Option<String> {
        let format = format!("--format={COMMIT_MARK}%H{HEADER_FIELD}%at");
        let mut arguments = vec![
            "-c",
            "diff.noprefix=false",
            "log",
            "--patch",
            "--no-ext-diff",
            "--no-color",
            "--no-renames",
            "--no-merges",
            "--unified=0",
            "--reverse",
            &format,
        ];

        let range = since.map(|commit| format!("{commit}..HEAD"));
        if let Some(range) = &range {
            arguments.push(range);
        }
        arguments.push("--");
        arguments.push(&self.changes_prefix);

        capture(&self.root, &arguments)
    }
}

/// Runs git, returning its output only when it succeeded.
fn capture(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new(GIT)
        .args(arguments)
        .current_dir(directory)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;

    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether the repository's commit graph carries changed-path Bloom filters.
///
/// They are what turns a 206 ms walk into an 80 ms one on a large repository,
/// and `gc.writeCommitGraph` does not write them. Detecting their absence is
/// worth a suggestion; writing them is not this tool's decision to make, since
/// it means 6.5 MB inside someone else's repository.
fn has_changed_path_filters(git_dir: &Path) -> bool {
    let info = git_dir.join("objects/info");
    let mut candidates = vec![info.join("commit-graph")];

    if let Ok(entries) = std::fs::read_dir(info.join("commit-graphs")) {
        candidates.extend(entries.filter_map(|entry| Some(entry.ok()?.path())));
    }

    candidates.iter().any(|path| carries_bloom_chunk(path))
}

/// Looks for the `BIDX` chunk in a commit-graph file's table of contents.
fn carries_bloom_chunk(path: &Path) -> bool {
    let Ok(raw) = std::fs::read(path) else {
        return false;
    };
    if raw.len() < 8 || &raw[..4] != b"CGPH" {
        return false;
    }

    // Header: magic, version, hash version, chunk count, base graph count.
    let chunks = usize::from(raw[6]);
    let table = &raw[8..];

    (0..=chunks)
        .filter_map(|index| table.get(index * 12..index * 12 + 4))
        .any(|id| id == b"BIDX")
}

/// Turns one `git log --patch` pass into completion events.
///
/// Only checkbox lines matter, and only the transitions between them: a commit
/// that reworks a task and ticks it in the same breath removes one line and
/// adds another, which reads as the new wording being completed. That is the
/// same bounded inaccuracy [`TaskIdentity`] carries everywhere else.
fn parse_log(raw: &str, changes_prefix: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let mut at = 0u64;
    let mut file: Option<String> = None;
    let mut added: Vec<(TaskIdentity, String, bool)> = Vec::new();
    let mut removed: Vec<(TaskIdentity, String, bool)> = Vec::new();

    // A file's hunks are only complete once the next file, or the next commit,
    // begins; flushing is therefore what both of those trigger.
    macro_rules! flush {
        () => {
            if let Some(change) = file.take() {
                events.extend(transitions(&change, &added, &removed, at));
            }
            added.clear();
            removed.clear();
        };
    }

    for line in raw.lines() {
        if let Some(header) = line.strip_prefix(COMMIT_MARK) {
            flush!();
            at = header
                .split(HEADER_FIELD)
                .nth(1)
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or_default();
            continue;
        }

        if let Some(path) = line.strip_prefix("+++ b/") {
            flush!();
            file = change_of(path.trim(), changes_prefix);
            continue;
        }
        // A deleted file: its removals are not transitions, they are a removal.
        if line.starts_with("+++ /dev/null") {
            flush!();
            continue;
        }
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@") {
            continue;
        }

        if file.is_none() {
            continue;
        }
        let Some(rest) = line
            .strip_prefix('+')
            .map(|rest| (true, rest))
            .or_else(|| line.strip_prefix('-').map(|rest| (false, rest)))
        else {
            continue;
        };
        let (is_addition, body) = rest;
        let Some(done) = checkbox_state(body.trim()) else {
            continue;
        };

        let label = label_of(body.trim());
        let entry = (TaskIdentity::of(body), label, done);
        if is_addition {
            added.push(entry);
        } else {
            removed.push(entry);
        }
    }

    flush!();
    events
}

/// The events one file's added and removed checkbox lines amount to.
fn transitions(
    change: &str,
    added: &[(TaskIdentity, String, bool)],
    removed: &[(TaskIdentity, String, bool)],
    at: u64,
) -> Vec<Event> {
    let mut events = Vec::new();
    let before: BTreeMap<TaskIdentity, bool> = removed
        .iter()
        .map(|(identity, _, done)| (*identity, *done))
        .collect();
    let after: BTreeMap<TaskIdentity, bool> = added
        .iter()
        .map(|(identity, _, done)| (*identity, *done))
        .collect();

    for (identity, label, done) in added {
        match before.get(identity) {
            // The same line moved within the file, or was only reindented.
            Some(was) if was == done => {}
            Some(_) => events.push(
                Event::new(
                    at,
                    if *done {
                        EventKind::Done
                    } else {
                        EventKind::Undone
                    },
                    change,
                )
                .about(*identity, label),
            ),
            None => {
                events.push(Event::new(at, EventKind::TaskAdded, change).about(*identity, label));
                if *done {
                    events.push(Event::new(at, EventKind::Done, change).about(*identity, label));
                }
            }
        }
    }

    for (identity, label, _) in removed {
        if !after.contains_key(identity) {
            events.push(Event::new(at, EventKind::TaskRemoved, change).about(*identity, label));
        }
    }

    events
}

/// The change a diffed path belongs to.
fn change_of(path: &str, changes_prefix: &str) -> Option<String> {
    let relative = path.strip_prefix(changes_prefix)?.trim_start_matches('/');
    let mut parts = relative.split('/');
    let first = parts.next().filter(|part| !part.is_empty())?;

    // `changes/archive/<id>/tasks.md` is the same change, after shipping.
    let (id, nested) = if first == "archive" {
        (parts.next()?, parts.next().is_some())
    } else {
        (first, parts.next().is_some())
    };

    Some(if nested {
        id.to_string()
    } else {
        // A single-file proposal: the change is the file stem.
        id.strip_suffix(".md").unwrap_or(id).to_string()
    })
}

/// Task text with the list marker and the checkbox removed.
fn label_of(trimmed: &str) -> String {
    trimmed
        .trim_start_matches(['-', '*', '+'])
        .trim_start()
        .trim_start_matches(['[', ']', 'x', 'X', ' ', '✓'])
        .trim()
        .to_string()
}

/// Where a workspace's version-control scan is cached.
fn cache_path_for(workspace_root: &Path) -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var(CACHE_ENV)
        && !explicit.trim().is_empty()
    {
        return Some(PathBuf::from(explicit));
    }
    Some(cache_root()?.join(DIRECTORY).join(key_for(workspace_root)))
}

/// Reads a cached scan and the commit it was taken at.
fn read_cache(path: &Path) -> Option<(String, Vec<Event>)> {
    let raw = std::fs::read_to_string(path).ok()?;
    let commit = raw
        .lines()
        .next()?
        .strip_prefix("# scanned ")?
        .trim()
        .to_string();
    (!commit.is_empty()).then(|| (commit, decode_all(&raw)))
}

/// Writes a scan out. Best effort: an unwritable cache costs the next scan its
/// speed, not its result.
fn write_cache(path: &Path, head: &str, events: &[Event]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut body = format!("# scanned {head}\n");
    for event in events {
        body.push_str(&event.encode());
        body.push('\n');
    }
    let _ = std::fs::write(path, body);
}

// ---------------------------------------------------------------------------
// Reconciling the two sources
// ---------------------------------------------------------------------------

/// Merges the observed record with what version control gave up.
///
/// A completion known to both appears once, timed from the observation: that is
/// the moment the task was finished, where the commit is the moment the work
/// was recorded. This is also what repairs a squash merge — eight completions
/// flattened onto one commit keep their eight observed times, because the
/// recovered ones are the copies that get dropped.
pub fn reconcile(observed: &[Event], recovered: &[Event]) -> Vec<Event> {
    let seen: std::collections::BTreeSet<(&str, EventKind, Option<TaskIdentity>)> = observed
        .iter()
        .filter(|event| event.kind.is_task())
        .map(|event| (event.change.as_str(), event.kind, event.task))
        .collect();

    let mut merged: Vec<Event> = recovered
        .iter()
        .filter(|event| {
            !event.kind.is_task()
                || !seen.contains(&(event.change.as_str(), event.kind, event.task))
        })
        .cloned()
        .collect();
    merged.extend_from_slice(observed);

    merged.sort_by(|left, right| {
        left.at
            .cmp(&right.at)
            .then_with(|| left.change.cmp(&right.change))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    merged
}

// ---------------------------------------------------------------------------
// What the history implies
// ---------------------------------------------------------------------------

/// One completed task, with how long it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub at: u64,
    pub change: String,
    pub label: String,
    /// Elapsed time from the task first appearing to it being completed.
    ///
    /// `None` when the history does not reach back far enough to hold its first
    /// appearance, which is honest where a zero would not be.
    pub elapsed: Option<u64>,
}

/// What the history implies about one change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSummary {
    pub change: String,
    /// Tasks done and tasks in total, as the workspace has them now. Progress
    /// is a fact about the present; the history is what dates it.
    pub done: usize,
    pub total: usize,
    /// From the first completion to the most recent one, or to archival.
    pub elapsed: Option<u64>,
    /// Median elapsed time of the tasks completed in this change.
    pub median: Option<u64>,
    /// That median applied to the tasks still outstanding.
    pub estimate: Option<u64>,
    /// When the change was last heard from, which is what orders the summary.
    pub last_seen: u64,
}

/// The merged history and everything derived from it.
#[derive(Debug, Clone, Default)]
pub struct Timeline {
    /// Every event, newest first.
    pub events: Vec<Event>,
    /// The most recent completion of each task, newest first.
    pub completions: Vec<Completion>,
    pub changes: Vec<ChangeSummary>,
    /// Whether version control was read at all.
    pub scanned: bool,
    /// Whether what could be read stops short of the beginning.
    pub truncated: bool,
    /// Whether writing changed-path filters would be worth suggesting.
    pub suggest_filters: bool,
}

impl Timeline {
    /// Derives the timeline from a merged event list.
    pub fn build(events: Vec<Event>, workspace: &Workspace) -> Self {
        let mut first_seen: BTreeMap<(String, TaskIdentity), u64> = BTreeMap::new();
        for event in &events {
            if let Some(task) = event.task {
                let key = (event.change.clone(), task);
                let seen = first_seen.entry(key).or_insert(event.at);
                *seen = (*seen).min(event.at);
            }
        }

        // The most recent completion of each task: a tick, an untick and a
        // re-tick are three events but one completed task.
        let mut latest: BTreeMap<(String, TaskIdentity), Completion> = BTreeMap::new();
        for event in events.iter().filter(|event| event.kind == EventKind::Done) {
            let Some(task) = event.task else { continue };
            let key = (event.change.clone(), task);
            let started = first_seen.get(&key).copied().unwrap_or(event.at);

            let completion = Completion {
                at: event.at,
                change: event.change.clone(),
                label: event.label.clone(),
                elapsed: (started < event.at).then(|| event.at - started),
            };
            latest
                .entry(key)
                .and_modify(|held| {
                    if completion.at >= held.at {
                        *held = completion.clone();
                    }
                })
                .or_insert(completion);
        }

        let mut completions: Vec<Completion> = latest.into_values().collect();
        completions.sort_by_key(|completion| std::cmp::Reverse(completion.at));

        let changes = summarise(&events, &completions, workspace);
        let mut events = events;
        events.reverse();

        Self {
            events,
            completions,
            changes,
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The summary for one change, when the history has anything on it.
    pub fn change(&self, id: &str) -> Option<&ChangeSummary> {
        self.changes.iter().find(|summary| summary.change == id)
    }
}

fn summarise(
    events: &[Event],
    completions: &[Completion],
    workspace: &Workspace,
) -> Vec<ChangeSummary> {
    let mut ids: Vec<String> = Vec::new();
    let mut last_seen: BTreeMap<&str, u64> = BTreeMap::new();
    let mut archived_at: BTreeMap<&str, u64> = BTreeMap::new();

    for event in events {
        if !last_seen.contains_key(event.change.as_str()) {
            ids.push(event.change.clone());
        }
        let seen = last_seen.entry(event.change.as_str()).or_insert(event.at);
        *seen = (*seen).max(event.at);

        if event.kind == EventKind::ChangeArchived {
            archived_at.insert(event.change.as_str(), event.at);
        }
    }

    let mut summaries: Vec<ChangeSummary> = ids
        .iter()
        .map(|id| {
            let mine: Vec<&Completion> = completions
                .iter()
                .filter(|completion| completion.change == *id)
                .collect();

            let elapsed = match (
                mine.iter().map(|completion| completion.at).min(),
                archived_at
                    .get(id.as_str())
                    .copied()
                    .or_else(|| mine.iter().map(|completion| completion.at).max()),
            ) {
                (Some(first), Some(last)) if last > first => Some(last - first),
                _ => None,
            };

            let mut spans: Vec<u64> = mine
                .iter()
                .filter_map(|completion| completion.elapsed)
                .collect();
            let median = median_of(&mut spans);

            // Progress is read from the workspace rather than counted out of
            // the history: the history knows what happened, the files know what
            // is true now, and an archived change is no longer in either list.
            let change = workspace.find_change(id);
            let total = change.map_or(0, |change| change.tasks_total);
            let done = change.map_or(mine.len(), |change| change.tasks_done);
            let outstanding = total.saturating_sub(done) as u64;

            ChangeSummary {
                change: id.clone(),
                done,
                total,
                elapsed,
                median,
                // Nothing to derive from is nothing to say: a change with no
                // completed task gets no estimate rather than an estimate of
                // zero, which would read as "no work left".
                estimate: median
                    .filter(|_| outstanding > 0)
                    .map(|median| median * outstanding),
                last_seen: last_seen.get(id.as_str()).copied().unwrap_or_default(),
            }
        })
        .collect();

    summaries.sort_by_key(|summary| std::cmp::Reverse(summary.last_seen));
    summaries
}

fn median_of(spans: &mut [u64]) -> Option<u64> {
    if spans.is_empty() {
        return None;
    }
    spans.sort_unstable();
    let middle = spans.len() / 2;
    Some(if spans.len().is_multiple_of(2) {
        (spans[middle - 1] + spans[middle]) / 2
    } else {
        spans[middle]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renumbering_and_reindenting_leave_a_task_alone() {
        let original = TaskIdentity::of("- [ ] 1.1 Create the plugin registry");

        for variant in [
            "- [x] 1.1 Create the plugin registry",
            "  - [ ] 2.4 Create the plugin registry",
            "- [ ] 12.  Create   the plugin   registry",
            "* [X] Create the Plugin Registry",
            "- [ ] 3.2.1) create the plugin registry",
        ] {
            assert_eq!(TaskIdentity::of(variant), original, "{variant}");
        }
    }

    #[test]
    fn rewording_a_task_makes_it_another_task() {
        let original = TaskIdentity::of("- [ ] 1.1 Create the plugin registry");
        assert_ne!(
            TaskIdentity::of("- [ ] 1.1 Create the extension registry"),
            original
        );
    }

    #[test]
    fn a_number_is_only_dropped_when_it_is_the_numbering() {
        // The leading token is the outline number; a number inside the text is
        // part of what the task says.
        assert_eq!(
            normalise("- [ ] 2. Support 3 backends"),
            "support 3 backends"
        );
        assert_eq!(normalise("- [ ] 2020 was the year"), "2020 was the year");
    }

    #[test]
    fn records_survive_a_round_trip() {
        let event = Event::new(1_754_700_000, EventKind::Done, "add-oauth").about(
            TaskIdentity::of("- [x] 1.1 Pick\ta library"),
            "1.1 Pick\ta library",
        );

        let decoded = Event::decode(&event.encode()).expect("a record");
        assert_eq!(decoded, event);
        assert!(!event.encode().contains('\n'));
    }

    #[test]
    fn a_malformed_line_is_skipped_rather_than_fatal() {
        let good = Event::new(10, EventKind::ChangeCreated, "add-oauth").encode();
        let raw = format!("not a record\n{good}\n17\tdone\ttruncated");

        let events = decode_all(&raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].change, "add-oauth");
    }

    #[test]
    fn events_survive_a_save_and_load_cycle() {
        let (_guard, path) = redirect_for_test("roundtrip");
        let root = Path::new("/tmp/openspectation-history-workspace");

        let mut store = Store::open(root);
        assert!(store.events().is_empty());
        store.append(&[
            Event::new(100, EventKind::ChangeCreated, "add-oauth"),
            Event::new(140, EventKind::Done, "add-oauth").about(
                TaskIdentity::of("- [x] 1.1 Pick a library"),
                "1.1 Pick a library",
            ),
        ]);

        let reopened = Store::open(root);
        assert_eq!(reopened.events().len(), 2);
        assert_eq!(reopened.events()[1].label, "1.1 Pick a library");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_truncated_final_line_costs_only_that_line() {
        let (_guard, path) = redirect_for_test("truncated");
        let root = Path::new("/tmp/openspectation-history-workspace");

        let mut store = Store::open(root);
        store.append(&[Event::new(100, EventKind::ChangeCreated, "add-oauth")]);

        // What a crash mid-append leaves behind.
        let raw = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("{raw}17\tdo")).unwrap();

        let reopened = Store::open(root);
        assert_eq!(reopened.events().len(), 1);

        let _ = std::fs::remove_file(&path);
    }

    fn state(done: &[&str], pending: &[&str], stamp: u64) -> ChangeState {
        let task = |label: &str, done: bool| TaskState {
            identity: TaskIdentity::of(label),
            label: label.to_string(),
            done,
        };

        ChangeState {
            stamp: watch::stamp_of(stamp, None),
            status: SpecStatus::Review,
            archived: false,
            tasks: done
                .iter()
                .map(|label| task(label, true))
                .chain(pending.iter().map(|label| task(label, false)))
                .collect(),
        }
    }

    fn snapshot(state: ChangeState) -> Snapshot {
        Snapshot {
            changes: [("add-oauth".to_string(), state)].into_iter().collect(),
        }
    }

    /// The reason the diff compares task lists rather than counting them: two
    /// transitions in one interval leave the count exactly where it was.
    #[test]
    fn un_ticking_one_task_and_ticking_another_is_two_events() {
        let before = snapshot(state(&["1.1 Pick a library"], &["1.2 Wire the flow"], 10));
        let after = snapshot(state(&["1.2 Wire the flow"], &["1.1 Pick a library"], 20));

        let events = before.diff(&after, 100);
        assert_eq!(events.len(), 2, "{events:#?}");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == EventKind::Done)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == EventKind::Undone)
                .count(),
            1
        );
    }

    #[test]
    fn a_change_whose_files_did_not_move_is_not_diffed() {
        let before = snapshot(state(&[], &["1.1 Pick a library"], 10));
        // The same stamp with a different task list is a contradiction the
        // fingerprint is trusted over: it is what keeps the tick cheap.
        let after = snapshot(state(&["1.1 Pick a library"], &[], 10));

        assert!(before.diff(&after, 100).is_empty());
    }

    #[test]
    fn archiving_is_seen_through_the_rename_it_actually_is() {
        let before = snapshot(state(&[], &["1.1 Pick a library"], 10));
        let mut archived = state(&["1.1 Pick a library"], &[], 20);
        archived.archived = true;
        let after = Snapshot {
            changes: [("2026-01-01-add-oauth".to_string(), archived)]
                .into_iter()
                .collect(),
        };

        let events = before.diff(&after, 100);
        assert_eq!(events.len(), 1, "{events:#?}");
        assert_eq!(events[0].kind, EventKind::ChangeArchived);
        assert_eq!(events[0].change, "add-oauth");
    }

    /// Captured from `git log --patch --unified=0` with the format this module
    /// asks for. The second commit both rewords a task and ticks it, which is
    /// the case task identity is deliberately allowed to get wrong in only one
    /// bounded way: the new wording is what completes.
    const LOG: &str = "\u{1}aaa1\u{1f}1700000000
diff --git a/openspec/changes/add-oauth/tasks.md b/openspec/changes/add-oauth/tasks.md
new file mode 100644
--- /dev/null
+++ b/openspec/changes/add-oauth/tasks.md
@@ -0,0 +1,2 @@
+- [ ] 1.1 Pick a library
+- [ ] 1.2 Wire the flow
\u{1}aaa2\u{1f}1700003600
diff --git a/openspec/changes/add-oauth/tasks.md b/openspec/changes/add-oauth/tasks.md
--- a/openspec/changes/add-oauth/tasks.md
+++ b/openspec/changes/add-oauth/tasks.md
@@ -1 +1 @@
-- [ ] 1.1 Pick a library
+- [x] 1.1 Pick an OAuth library
";

    #[test]
    fn the_parser_reads_transitions_out_of_captured_diff_output() {
        let events = parse_log(LOG, "openspec/changes");

        assert_eq!(
            events
                .iter()
                .map(|event| (event.at, event.kind, event.label.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (1_700_000_000, EventKind::TaskAdded, "1.1 Pick a library"),
                (1_700_000_000, EventKind::TaskAdded, "1.2 Wire the flow"),
                // The reworded line is a task arriving already done, and the
                // wording it replaced is a task that went away.
                (
                    1_700_003_600,
                    EventKind::TaskAdded,
                    "1.1 Pick an OAuth library"
                ),
                (1_700_003_600, EventKind::Done, "1.1 Pick an OAuth library"),
                (1_700_003_600, EventKind::TaskRemoved, "1.1 Pick a library"),
            ]
        );
        assert!(events.iter().all(|event| event.change == "add-oauth"));
    }

    #[test]
    fn a_diffed_path_names_the_change_it_belongs_to() {
        let prefix = "openspec/changes";
        assert_eq!(
            change_of("openspec/changes/add-oauth/tasks.md", prefix).as_deref(),
            Some("add-oauth")
        );
        assert_eq!(
            change_of("openspec/changes/archive/2026-01-01-old/tasks.md", prefix).as_deref(),
            Some("2026-01-01-old")
        );
        assert_eq!(
            change_of("openspec/changes/single.md", prefix).as_deref(),
            Some("single")
        );
        assert_eq!(change_of("src/main.rs", prefix), None);
    }

    fn done_at(at: u64, label: &str) -> Event {
        Event::new(at, EventKind::Done, "add-oauth").about(TaskIdentity::of(label), label)
    }

    #[test]
    fn a_completion_observed_and_later_committed_appears_once() {
        let observed = [done_at(1_000, "1.1 Pick a library")];
        // The same completion, as version control dates it: the commit came
        // four hours after the work.
        let recovered = [done_at(15_400, "1.1 Pick a library")];

        let merged = reconcile(&observed, &recovered);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].at, 1_000);
    }

    #[test]
    fn completions_squashed_into_one_commit_keep_their_observed_times() {
        let observed = [
            done_at(1_000, "1.1 Pick a library"),
            done_at(5_000, "1.2 Wire the flow"),
        ];
        // A squash merge: both completions attributed to one instant.
        let recovered = [
            done_at(90_000, "1.1 Pick a library"),
            done_at(90_000, "1.2 Wire the flow"),
        ];

        let merged = reconcile(&observed, &recovered);
        assert_eq!(
            merged.iter().map(|event| event.at).collect::<Vec<_>>(),
            vec![1_000, 5_000]
        );
    }

    #[test]
    fn a_completion_only_version_control_knows_about_survives() {
        let merged = reconcile(&[], &[done_at(90_000, "1.1 Pick a library")]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].at, 90_000);
    }

    /// A workspace with one change whose tasks are all outstanding.
    fn scratch_workspace(name: &str) -> (PathBuf, Workspace) {
        let root = std::env::temp_dir().join(format!(
            "openspectation-history-ws-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("changes/add-oauth")).unwrap();
        std::fs::create_dir_all(root.join("specs")).unwrap();
        std::fs::write(root.join("openspec.yaml"), "version: 1\n").unwrap();
        std::fs::write(
            root.join("changes/add-oauth/tasks.md"),
            "- [ ] 1.1 Pick a library\n- [ ] 1.2 Wire the flow\n",
        )
        .unwrap();

        let workspace = Workspace::load(&root).unwrap();
        (root, workspace)
    }

    #[test]
    fn a_change_with_nothing_completed_yields_no_figures() {
        let (root, workspace) = scratch_workspace("no-completions");

        let timeline = Timeline::build(
            vec![
                Event::new(100, EventKind::ChangeCreated, "add-oauth"),
                Event::new(100, EventKind::TaskAdded, "add-oauth")
                    .about(TaskIdentity::of("1.1 Pick a library"), "1.1 Pick a library"),
            ],
            &workspace,
        );

        let summary = timeline.change("add-oauth").expect("a summary");
        assert_eq!((summary.done, summary.total), (0, 2));
        assert_eq!(summary.elapsed, None, "a zero would be a lie");
        assert_eq!(summary.median, None);
        assert_eq!(summary.estimate, None);
        assert!(timeline.completions.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn what_is_left_is_estimated_from_what_was_done() {
        let (root, workspace) = scratch_workspace("estimate");
        let label = "1.1 Pick a library";

        // Appeared at 100, completed at 700: an hour and forty minutes elapsed,
        // and one of the two tasks is still outstanding.
        let timeline = Timeline::build(
            vec![
                Event::new(100, EventKind::TaskAdded, "add-oauth")
                    .about(TaskIdentity::of(label), label),
                done_at(700, label),
            ],
            &workspace,
        );

        let completion = &timeline.completions[0];
        assert_eq!(completion.elapsed, Some(600));

        let summary = timeline.change("add-oauth").expect("a summary");
        assert_eq!(summary.median, Some(600));
        // The workspace still has both tasks unticked, so one task's worth of
        // the median is what remains.
        assert_eq!(summary.estimate, Some(1_200));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_the_most_recent_completion_of_a_task_counts() {
        let (root, workspace) = scratch_workspace("retick");
        let label = "1.1 Pick a library";

        let timeline = Timeline::build(
            vec![
                Event::new(100, EventKind::TaskAdded, "add-oauth")
                    .about(TaskIdentity::of(label), label),
                done_at(300, label),
                Event::new(400, EventKind::Undone, "add-oauth")
                    .about(TaskIdentity::of(label), label),
                done_at(900, label),
            ],
            &workspace,
        );

        assert_eq!(timeline.completions.len(), 1);
        assert_eq!(timeline.completions[0].at, 900);
        assert_eq!(timeline.completions[0].elapsed, Some(800));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The whole recovery path, against a repository built for the purpose.
    ///
    /// Skipped where git is absent, which is also the case the recovery itself
    /// has to survive: the interface works without it, with a smaller history.
    #[test]
    fn a_repository_gives_up_the_completions_it_recorded() {
        let (_guard, _path) = redirect_for_test("recover");
        if std::process::Command::new(GIT)
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }

        let root = std::env::temp_dir().join(format!(
            "openspectation-history-repo-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = root.join("openspec");
        let tasks = workspace.join("changes/add-oauth/tasks.md");
        std::fs::create_dir_all(tasks.parent().unwrap()).unwrap();

        let git = |arguments: &[&str]| {
            std::process::Command::new(GIT)
                .args(arguments)
                .current_dir(&root)
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
                .output()
                .expect("git");
        };

        git(&["init", "--quiet"]);
        std::fs::write(&tasks, "- [ ] 1.1 Pick a library\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "add the task"]);
        std::fs::write(&tasks, "- [x] 1.1 Pick a library\n").unwrap();
        git(&["commit", "--quiet", "-a", "-m", "tick it"]);

        let recovered = recover(&workspace);
        assert!(recovered.available, "the repository was not found");
        assert!(!recovered.truncated);
        assert_eq!(
            recovered
                .events
                .iter()
                .filter(|event| event.kind == EventKind::Done)
                .count(),
            1,
            "{:#?}",
            recovered.events
        );
        assert!(
            recovered
                .events
                .iter()
                .all(|event| event.change == "add-oauth")
        );

        // The second pass reads only the range since the cached commit, and
        // must report the same history rather than a doubled one.
        let again = recover(&workspace);
        assert_eq!(again.events.len(), recovered.events.len());
        assert_eq!(again.head, recovered.head);

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(std::env::var(CACHE_ENV).unwrap_or_default());
    }

    #[test]
    fn a_workspace_outside_version_control_recovers_nothing_and_says_so() {
        let (_guard, _path) = redirect_for_test("no-repository");
        let root = std::env::temp_dir().join(format!(
            "openspectation-history-bare-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("changes")).unwrap();

        let recovered = recover(&root);
        assert!(!recovered.available);
        assert!(recovered.events.is_empty());
        assert!(!recovered.truncated, "no history is not a truncated one");
        assert!(!recovered.suggests_filters());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn two_workspaces_never_share_a_file() {
        assert_ne!(
            key_for(Path::new("/one/openspec")),
            key_for(Path::new("/two/openspec"))
        );
        assert!(key_for(Path::new("/one/openspec")).starts_with("openspec-"));
    }

    /// Points [`PATH_ENV`] at a file of this test's own.
    ///
    /// It takes the settings guard rather than one of its own: there is a
    /// single set of environment variables in a threaded test binary, and two
    /// mutexes over it would guard nothing.
    fn redirect_for_test(name: &str) -> (std::sync::MutexGuard<'static, ()>, PathBuf) {
        let guard = crate::domain::settings::TEST_ENV
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let path = std::env::temp_dir().join(format!(
            "openspectation-history-{}-{name}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        // Safe: the caller holds the `TEST_ENV` mutex, so no other thread
        // touches the environment while the variable moves.
        unsafe { std::env::set_var(PATH_ENV, &path) };
        (guard, path)
    }
}
