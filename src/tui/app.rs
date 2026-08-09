//! Interactive application state and input handling.
//!
//! The dashboard is a workbench, not a viewer: it browses a change, opens its
//! artifacts, reads the requirement delta, runs validation, toggles tasks, edits
//! a document in place and archives the change. Everything that writes to disk
//! goes through the domain layer, and everything that shells out is queued as a
//! [`Pending`] action so the frame showing "working…" is drawn before the
//! process blocks the loop.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::ListState;
use rust_i18n::t;

use crate::domain::change::ChangeProposal;
use crate::domain::delta::{self, CapabilityDelta};
use crate::domain::editor::Buffer;
use crate::domain::filter::Filter;
use crate::domain::history::{self, Event, EventKind, Recovered, Snapshot, Store, Timeline};
use crate::domain::settings::Settings;
use crate::domain::spec::SpecDocument;
use crate::domain::tasks::{self, TaskLine};
use crate::domain::watch::{self, Fingerprint};
use crate::domain::workspace::Workspace;
use crate::error::Result;
use crate::i18n;
use crate::tui::command::{self, Command};
use crate::tui::diffview;
use crate::tui::markdown;
use crate::tui::theme::{Palette, Theme};
use crate::tui::tree::{ArtifactTree, NodeKind};
use crate::tui::ui::SIDEBAR_MIN;

/// Ticks a transient message stays on screen (about four seconds at 250 ms).
const TOAST_TICKS: u64 = 16;

/// Rows scrolled by a single mouse wheel notch.
const WHEEL_STEP: i32 = 3;

/// Ticks between two scans of the workspace tree (about a second at 250 ms).
const WATCH_INTERVAL: u64 = 4;

/// Lines kept in the activity log.
const LOG_CAPACITY: usize = 64;

/// Rows a single entry of the change list occupies.
pub const CHANGE_ROW_HEIGHT: u16 = 2;

/// Ticks within which a second press on the divider is one gesture, not two
/// (about half a second at 250 ms).
const DOUBLE_CLICK_TICKS: u64 = 2;

/// Which screen is on display.
///
/// The timeline is its own view rather than a pane of the editor: it does not
/// fit in the log, and the dashboard is a summary of the present where this is
/// a summary of the past.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Editor,
    Dashboard,
    Activity,
}

/// What the sidebar list is enumerating.
///
/// The archive is a tab rather than a filter or a hidden toggle: it is the one
/// part of a workspace whose entries are read-only history, and a tab is the
/// only affordance that says so without being pressed first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Changes,
    Archive,
    Specs,
    Project,
}

impl Tab {
    pub const ORDER: [Self; 4] = [Self::Changes, Self::Archive, Self::Specs, Self::Project];

    pub fn title(self) -> String {
        match self {
            Self::Changes => t!("tui.tab.changes"),
            Self::Archive => t!("tui.tab.archive"),
            Self::Specs => t!("tui.tab.specs"),
            Self::Project => t!("tui.tab.project"),
        }
        .to_string()
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Changes => 0,
            Self::Archive => 1,
            Self::Specs => 2,
            Self::Project => 3,
        }
    }

    /// Whether this tab lists change proposals rather than documents.
    pub const fn holds_changes(self) -> bool {
        matches!(self, Self::Changes | Self::Archive)
    }

    fn stepped(self, delta: isize) -> Self {
        let next = (self.index() as isize + delta).rem_euclid(Self::ORDER.len() as isize);
        Self::ORDER[next as usize]
    }
}

/// Which pane takes the navigation keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    List,
    Artifacts,
    Viewer,
}

/// What the main pane is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerMode {
    Document,
    Diff,
    Tasks,
    Edit,
}

impl ViewerMode {
    pub fn label(self) -> String {
        match self {
            Self::Document => t!("tui.viewer.document"),
            Self::Diff => t!("tui.viewer.diff"),
            Self::Tasks => t!("tui.viewer.tasks"),
            Self::Edit => t!("tui.viewer.edit"),
        }
        .to_string()
    }
}

/// What the keyboard is currently feeding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
    Command,
    Confirm,
    Help,
    /// Preferences panel: the palette and the interface language.
    Settings,
    /// Every key goes to the text buffer.
    Edit,
}

/// A row of the settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Theme,
    Language,
}

impl Setting {
    pub const ORDER: [Self; 2] = [Self::Theme, Self::Language];

    pub fn label(self) -> String {
        match self {
            Self::Theme => t!("tui.settings.theme"),
            Self::Language => t!("tui.settings.language"),
        }
        .to_string()
    }

    pub fn hint(self) -> String {
        match self {
            Self::Theme => t!("tui.settings.theme_hint"),
            Self::Language => t!("tui.settings.language_hint"),
        }
        .to_string()
    }
}

/// Work that must run between two frames rather than inside a key handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    Validate {
        strict: bool,
        target: Option<String>,
    },
    Archive {
        skip_specs: bool,
        target: String,
    },
    Editor(PathBuf),
    /// Reads task history out of version control.
    ///
    /// A subprocess walking a commit graph, which on a large repository is over
    /// a second: run inline it would drop six frames with nothing on screen
    /// saying why.
    ScanHistory,
}

/// What a confirmed question does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirmed {
    Run(Pending),
    Quit,
}

/// Colour role of an activity log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Accent,
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    expires_at: u64,
}

/// A question awaiting `y` or `n`.
#[derive(Debug, Clone)]
pub struct Confirm {
    pub question: String,
    pub detail: String,
    /// Label of the accepting key, which names what is about to happen.
    pub accept: String,
    action: Confirmed,
}

/// Screen rectangles from the last frame, used to route mouse events.
#[derive(Debug, Clone, Copy, Default)]
pub struct Regions {
    pub list: Rect,
    pub tabs: [Rect; 4],
    pub artifacts: Rect,
    /// The whole sidebar column, which a drag measures a new width against.
    pub sidebar: Rect,
    /// The divider between the sidebar and the viewer, widened for hit testing.
    pub divider: Rect,
    pub viewer: Rect,
    pub log: Rect,
    pub command: Rect,
    /// The gear in the header.
    pub settings: Rect,
    /// One per row of the preferences panel, so a click can land on a value.
    pub setting_rows: [Rect; 2],
}

/// Cached, pre-wrapped viewer content so scrolling never re-parses Markdown.
#[derive(Debug, Default)]
struct ViewerCache {
    key: Option<ViewerKey>,
    lines: Vec<Line<'static>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewerKey {
    mode: u8,
    tab: u8,
    /// What is on display, not where it sits in a list. A filter can leave the
    /// selected index alone while changing what that index resolves to, and a
    /// key made of positions would call that a cache hit and keep rendering the
    /// previous document under the new title.
    document: Option<PathBuf>,
    change: Option<String>,
    width: u16,
    locale: String,
    revision: u64,
}

pub struct App {
    pub workspace: Workspace,
    pub theme: Theme,
    pub palette: Palette,
    pub view: View,
    pub tab: Tab,
    pub pane: Pane,
    pub mode: Mode,
    pub viewer: ViewerMode,

    /// One selection per tab, so switching back returns where you were.
    pub states: [ListState; 4],
    pub tree: ArtifactTree,

    pub tasks: Vec<TaskLine>,
    pub task_cursor: usize,
    pub deltas: Vec<CapabilityDelta>,

    /// The document being edited in place, kept across a trip to the preview.
    pub buffer: Option<Buffer>,
    /// First visible column of the editor, so a long line can be reached.
    pub edit_offset: usize,

    pub filter: Filter,
    /// Raw text of the filter, kept so the search box can be re-opened on it.
    pub filter_text: String,
    /// Line being typed in the search box or on the command line.
    pub draft: String,
    pub history: Vec<String>,
    history_cursor: Option<usize>,

    pub scroll: u16,
    pub log: Vec<LogLine>,
    pub toast: Option<Toast>,
    pub confirm: Option<Confirm>,
    pub pending: Option<Pending>,

    /// User preferences, as read from the configuration directory. Changing one
    /// applies immediately and is written back straight away: a preferences
    /// panel with a save button is a panel that can be left half applied.
    pub settings: Settings,
    pub setting_cursor: usize,

    /// Sidebar width in columns, or `None` while it sizes itself.
    ///
    /// Not a `u16` carrying a default: a default is indistinguishable from a
    /// chosen width, which makes returning to automatic unimplementable and
    /// freezes the automatic formula for anyone who ever opened the interface.
    pub sidebar_width: Option<u16>,
    /// Whether the divider is following the pointer.
    dragging_divider: bool,
    /// Tick of the last press on the divider, which is how a double click is
    /// recognised: the terminal reports presses, never pairs of them.
    divider_pressed_at: Option<u64>,

    /// Everything this interface saw happen, on this workspace, ever.
    pub record: Store,
    /// What version control gave up, once it has been asked.
    recovered: Recovered,
    /// Whether the scan has been requested; it runs on demand, so a user who
    /// never opens the timeline never pays for it.
    scanned: bool,
    /// The merged history, rebuilt whenever either source moves.
    timeline: Option<Timeline>,
    pub activity_scroll: u16,
    pub activity_height: u16,
    pub activity_total: usize,

    pub should_quit: bool,
    pub tick: u64,
    /// Version of the upstream CLI, probed once at start-up.
    pub cli_version: Option<String>,
    revision: u64,
    fingerprint: Fingerprint,

    pub regions: Regions,
    pub viewer_total: usize,
    pub viewer_height: u16,
    pub viewer_width: u16,
    cache: ViewerCache,
}

impl App {
    pub fn new(workspace: Workspace) -> Self {
        let fingerprint = watch::fingerprint(&workspace);
        let history = Store::open(&workspace.root);
        let settings = Settings::load();
        let mut app = Self {
            workspace,
            theme: Palette::default().theme(),
            palette: Palette::default(),
            view: View::Editor,
            tab: Tab::Changes,
            pane: Pane::List,
            mode: Mode::Normal,
            viewer: ViewerMode::Document,
            states: std::array::from_fn(|_| ListState::default()),
            tree: ArtifactTree::default(),
            tasks: Vec::new(),
            task_cursor: 0,
            deltas: Vec::new(),
            buffer: None,
            edit_offset: 0,
            filter: Filter::All,
            filter_text: String::new(),
            draft: String::new(),
            history: Vec::new(),
            history_cursor: None,
            scroll: 0,
            log: Vec::new(),
            toast: None,
            confirm: None,
            pending: None,
            sidebar_width: settings.sidebar_width,
            settings,
            setting_cursor: 0,
            dragging_divider: false,
            divider_pressed_at: None,
            record: history,
            recovered: Recovered::default(),
            scanned: false,
            timeline: None,
            activity_scroll: 0,
            activity_height: 1,
            activity_total: 0,
            should_quit: false,
            tick: 0,
            cli_version: crate::domain::openspec::version(),
            revision: 0,
            fingerprint,
            regions: Regions::default(),
            viewer_total: 0,
            viewer_height: 1,
            viewer_width: 1,
            cache: ViewerCache::default(),
        };

        for tab in Tab::ORDER {
            let populated = !visible(&app.workspace, tab, &app.filter).is_empty();
            app.states[tab.index()].select(populated.then_some(0));
        }
        app.sync_selection();
        app
    }

    /// Selects the palette, as `--theme` asks for.
    #[must_use]
    pub const fn with_palette(mut self, palette: Palette) -> Self {
        self.set_palette(palette);
        self
    }

    /// The one place `theme` and `palette` are written, so they cannot drift.
    ///
    /// Rendered lines carry their colors, so the viewer cache is stale the
    /// moment the palette changes; the revision bump is what discards it.
    const fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
        self.theme = palette.theme();
        self.revision += 1;
    }

    /// Opens the workbench on a specific change, as `--change` asks for.
    pub fn focus_change(&mut self, id: &str) -> bool {
        let Some(absolute) = self.workspace.change_index(id) else {
            return false;
        };

        // Naming an archived change is asking to see it, so the tab follows.
        self.tab = if self.workspace.changes[absolute].archived {
            Tab::Archive
        } else {
            Tab::Changes
        };
        self.filter = Filter::All;
        self.filter_text.clear();

        let position = self
            .visible()
            .iter()
            .position(|candidate| *candidate == absolute);
        self.states[self.tab.index()].select(position);
        self.sync_selection();
        true
    }

    // ---- Selection ------------------------------------------------------

    /// Indices, into the pool of the active tab, of the entries on display.
    pub fn visible(&self) -> Vec<usize> {
        visible(&self.workspace, self.tab, &self.filter)
    }

    /// Rows the sidebar list is currently showing.
    pub fn visible_len(&self) -> usize {
        self.visible().len()
    }

    /// How many entries a tab would list, ignoring the filter.
    pub fn tab_len(&self, tab: Tab) -> usize {
        visible(&self.workspace, tab, &Filter::All).len()
    }

    pub fn list_state(&self) -> &ListState {
        &self.states[self.tab.index()]
    }

    pub fn selected_change(&self) -> Option<&ChangeProposal> {
        selected_change(&self.workspace, self.tab, &self.filter, self.selected())
    }

    fn selected(&self) -> Option<usize> {
        self.list_state().selected()
    }

    /// Documents the artifact tree is built from.
    pub fn artifacts(&self) -> Vec<&SpecDocument> {
        artifacts(&self.workspace, self.tab, &self.filter, self.selected())
    }

    /// Document shown by the viewer, if any.
    pub fn open_document(&self) -> Option<&SpecDocument> {
        let index = self.tree.selected_document()?;
        self.artifacts().get(index).copied()
    }

    /// Path shown in the viewer header.
    pub fn open_path(&self) -> Option<String> {
        let root = self.artifact_root()?;
        let document = self.open_document()?;
        Some(
            document
                .path
                .strip_prefix(&root)
                .unwrap_or(&document.path)
                .display()
                .to_string(),
        )
    }

    fn artifact_root(&self) -> Option<PathBuf> {
        artifact_root(&self.workspace, self.tab, &self.filter, self.selected())
    }

    /// Rebuilds everything derived from the sidebar selection.
    fn sync_selection(&mut self) {
        let selected = self.selected();
        let root = artifact_root(&self.workspace, self.tab, &self.filter, selected);
        let documents = artifacts(&self.workspace, self.tab, &self.filter, selected);
        self.tree.rebuild(root.as_deref(), &documents);

        let change = selected_change(&self.workspace, self.tab, &self.filter, selected);
        self.tasks = match change {
            Some(change) => tasks::collect(change),
            None => Vec::new(),
        };
        self.task_cursor = self.task_cursor.min(self.tasks.len().saturating_sub(1));

        self.deltas = match change {
            Some(change) => delta::for_change(change, &self.workspace),
            None => Vec::new(),
        };

        // Editing owns the pane until it is left; nothing derived from the
        // sidebar may move the cursor or the scroll out from under a typist.
        if self.viewer == ViewerMode::Edit {
            return;
        }

        // A mode with nothing behind it would render an empty pane; fall back
        // to the document rather than leaving the user staring at a blank.
        if self.viewer == ViewerMode::Tasks && self.tasks.is_empty() {
            self.viewer = ViewerMode::Document;
        }
        if self.viewer == ViewerMode::Diff && self.deltas.is_empty() {
            self.viewer = ViewerMode::Document;
        }

        self.scroll = 0;
    }

    fn clamp_list(&mut self) {
        let length = self.visible_len();
        let state = &mut self.states[self.tab.index()];
        match state.selected() {
            Some(index) if index >= length => state.select(length.checked_sub(1)),
            None if length > 0 => state.select(Some(0)),
            _ => {}
        }
    }

    // ---- Viewer ---------------------------------------------------------

    /// Rebuilds the pre-wrapped viewer content when anything it depends on
    /// changed.
    pub fn sync_viewer(&mut self, width: u16) {
        let key = ViewerKey {
            mode: match self.viewer {
                ViewerMode::Document => 0,
                ViewerMode::Diff => 1,
                ViewerMode::Tasks => 2,
                ViewerMode::Edit => 3,
            },
            tab: self.tab.index() as u8,
            document: self.open_document().map(|document| document.path.clone()),
            change: self.selected_change().map(|change| change.id.clone()),
            width,
            locale: i18n::current(),
            revision: self.revision,
        };

        if self.cache.key.as_ref() == Some(&key) {
            self.viewer_total = self.cache.lines.len();
            return;
        }

        self.cache.lines = match self.viewer {
            ViewerMode::Diff => diffview::render(&self.deltas, &self.theme, width),
            // Tasks and the editor are rendered live, one row per entry, so the
            // cursor and the click target stay on the same line.
            ViewerMode::Tasks | ViewerMode::Edit => Vec::new(),
            ViewerMode::Document => match self.open_document() {
                Some(document) if !document.body.trim().is_empty() => {
                    markdown::render(&document.body, &self.theme, width)
                }
                _ => Vec::new(),
            },
        };

        self.cache.key = Some(key);
        self.viewer_total = self.cache.lines.len();
    }

    /// Slice of the viewer currently visible, cloned for rendering.
    pub fn viewer_window(&self, height: u16) -> Vec<Line<'static>> {
        self.cache
            .lines
            .iter()
            .skip(usize::from(self.scroll))
            .take(usize::from(height))
            .cloned()
            .collect()
    }

    pub fn viewer_is_empty(&self) -> bool {
        match self.viewer {
            ViewerMode::Tasks => self.tasks.is_empty(),
            ViewerMode::Edit => self.buffer.is_none(),
            _ => self.cache.lines.is_empty(),
        }
    }

    /// Total scrollable rows for the active viewer mode.
    pub fn viewer_rows(&self) -> usize {
        match self.viewer {
            ViewerMode::Tasks => self.tasks.len(),
            ViewerMode::Edit => self.buffer.as_ref().map_or(0, Buffer::len),
            _ => self.viewer_total,
        }
    }

    pub fn max_scroll(&self) -> u16 {
        u16::try_from(self.viewer_rows())
            .unwrap_or(u16::MAX)
            .saturating_sub(self.viewer_height)
    }

    pub fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }

    /// Keeps the highlighted task inside the visible window.
    pub fn reveal_task(&mut self) {
        if self.viewer != ViewerMode::Tasks {
            return;
        }
        let cursor = u16::try_from(self.task_cursor).unwrap_or(u16::MAX);
        self.reveal_row(cursor);
    }

    /// Keeps the text cursor inside the visible window, in both axes.
    pub fn reveal_cursor(&mut self) {
        let Some(buffer) = &self.buffer else {
            return;
        };
        let (row, column) = (buffer.row, buffer.column);

        self.reveal_row(u16::try_from(row).unwrap_or(u16::MAX));

        // One column is kept for the cursor block itself, which sits one past
        // the last character when the line is being appended to.
        let width = usize::from(self.viewer_width.max(2)) - 1;
        if column < self.edit_offset {
            self.edit_offset = column;
        } else if column >= self.edit_offset + width {
            self.edit_offset = column + 1 - width;
        }
    }

    fn reveal_row(&mut self, row: u16) {
        let height = self.viewer_height.max(1);
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + height {
            self.scroll = row.saturating_sub(height - 1);
        }
    }

    // ---- Activity log ---------------------------------------------------

    pub fn say(&mut self, text: impl Into<String>, tone: Tone) {
        self.log.push(LogLine {
            text: text.into(),
            tone,
        });
        if self.log.len() > LOG_CAPACITY {
            let excess = self.log.len() - LOG_CAPACITY;
            self.log.drain(..excess);
        }
    }

    pub fn notify(&mut self, message: String) {
        self.toast = Some(Toast {
            message,
            expires_at: self.tick + TOAST_TICKS,
        });
    }

    // ---- Ticking --------------------------------------------------------

    pub fn on_tick(&mut self) {
        self.tick += 1;

        if let Some(toast) = &self.toast
            && self.tick >= toast.expires_at
        {
            self.toast = None;
        }

        // The tree is watched by fingerprint rather than by notification, so a
        // change written by an agent shows up without pressing anything. Not
        // while typing: reloading under an open buffer would be a surprise
        // dressed up as a feature.
        if self.mode != Mode::Edit && self.tick.is_multiple_of(WATCH_INTERVAL) {
            let current = watch::fingerprint(&self.workspace);
            if current != self.fingerprint {
                self.fingerprint = current;
                // The generic line is the fallback, not the message: it is what
                // is left to say when nothing identifiable happened.
                if let Ok(0) = self.reload() {
                    self.say(t!("tui.log.detected").to_string(), Tone::Accent);
                }
            }
        }
    }

    /// Re-reads the workspace and reports what actually changed.
    ///
    /// Returns how many events the reload turned out to be, which is what lets
    /// the caller keep the generic "something moved" line for the case where
    /// nothing identifiable happened.
    fn reload(&mut self) -> Result<usize> {
        let before = Snapshot::capture(&self.workspace);
        self.workspace.reload()?;
        self.revision += 1;
        self.fingerprint = watch::fingerprint(&self.workspace);
        self.clamp_list();
        self.sync_selection();
        Ok(self.observe(&before))
    }

    /// Records and reports everything that happened across a reload.
    fn observe(&mut self, before: &Snapshot) -> usize {
        let events = before.diff(&Snapshot::capture(&self.workspace), history::now());
        if events.is_empty() {
            return 0;
        }

        self.record.append(&events);
        for event in &events {
            let tone = match event.kind {
                EventKind::Done | EventKind::ChangeCreated => Tone::Success,
                EventKind::Undone | EventKind::TaskRemoved => Tone::Warning,
                EventKind::ChangeArchived => Tone::Accent,
                _ => Tone::Muted,
            };
            self.say(describe(event), tone);
        }
        // The timeline reads from the merged history, so it has to see these
        // without waiting for another version-control scan.
        self.timeline = None;
        events.len()
    }

    // ---- History --------------------------------------------------------

    /// The merged history, rebuilt only when one of its sources moved.
    pub fn timeline(&mut self) -> &Timeline {
        if self.timeline.is_none() {
            let merged = history::reconcile(self.record.events(), &self.recovered.events);
            let mut timeline = Timeline::build(merged, &self.workspace);
            timeline.scanned = self.recovered.available;
            timeline.truncated = self.recovered.truncated;
            timeline.suggest_filters = self.recovered.suggests_filters();
            self.timeline = Some(timeline);
        }
        self.timeline.as_ref().expect("just built")
    }

    /// Opens the timeline, asking version control for the rest of it on the
    /// first visit.
    ///
    /// Never while a document is being edited: the scan is a subprocess, and
    /// the interface does not touch the workspace under an open buffer.
    fn open_activity(&mut self) {
        self.view = View::Activity;
        self.activity_scroll = 0;

        if self.scanned || self.mode == Mode::Edit || self.viewer == ViewerMode::Edit {
            return;
        }
        self.scanned = true;
        // The log lives in the editor, and this view is not it: the toast is
        // what actually puts "working" on screen before the loop stops
        // answering. Both, because one is the record and the other is the
        // notice.
        self.say(t!("tui.log.scanning").to_string(), Tone::Accent);
        self.notify(t!("tui.log.scanning").to_string());
        self.pending = Some(Pending::ScanHistory);
    }

    /// Folds a completed version-control scan into the history.
    pub fn report_history(&mut self, recovered: Recovered) {
        let (found, available) = (recovered.events.len(), recovered.available);
        self.recovered = recovered;
        self.timeline = None;

        if !available {
            self.say(format!("  {}", t!("tui.log.no_repository")), Tone::Muted);
            return;
        }
        self.say(
            format!("  {}", t!("tui.log.recovered", count = found)),
            Tone::Success,
        );
        if self.recovered.truncated {
            self.say(
                format!("  {}", t!("tui.activity.incomplete")),
                Tone::Warning,
            );
        }
        if self.recovered.suggests_filters() {
            self.say(format!("  {}", t!("tui.log.suggest_filters")), Tone::Muted);
        }
    }

    /// Reads the workspace's version-control history. Runs off the event loop.
    pub fn scan_history(&self) -> Recovered {
        history::recover(&self.workspace.root)
    }

    // ---- Deferred work --------------------------------------------------

    /// Hands the queued action to the event loop, which owns the terminal.
    pub fn take_pending(&mut self) -> Option<Pending> {
        self.pending.take()
    }

    /// Reports the outcome of a validation run.
    pub fn report_validation(&mut self, report: &crate::domain::openspec::ValidationReport) {
        use crate::domain::openspec::Severity;

        for item in &report.items {
            let glyph = if item.valid { "✔" } else { "✖" };
            let tone = if item.valid {
                Tone::Success
            } else {
                Tone::Danger
            };
            self.say(format!("  {glyph} {} · {}", item.id, item.kind), tone);

            for finding in &item.findings {
                let tone = match finding.severity {
                    Severity::Error => Tone::Danger,
                    Severity::Warning => Tone::Warning,
                    Severity::Info => Tone::Muted,
                };
                self.say(
                    format!(
                        "      {} {} — {}",
                        finding.severity.glyph(),
                        finding.location,
                        finding.message
                    ),
                    tone,
                );
            }
        }

        let summary = t!(
            "tui.log.validated",
            passed = report.passed,
            failed = report.failed,
            errors = report.count(Severity::Error),
            warnings = report.count(Severity::Warning),
            ms = report.total_duration()
        )
        .to_string();
        let tone = if report.is_ok() {
            Tone::Success
        } else {
            Tone::Danger
        };
        self.say(format!("  {summary}"), tone);
    }

    /// Reports the outcome of an archive run and reloads the workspace.
    pub fn report_archive(&mut self, lines: &[String]) {
        for line in lines {
            self.say(format!("  {line}"), Tone::Accent);
        }
        if let Err(error) = self.reload() {
            self.say(format!("  {}", error.message()), Tone::Danger);
        } else {
            self.say(format!("  {}", t!("tui.log.archived")), Tone::Success);
        }
    }

    /// Reports a failure from any deferred action.
    pub fn report_failure(&mut self, error: &crate::error::Error) {
        self.say(format!("  {}", error.message()), Tone::Danger);
        if let Some(hint) = error.hint() {
            self.say(format!("  {hint}"), Tone::Muted);
        }
    }

    /// Called after `$EDITOR` returns.
    pub fn after_editor(&mut self, path: &Path) {
        match self.reload() {
            Ok(_) => self.say(
                format!("  {}", t!("tui.log.edited", path = display(path))),
                Tone::Success,
            ),
            Err(error) => self.report_failure(&error),
        }
    }

    // ---- Keyboard -------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.mode {
            Mode::Search => self.on_search_key(key),
            Mode::Command => self.on_command_key(key),
            Mode::Confirm => self.on_confirm_key(key),
            Mode::Help => self.on_help_key(key),
            Mode::Settings => self.on_settings_key(key),
            Mode::Edit => self.on_edit_key(key),
            Mode::Normal => self.on_normal_key(key)?,
        }
        Ok(())
    }

    fn on_normal_key(&mut self, key: KeyEvent) -> Result<()> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('c') if control => self.request_quit(),
            KeyCode::Char('d') if control => self.scroll_by(self.page() / 2),
            KeyCode::Char('u') if control => self.scroll_by(-(self.page() / 2)),
            KeyCode::Char('s') if control => self.save_buffer(),

            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Esc => self.on_escape(),
            KeyCode::Char('?') | KeyCode::F(1) => self.mode = Mode::Help,
            KeyCode::Char(',') | KeyCode::F(2) => self.open_settings(),
            KeyCode::Char(':') => self.open_command(String::new()),
            KeyCode::Char('/') => {
                self.draft = self.filter_text.clone();
                self.mode = Mode::Search;
            }

            KeyCode::Char('g') => self.toggle_dashboard(),
            KeyCode::Char('H') => self.toggle_activity(),
            KeyCode::Char('s') => self.set_tab(self.tab.stepped(1)),
            KeyCode::Char('S') => self.set_tab(self.tab.stepped(-1)),
            KeyCode::Char('1') => self.set_tab(Tab::Changes),
            KeyCode::Char('2') => self.set_tab(Tab::Archive),
            KeyCode::Char('3') => self.set_tab(Tab::Specs),
            KeyCode::Char('4') => self.set_tab(Tab::Project),
            KeyCode::Char('A') => self.set_tab(match self.tab {
                Tab::Archive => Tab::Changes,
                _ => Tab::Archive,
            }),
            KeyCode::Char('d') => self.set_viewer(ViewerMode::Diff),
            KeyCode::Char('t') => self.set_viewer(ViewerMode::Tasks),
            KeyCode::Char('m') => self.set_viewer(ViewerMode::Document),
            KeyCode::Char('v') => self.request_validate(true, None),
            KeyCode::Char('a') => self.request_archive(false, None),
            KeyCode::Char('e') => self.begin_edit(),
            KeyCode::Char('E') => self.request_editor(None),
            KeyCode::Char(' ') => self.toggle_task(),
            KeyCode::Char('r') => match self.reload() {
                Ok(_) => self.notify(t!("tui.toast.reloaded").to_string()),
                Err(error) => self.report_failure(&error),
            },
            KeyCode::Char('L') => {
                let locale = i18n::cycle();
                // The placeholder is `language`, not `locale`: `t!` reserves
                // the latter for choosing which catalogue to read from.
                self.notify(
                    t!(
                        "tui.toast.locale_changed",
                        language = i18n::display_name(&locale)
                    )
                    .to_string(),
                );
            }

            // Scoped to the sidebar holding the focus, which keeps them clear
            // of the editor and leaves the pair meaningless nowhere else.
            KeyCode::Char('<') if self.sidebar_has_focus() => self.nudge_sidebar(-1),
            KeyCode::Char('>') if self.sidebar_has_focus() => self.nudge_sidebar(1),

            KeyCode::Tab => self.cycle_pane(1),
            KeyCode::BackTab => self.cycle_pane(-1),
            // Inside the tree `h` and `l` fold and unfold, which is what they
            // mean everywhere else; the arrows keep switching panes.
            KeyCode::Char('h') if self.pane == Pane::Artifacts => self.collapse_node(),
            KeyCode::Char('l') if self.pane == Pane::Artifacts => self.expand_node(),
            KeyCode::Left | KeyCode::Char('h') => self.focus_left(),
            KeyCode::Right | KeyCode::Char('l') => self.focus_right(),
            KeyCode::Enter => self.activate(),
            KeyCode::Down | KeyCode::Char('j') => self.step(1),
            KeyCode::Up | KeyCode::Char('k') => self.step(-1),
            KeyCode::PageDown => self.scroll_by(self.page()),
            KeyCode::PageUp => self.scroll_by(-self.page()),
            KeyCode::Home => self.go_first(),
            KeyCode::End => self.go_last(),
            _ => {}
        }

        Ok(())
    }

    fn on_escape(&mut self) {
        if self.view != View::Editor {
            self.view = View::Editor;
            return;
        }
        if self.viewer != ViewerMode::Document {
            self.set_viewer(ViewerMode::Document);
            return;
        }
        if self.filter.is_active() {
            self.clear_filter();
            return;
        }
        if !self.log.is_empty() {
            self.log.clear();
            return;
        }
        self.request_quit();
    }

    fn on_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.mode = Mode::Normal;
            }
            _ => self.mode = Mode::Normal,
        }
    }

    // ---- Settings -------------------------------------------------------

    pub fn open_settings(&mut self) {
        self.setting_cursor = 0;
        self.mode = Mode::Settings;
    }

    /// Keys while the preferences panel is open.
    ///
    /// Unlike the help overlay, which any key dismisses, this one is a form:
    /// only the keys that mean something act, so a mistyped letter does not
    /// close a panel the user is reading.
    fn on_settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char(',') | KeyCode::F(2) | KeyCode::Char('q') => {
                self.mode = Mode::Normal;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => self.step_setting(1),
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => self.step_setting(-1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ') => {
                self.cycle_setting(1);
            }
            KeyCode::Left | KeyCode::Char('h') => self.cycle_setting(-1),
            _ => {}
        }
    }

    fn step_setting(&mut self, delta: isize) {
        let length = Setting::ORDER.len() as isize;
        self.setting_cursor = (self.setting_cursor as isize + delta).rem_euclid(length) as usize;
    }

    /// Moves the highlighted preference to its next value and writes it out.
    fn cycle_setting(&mut self, delta: isize) {
        match Setting::ORDER[self.setting_cursor.min(Setting::ORDER.len() - 1)] {
            Setting::Theme => {
                let palette = step_through(&Palette::ALL, self.palette, delta);
                self.set_palette(palette);
                self.settings.theme = Some(palette.name().to_string());
            }
            Setting::Language => {
                let locales = i18n::available();
                let current = i18n::current();
                let index = locales
                    .iter()
                    .position(|locale| *locale == current)
                    .unwrap_or(0) as isize;
                let next = &locales[(index + delta).rem_euclid(locales.len() as isize) as usize];
                rust_i18n::set_locale(next);
                self.settings.language = Some(next.clone());
                // Every rendered string changes, and the viewer is cached.
                self.revision += 1;
            }
        }

        self.persist_settings();
    }

    /// Writes the preferences, reporting a failure without undoing the change.
    ///
    /// The setting is already live; refusing to apply it because the disk is
    /// read-only would trade a working session for a consistent one.
    fn persist_settings(&mut self) {
        if let Err(error) = self.settings.save() {
            self.say(
                format!("  {}", t!("tui.log.settings_failed")),
                Tone::Warning,
            );
            self.say(format!("  {}", error.message()), Tone::Muted);
        }
    }

    // ---- Sidebar width --------------------------------------------------

    /// Whether one of the sidebar panes is the one taking the keys.
    const fn sidebar_has_focus(&self) -> bool {
        matches!(self.pane, Pane::List | Pane::Artifacts)
    }

    /// Records the width the user asked for.
    ///
    /// Only the floor is applied here. It belongs to the sidebar and holds on
    /// every terminal, where the ceiling depends on how wide this one happens
    /// to be and so belongs to the frame being drawn.
    fn set_sidebar_width(&mut self, columns: u16) {
        self.sidebar_width = Some(columns.max(SIDEBAR_MIN));
    }

    /// Writes the current width out, once the gesture setting it has finished.
    fn store_sidebar_width(&mut self) {
        if self.settings.sidebar_width == self.sidebar_width {
            return;
        }
        self.settings.sidebar_width = self.sidebar_width;
        self.persist_settings();
    }

    /// Hands the width back to the layout and forgets the stored one.
    fn reset_sidebar_width(&mut self) {
        self.sidebar_width = None;
        self.store_sidebar_width();
        self.notify(t!("tui.toast.sidebar_auto").to_string());
    }

    /// Moves the divider by a column, from whatever the last frame drew.
    fn nudge_sidebar(&mut self, delta: i32) {
        // A sidebar of no width is a terminal too narrow to have drawn one,
        // where a width is neither shown nor meant.
        let drawn = self.regions.sidebar.width;
        if drawn == 0 {
            return;
        }

        let current = i32::from(self.sidebar_width.unwrap_or(drawn));
        self.set_sidebar_width(u16::try_from(current + delta).unwrap_or(SIDEBAR_MIN));
        self.store_sidebar_width();
    }

    /// Feeds one mouse event to the divider, answering whether it took it.
    ///
    /// The divider holds the pointer for the whole gesture, so a drag that
    /// wanders into a pane is still a resize. A release that never arrives
    /// would otherwise leave the divider stuck to the pointer, which is why
    /// the grab is dropped by anything that is not a drag rather than by a
    /// release alone.
    fn drag_divider(&mut self, event: MouseEvent, at: Position) -> bool {
        match event.kind {
            MouseEventKind::Drag(MouseButton::Left) if self.dragging_divider => {
                let width =
                    at.x.saturating_sub(self.regions.sidebar.x)
                        .saturating_add(1);
                self.set_sidebar_width(width);
                true
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging_divider => {
                self.dragging_divider = false;
                // Once per gesture: a drag reports a column at a time, and a
                // file written on each of them turns a movement into dozens.
                self.store_sidebar_width();
                true
            }
            MouseEventKind::Down(MouseButton::Left) if self.regions.divider.contains(at) => {
                let again = self
                    .divider_pressed_at
                    .is_some_and(|tick| self.tick.saturating_sub(tick) <= DOUBLE_CLICK_TICKS);
                self.divider_pressed_at = Some(self.tick);

                if again {
                    self.dragging_divider = false;
                    self.divider_pressed_at = None;
                    self.reset_sidebar_width();
                } else {
                    self.dragging_divider = true;
                }
                true
            }
            _ => {
                self.dragging_divider = false;
                false
            }
        }
    }

    /// The value shown against a preference, and where it came from.
    pub fn setting_value(&self, setting: Setting) -> String {
        match setting {
            Setting::Theme => self.palette.name().to_string(),
            Setting::Language => i18n::display_name(&i18n::current()).to_string(),
        }
    }

    /// Keys while a document is being edited.
    ///
    /// Everything printable is text, so leaving is deliberate: `Esc` or
    /// `Ctrl+E` returns to the preview and `Ctrl+S` writes the file.
    fn on_edit_key(&mut self, key: KeyEvent) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Esc => {
                self.end_edit();
                return;
            }
            KeyCode::Char('e') if control => {
                self.end_edit();
                return;
            }
            KeyCode::Char('c') if control => {
                self.end_edit();
                return;
            }
            KeyCode::Char('s') if control => {
                self.save_buffer();
                return;
            }
            _ => {}
        }

        let height = isize::try_from(self.viewer_height.max(1)).unwrap_or(1);
        let Some(buffer) = &mut self.buffer else {
            return;
        };

        match key.code {
            KeyCode::Char(character) if !control => buffer.insert(character),
            KeyCode::Enter => buffer.newline(),
            KeyCode::Tab => buffer.indent(),
            KeyCode::Backspace => buffer.backspace(),
            KeyCode::Delete => buffer.delete(),
            KeyCode::Left => buffer.step_horizontal(-1),
            KeyCode::Right => buffer.step_horizontal(1),
            KeyCode::Up => buffer.step_vertical(-1),
            KeyCode::Down => buffer.step_vertical(1),
            KeyCode::PageUp => buffer.step_vertical(-height),
            KeyCode::PageDown => buffer.step_vertical(height),
            KeyCode::Home => buffer.home(),
            KeyCode::End => buffer.end(),
            _ => {}
        }

        self.reveal_cursor();
    }

    fn on_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.draft.clear();
                if self.filter.is_active() {
                    self.clear_filter();
                }
            }
            KeyCode::Enter => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                self.draft.pop();
                self.apply_draft_filter();
            }
            KeyCode::Char(character) => {
                self.draft.push(character);
                self.apply_draft_filter();
            }
            _ => {}
        }
    }

    fn on_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.draft.clear();
                self.history_cursor = None;
            }
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.draft);
                self.mode = Mode::Normal;
                self.history_cursor = None;
                self.run_command(&line);
            }
            KeyCode::Backspace => {
                self.draft.pop();
            }
            KeyCode::Up => self.recall_history(-1),
            KeyCode::Down => self.recall_history(1),
            KeyCode::Char(character) => self.draft.push(character),
            _ => {}
        }
    }

    fn on_confirm_key(&mut self, key: KeyEvent) {
        let accepted = matches!(key.code, KeyCode::Char('y' | 'Y') | KeyCode::Enter);
        let confirm = self.confirm.take();
        self.mode = Mode::Normal;

        let Some(confirm) = confirm else {
            return;
        };
        if !accepted {
            self.say(format!("  {}", t!("tui.log.cancelled")), Tone::Muted);
            return;
        }

        match confirm.action {
            Confirmed::Quit => self.should_quit = true,
            Confirmed::Run(action) => {
                if let Pending::Archive { target, .. } = &action {
                    self.say(
                        t!("tui.log.archiving", change = target.clone()).to_string(),
                        Tone::Accent,
                    );
                }
                self.pending = Some(action);
            }
        }
    }

    fn recall_history(&mut self, delta: isize) {
        if self.history.is_empty() {
            return;
        }
        let last = self.history.len() - 1;
        let next = match self.history_cursor {
            None if delta < 0 => last,
            None => return,
            Some(current) => (current as isize + delta).clamp(0, last as isize) as usize,
        };

        self.history_cursor = Some(next);
        self.draft = self.history[next].clone();
    }

    // ---- Mouse ----------------------------------------------------------

    pub fn on_mouse(&mut self, event: MouseEvent) {
        let at = Position::new(event.column, event.row);
        let clicked = event.kind == MouseEventKind::Down(MouseButton::Left);

        if self.mode == Mode::Settings {
            if clicked {
                self.click_setting(at);
            }
            return;
        }

        // The gear is drawn over every view, so it answers from all of them.
        // Not from the editor, where the header is the only thing on screen
        // that is not the buffer and a click there would be an accident.
        if clicked && self.mode != Mode::Edit && self.regions.settings.contains(at) {
            self.open_settings();
            return;
        }

        if self.view == View::Activity && self.mode == Mode::Normal {
            match event.kind {
                MouseEventKind::ScrollDown => self.scroll_activity(WHEEL_STEP),
                MouseEventKind::ScrollUp => self.scroll_activity(-WHEEL_STEP),
                _ => {}
            }
            return;
        }

        if self.view != View::Editor || matches!(self.mode, Mode::Help | Mode::Confirm) {
            // A stray click must not act on a pane the overlay is covering.
            if clicked {
                self.mode = Mode::Normal;
            }
            return;
        }

        // The divider answers before any pane does: while it is being dragged
        // the pointer belongs to it, wherever it has got to.
        if self.drag_divider(event, at) {
            return;
        }

        match event.kind {
            MouseEventKind::ScrollDown => self.wheel(at, WHEEL_STEP),
            MouseEventKind::ScrollUp => self.wheel(at, -WHEEL_STEP),
            MouseEventKind::Down(MouseButton::Left) => self.click(at),
            _ => {}
        }
    }

    /// A click on a preference row moves the cursor there and advances the
    /// value; anywhere else closes the panel.
    fn click_setting(&mut self, at: Position) {
        for (index, row) in self.regions.setting_rows.iter().enumerate() {
            if row.contains(at) {
                self.setting_cursor = index;
                self.cycle_setting(1);
                return;
            }
        }
        self.mode = Mode::Normal;
    }

    fn wheel(&mut self, at: Position, delta: i32) {
        if self.regions.list.contains(at) {
            if self.mode == Mode::Edit {
                return;
            }
            self.pane = Pane::List;
            self.step_list(delta.signum() as isize);
        } else if self.regions.artifacts.contains(at) {
            if self.mode == Mode::Edit {
                return;
            }
            self.pane = Pane::Artifacts;
            self.tree.step(delta.signum() as isize);
            self.open_selected_artifact();
        } else if self.mode == Mode::Edit {
            let rows = isize::try_from(delta).unwrap_or(0);
            if let Some(buffer) = &mut self.buffer {
                buffer.step_vertical(rows);
            }
            self.reveal_cursor();
        } else {
            self.pane = Pane::Viewer;
            self.scroll_by(delta);
        }
    }

    fn click(&mut self, at: Position) {
        if self.mode == Mode::Edit {
            if self.regions.viewer.contains(at) {
                let row = usize::from(self.scroll) + usize::from(at.y - self.regions.viewer.y);
                let column = self.edit_offset + usize::from(at.x - self.regions.viewer.x);
                if let Some(buffer) = &mut self.buffer {
                    buffer.move_to(row, column);
                }
                self.reveal_cursor();
            }
            return;
        }

        if self.regions.command.contains(at) {
            self.open_command(String::new());
            return;
        }

        for tab in Tab::ORDER {
            if self.regions.tabs[tab.index()].contains(at) {
                self.set_tab(tab);
                return;
            }
        }

        if self.regions.list.contains(at) {
            self.pane = Pane::List;
            let row = (at.y - self.regions.list.y) / CHANGE_ROW_HEIGHT;
            let index = self.list_state().offset() + usize::from(row);
            if index < self.visible_len() {
                self.states[self.tab.index()].select(Some(index));
                self.sync_selection();
            }
            return;
        }

        if self.regions.artifacts.contains(at) {
            self.pane = Pane::Artifacts;
            let index = usize::from(at.y - self.regions.artifacts.y);
            if index < self.tree.rows().len() {
                self.tree.select(index);
                let directory = self
                    .tree
                    .selected_row()
                    .is_some_and(|row| row.kind == NodeKind::Directory);
                if directory {
                    self.toggle_tree_node();
                } else {
                    self.open_selected_artifact();
                }
            }
            return;
        }

        if self.regions.viewer.contains(at) {
            self.pane = Pane::Viewer;
            if self.viewer == ViewerMode::Tasks {
                let index = usize::from(self.scroll) + usize::from(at.y - self.regions.viewer.y);
                if index < self.tasks.len() {
                    self.task_cursor = index;
                    self.toggle_task();
                }
            }
        }
    }

    // ---- Navigation -----------------------------------------------------

    fn cycle_pane(&mut self, delta: isize) {
        const ORDER: [Pane; 3] = [Pane::List, Pane::Artifacts, Pane::Viewer];
        let current = ORDER
            .iter()
            .position(|pane| *pane == self.pane)
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(ORDER.len() as isize) as usize;
        self.pane = ORDER[next];
    }

    fn focus_left(&mut self) {
        self.pane = match self.pane {
            Pane::Viewer => Pane::Artifacts,
            _ => Pane::List,
        };
    }

    fn focus_right(&mut self) {
        self.pane = match self.pane {
            Pane::List => Pane::Artifacts,
            _ => Pane::Viewer,
        };
    }

    fn activate(&mut self) {
        match self.pane {
            Pane::List => self.pane = Pane::Artifacts,
            Pane::Artifacts => {
                let directory = self
                    .tree
                    .selected_row()
                    .is_some_and(|row| row.kind == NodeKind::Directory);
                if directory {
                    self.toggle_tree_node();
                } else {
                    self.open_selected_artifact();
                    self.pane = Pane::Viewer;
                }
            }
            Pane::Viewer if self.viewer == ViewerMode::Tasks => self.toggle_task(),
            Pane::Viewer => {}
        }
    }

    fn step(&mut self, delta: isize) {
        if self.view == View::Activity {
            self.scroll_activity(i32::try_from(delta).unwrap_or(0));
            return;
        }
        match self.pane {
            Pane::List => self.step_list(delta),
            Pane::Artifacts => {
                self.tree.step(delta);
                self.open_selected_artifact();
            }
            Pane::Viewer if self.viewer == ViewerMode::Tasks => {
                if self.tasks.is_empty() {
                    return;
                }
                let length = self.tasks.len() as isize;
                self.task_cursor = (self.task_cursor as isize + delta).rem_euclid(length) as usize;
                self.reveal_task();
            }
            Pane::Viewer => self.scroll_by(delta as i32),
        }
    }

    fn step_list(&mut self, delta: isize) {
        let length = self.visible_len();
        if length == 0 {
            return;
        }

        let state = &mut self.states[self.tab.index()];
        let current = state.selected().unwrap_or(0) as isize;
        state.select(Some((current + delta).rem_euclid(length as isize) as usize));

        self.sync_selection();
    }

    /// Folds or unfolds the directory under the tree cursor.
    ///
    /// The document set is resolved through the free helpers so the workspace
    /// borrow and the mutable tree borrow stay on disjoint fields.
    fn toggle_tree_node(&mut self) {
        let selected = self.selected();
        let root = artifact_root(&self.workspace, self.tab, &self.filter, selected);
        let documents = artifacts(&self.workspace, self.tab, &self.filter, selected);
        let _ = self.tree.toggle_selected(root.as_deref(), &documents);
    }

    fn collapse_node(&mut self) {
        let selected = self.selected();
        let root = artifact_root(&self.workspace, self.tab, &self.filter, selected);
        let documents = artifacts(&self.workspace, self.tab, &self.filter, selected);
        self.tree.collapse_or_parent(root.as_deref(), &documents);
    }

    fn expand_node(&mut self) {
        let selected = self.selected();
        let root = artifact_root(&self.workspace, self.tab, &self.filter, selected);
        let documents = artifacts(&self.workspace, self.tab, &self.filter, selected);
        self.tree.expand_or_child(root.as_deref(), &documents);
    }

    /// Shows the document under the tree cursor, keeping the viewer mode sane.
    fn open_selected_artifact(&mut self) {
        if self.tree.selected_document().is_none() {
            return;
        }
        if matches!(self.viewer, ViewerMode::Diff | ViewerMode::Edit) {
            self.viewer = ViewerMode::Document;
        }
        self.scroll = 0;
    }

    fn scroll_by(&mut self, delta: i32) {
        if self.view == View::Activity {
            self.scroll_activity(delta);
            return;
        }
        if self.viewer == ViewerMode::Tasks && self.pane == Pane::Viewer {
            self.step(delta.signum() as isize);
            return;
        }
        let maximum = i32::from(self.max_scroll());
        let next = (i32::from(self.scroll) + delta).clamp(0, maximum.max(0));
        self.scroll = u16::try_from(next).unwrap_or(0);
    }

    fn go_first(&mut self) {
        if self.view == View::Activity {
            self.activity_scroll = 0;
            return;
        }
        match self.pane {
            Pane::List if self.visible_len() > 0 => {
                self.states[self.tab.index()].select(Some(0));
                self.sync_selection();
            }
            Pane::Artifacts => self.tree.first(),
            _ => {
                self.task_cursor = 0;
                self.scroll = 0;
            }
        }
    }

    fn go_last(&mut self) {
        if self.view == View::Activity {
            self.scroll_activity(i32::MAX);
            return;
        }
        match self.pane {
            Pane::List => {
                let last = self.visible_len().checked_sub(1);
                self.states[self.tab.index()].select(last);
                self.sync_selection();
            }
            Pane::Artifacts => self.tree.last(),
            _ => {
                self.task_cursor = self.tasks.len().saturating_sub(1);
                self.scroll = self.max_scroll();
                self.reveal_task();
            }
        }
    }

    const fn page(&self) -> i32 {
        if self.viewer_height > 2 {
            self.viewer_height as i32 - 2
        } else {
            1
        }
    }

    // ---- Actions --------------------------------------------------------

    fn toggle_dashboard(&mut self) {
        self.view = match self.view {
            View::Dashboard => View::Editor,
            _ => View::Dashboard,
        };
    }

    fn toggle_activity(&mut self) {
        match self.view {
            View::Activity => self.view = View::Editor,
            _ => self.open_activity(),
        }
    }

    /// Scrolls the timeline, which owns its offset because it is a view rather
    /// than a mode of the viewer pane.
    fn scroll_activity(&mut self, delta: i32) {
        let maximum = i32::from(
            u16::try_from(self.activity_total)
                .unwrap_or(u16::MAX)
                .saturating_sub(self.activity_height),
        );
        let next = (i32::from(self.activity_scroll) + delta).clamp(0, maximum.max(0));
        self.activity_scroll = u16::try_from(next).unwrap_or(0);
    }

    fn set_tab(&mut self, tab: Tab) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        self.pane = Pane::List;
        self.viewer = ViewerMode::Document;
        self.clamp_list();
        self.sync_selection();
    }

    fn set_viewer(&mut self, mode: ViewerMode) {
        // Pressing the same key twice returns to the document, so a mode is
        // never a trap.
        let target = if self.viewer == mode && mode != ViewerMode::Document {
            ViewerMode::Document
        } else {
            mode
        };

        match target {
            ViewerMode::Diff if self.deltas.is_empty() => {
                self.notify(t!("tui.toast.no_deltas").to_string());
                return;
            }
            ViewerMode::Tasks if self.tasks.is_empty() => {
                self.notify(t!("tui.toast.no_tasks").to_string());
                return;
            }
            _ => {}
        }

        self.viewer = target;
        self.pane = Pane::Viewer;
        self.scroll = 0;
        if target == ViewerMode::Tasks {
            self.reveal_task();
        }
    }

    // ---- Editing --------------------------------------------------------

    /// Opens the document under the cursor for editing, or returns to the
    /// preview when it is already open.
    fn begin_edit(&mut self) {
        if self.viewer == ViewerMode::Edit {
            self.end_edit();
            return;
        }

        let Some(document) = self.open_document() else {
            self.notify(t!("tui.toast.no_document").to_string());
            return;
        };
        let path = document.path.clone();

        // A synthetic document — the `context:` block read out of the
        // configuration — has no file of its own to write back to.
        if !path.is_file() {
            self.notify(t!("tui.toast.not_a_file").to_string());
            return;
        }

        // Unsaved work outlives a trip to the preview, so the same file is
        // resumed rather than re-read.
        let resume = self
            .buffer
            .as_ref()
            .is_some_and(|buffer| buffer.path == path && buffer.dirty);
        if !resume {
            match Buffer::open(&path) {
                Ok(buffer) => self.buffer = Some(buffer),
                Err(error) => {
                    self.report_failure(&error);
                    return;
                }
            }
        }

        self.viewer = ViewerMode::Edit;
        self.mode = Mode::Edit;
        self.pane = Pane::Viewer;
        self.edit_offset = 0;
        self.scroll = 0;
        self.reveal_cursor();
    }

    /// Leaves the editor for the preview, keeping unsaved work in memory.
    fn end_edit(&mut self) {
        self.mode = Mode::Normal;
        self.viewer = ViewerMode::Document;
        self.scroll = 0;

        let dirty = self.buffer.as_ref().is_some_and(|buffer| buffer.dirty);
        if dirty {
            self.notify(t!("tui.toast.unsaved").to_string());
        } else {
            self.buffer = None;
        }
    }

    fn save_buffer(&mut self) {
        let Some(buffer) = &mut self.buffer else {
            self.notify(t!("tui.toast.nothing_to_save").to_string());
            return;
        };
        if !buffer.dirty {
            self.notify(t!("tui.toast.nothing_to_save").to_string());
            return;
        }

        let name = buffer.file_name();
        match buffer.save() {
            Ok(()) => {
                self.notify(t!("tui.toast.saved", path = name).to_string());
                if let Err(error) = self.reload() {
                    self.report_failure(&error);
                }
            }
            Err(error) => self.report_failure(&error),
        }
    }

    /// Quits, unless there is unsaved work to ask about first.
    fn request_quit(&mut self) {
        let Some(buffer) = &self.buffer else {
            self.should_quit = true;
            return;
        };
        if !buffer.dirty {
            self.should_quit = true;
            return;
        }

        self.confirm = Some(Confirm {
            question: t!("tui.confirm.discard", path = buffer.file_name()).to_string(),
            detail: t!("tui.confirm.discard_detail").to_string(),
            accept: t!("tui.confirm.accept_quit").to_string(),
            action: Confirmed::Quit,
        });
        self.mode = Mode::Confirm;
    }

    fn toggle_task(&mut self) {
        if self.tasks.is_empty() {
            self.notify(t!("tui.toast.no_tasks").to_string());
            return;
        }
        if self.viewer != ViewerMode::Tasks {
            self.set_viewer(ViewerMode::Tasks);
            return;
        }

        let Some(task) = self.tasks.get(self.task_cursor).cloned() else {
            return;
        };

        match tasks::toggle(&task) {
            Ok(done) => {
                let key = if done {
                    "tui.toast.task_done"
                } else {
                    "tui.toast.task_undone"
                };
                self.notify(
                    t!(key, task = crate::domain::util::truncate(&task.label, 48)).to_string(),
                );
                if let Err(error) = self.reload() {
                    self.report_failure(&error);
                }
                self.task_cursor = self.task_cursor.min(self.tasks.len().saturating_sub(1));
                self.reveal_task();
            }
            Err(error) => {
                self.report_failure(&error);
                let _ = self.reload();
            }
        }
    }

    /// Reports a missing CLI before an action that cannot work without it.
    fn cli_is_missing(&mut self) -> bool {
        if self.cli_version.is_some() {
            return false;
        }
        let error = crate::error::Error::CliMissing;
        self.report_failure(&error);
        true
    }

    fn request_validate(&mut self, strict: bool, target: Option<String>) {
        if self.cli_is_missing() {
            return;
        }
        let target = target.or_else(|| self.selected_change().map(|change| change.id.clone()));
        let label = target
            .clone()
            .unwrap_or_else(|| t!("tui.log.everything").to_string());

        self.say(
            t!("tui.log.validating", change = label).to_string(),
            Tone::Accent,
        );
        self.pending = Some(Pending::Validate { strict, target });
    }

    fn request_archive(&mut self, skip_specs: bool, target: Option<String>) {
        if self.cli_is_missing() {
            return;
        }

        let Some(change) = target
            .and_then(|id| {
                self.workspace
                    .find_change(&id)
                    .map(|change| (change.id.clone(), change.archived))
            })
            .or_else(|| {
                self.selected_change()
                    .map(|change| (change.id.clone(), change.archived))
            })
        else {
            self.notify(t!("tui.toast.no_change").to_string());
            return;
        };

        if change.1 {
            self.notify(t!("tui.toast.already_archived").to_string());
            return;
        }

        self.confirm = Some(Confirm {
            question: t!("tui.confirm.archive", change = change.0.clone()).to_string(),
            detail: t!("tui.confirm.archive_detail").to_string(),
            accept: t!("tui.confirm.accept_archive").to_string(),
            action: Confirmed::Run(Pending::Archive {
                skip_specs,
                target: change.0,
            }),
        });
        self.mode = Mode::Confirm;
    }

    fn request_editor(&mut self, path: Option<String>) {
        let target = match path {
            Some(relative) => self
                .artifact_root()
                .map(|root| root.join(&relative))
                .unwrap_or_else(|| PathBuf::from(relative)),
            None => match self.open_document() {
                Some(document) => document.path.clone(),
                None => {
                    self.notify(t!("tui.toast.no_document").to_string());
                    return;
                }
            },
        };

        if !target.is_file() {
            self.notify(t!("tui.toast.no_document").to_string());
            return;
        }

        self.pending = Some(Pending::Editor(target));
    }

    fn scaffold_change(&mut self, id: &str) {
        let slug = crate::domain::template::slugify(id);
        if slug.is_empty() {
            self.say(
                format!("  {}", t!("error.invalid_name", name = id)),
                Tone::Danger,
            );
            return;
        }

        let root = self.workspace.changes_dir().join(&slug);
        if root.exists() {
            self.say(
                format!("  {}", t!("tui.log.change_exists", change = slug.clone())),
                Tone::Warning,
            );
            return;
        }

        let files = crate::domain::template::change_scaffold(&slug);
        for (relative, contents) in files {
            let path = root.join(relative);
            if let Some(parent) = path.parent()
                && let Err(source) = std::fs::create_dir_all(parent)
            {
                self.report_failure(&crate::error::Error::io(parent, source));
                return;
            }
            if let Err(source) = std::fs::write(&path, contents) {
                self.report_failure(&crate::error::Error::io(&path, source));
                return;
            }
            self.say(
                format!("  → changes/{slug}/{}", relative.replace('\\', "/")),
                Tone::Accent,
            );
        }

        if let Err(error) = self.reload() {
            self.report_failure(&error);
            return;
        }

        self.say(
            format!("  {}", t!("tui.log.change_created", change = slug.clone())),
            Tone::Success,
        );
        self.focus_change(&slug);
    }

    // ---- Filtering ------------------------------------------------------

    fn apply_draft_filter(&mut self) {
        self.filter_text = self.draft.clone();
        self.filter = Filter::parse(&self.draft);
        self.clamp_list();
        self.sync_selection();
    }

    fn clear_filter(&mut self) {
        self.filter = Filter::All;
        self.filter_text.clear();
        self.draft.clear();
        self.clamp_list();
        self.sync_selection();
        self.notify(t!("tui.toast.filter_cleared").to_string());
    }

    // ---- Command line ---------------------------------------------------

    fn open_command(&mut self, seed: String) {
        self.draft = seed;
        self.history_cursor = None;
        self.mode = Mode::Command;
    }

    /// Applies `:theme`; without a name the palettes are cycled through.
    fn switch_palette(&mut self, name: Option<&str>) {
        let palette = match name {
            Some(requested) => match Palette::from_name(requested) {
                Some(palette) => palette,
                None => {
                    self.say(
                        format!(
                            "  {}",
                            t!(
                                "command.unknown_theme",
                                name = requested,
                                names = Palette::names()
                            )
                        ),
                        Tone::Danger,
                    );
                    return;
                }
            },
            None => self.palette.next(),
        };

        self.set_palette(palette);
        self.notify(t!("tui.toast.theme_changed", theme = palette.name()).to_string());
    }

    /// Executes a command line, as typed or as replayed from a keystroke.
    pub fn run_command(&mut self, line: &str) {
        let Some(parsed) = command::parse(line) else {
            return;
        };

        let trimmed = line.trim().to_string();
        if self.history.last() != Some(&trimmed) {
            self.history.push(trimmed);
        }

        match parsed {
            Command::Validate { strict, target } => self.request_validate(strict, target),
            Command::Archive { skip_specs, target } => self.request_archive(skip_specs, target),
            Command::New(id) => {
                self.say(
                    t!("tui.log.creating", change = id.clone()).to_string(),
                    Tone::Accent,
                );
                self.scaffold_change(&id);
            }
            Command::Filter(expression) => {
                self.draft = expression.clone();
                self.apply_draft_filter();
                self.draft.clear();
                if !expression.trim().is_empty() {
                    self.notify(self.filter.describe());
                }
            }
            Command::Write => self.save_buffer(),
            Command::Edit(path) => match path {
                Some(_) => self.request_editor(path),
                None => self.begin_edit(),
            },
            Command::External(path) => self.request_editor(path),
            Command::Diff => self.set_viewer(ViewerMode::Diff),
            Command::Tasks => self.set_viewer(ViewerMode::Tasks),
            Command::Dashboard => self.view = View::Dashboard,
            Command::Activity => self.open_activity(),
            Command::Editor => self.view = View::Editor,
            Command::Open(tab) => self.set_tab(tab),
            Command::Reload => match self.reload() {
                Ok(_) => self.notify(t!("tui.toast.reloaded").to_string()),
                Err(error) => self.report_failure(&error),
            },
            Command::Language(code) => {
                let locale = match code {
                    Some(requested) => i18n::activate(Some(&requested)),
                    None => i18n::cycle(),
                };
                self.notify(
                    t!(
                        "tui.toast.locale_changed",
                        language = i18n::display_name(&locale)
                    )
                    .to_string(),
                );
            }
            Command::Theme(name) => self.switch_palette(name.as_deref()),
            Command::Width(columns) => match columns {
                Some(columns) => {
                    self.set_sidebar_width(columns);
                    self.store_sidebar_width();
                    // The stored value, not the one asked for: below the
                    // minimum they differ, and the message has to say what
                    // actually happened.
                    self.notify(
                        t!(
                            "tui.toast.sidebar_width",
                            columns = self.sidebar_width.unwrap_or_default()
                        )
                        .to_string(),
                    );
                }
                None => self.reset_sidebar_width(),
            },
            Command::Clear => self.log.clear(),
            Command::Settings => self.open_settings(),
            Command::Help => self.mode = Mode::Help,
            Command::Quit => self.request_quit(),
            Command::Invalid(reason) => self.say(format!("  {reason}"), Tone::Danger),
            Command::Unknown(name) => self.say(
                format!("  {}", t!("command.unknown", name = name)),
                Tone::Danger,
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers
//
// These take `&Workspace` rather than `&self` so a caller can hold the borrow
// while mutating another field of the application.
// ---------------------------------------------------------------------------

/// The sentence reporting one event, naming its change and its task.
///
/// Prefixed with the clock time it carries: an activity panel that says what
/// happened but not when is a panel that cannot be read back through.
pub fn describe(event: &Event) -> String {
    format!(
        "{}  {}",
        crate::domain::util::format_clock(Some(event.time())),
        t!(
            event.kind.message_key(),
            change = event.change.clone(),
            detail = crate::domain::util::truncate(&event.label, 56)
        )
    )
}

/// The neighbour of `current` in `items`, wrapping at both ends.
fn step_through<T: Copy + PartialEq>(items: &[T], current: T, delta: isize) -> T {
    let index = items.iter().position(|item| *item == current).unwrap_or(0) as isize;
    items[(index + delta).rem_euclid(items.len() as isize) as usize]
}

/// Every document a tab can list, in presentation order.
///
/// The design document is not under `specs/`, but it is the one page a reader
/// wants first, so the project tab exists to hold it rather than leaving it
/// wedged at the top of the capability list.
pub fn document_pool(workspace: &Workspace, tab: Tab) -> Vec<&SpecDocument> {
    match tab {
        Tab::Specs => workspace.specs.iter().collect(),
        Tab::Project => workspace.project_documents(),
        Tab::Changes | Tab::Archive => Vec::new(),
    }
}

/// Indices, into the pool of `tab`, of the entries the filter admits.
fn visible(workspace: &Workspace, tab: Tab, filter: &Filter) -> Vec<usize> {
    if tab.holds_changes() {
        let archived = tab == Tab::Archive;
        return workspace
            .changes
            .iter()
            .enumerate()
            .filter(|(_, change)| change.archived == archived && filter.matches_change(change))
            .map(|(index, _)| index)
            .collect();
    }

    document_pool(workspace, tab)
        .into_iter()
        .enumerate()
        .filter(|(_, document)| filter.matches_spec(document))
        .map(|(index, _)| index)
        .collect()
}

fn selected_change<'a>(
    workspace: &'a Workspace,
    tab: Tab,
    filter: &Filter,
    position: Option<usize>,
) -> Option<&'a ChangeProposal> {
    if !tab.holds_changes() {
        return None;
    }
    let entries = visible(workspace, tab, filter);
    workspace.changes.get(*entries.get(position?)?)
}

fn selected_document<'a>(
    workspace: &'a Workspace,
    tab: Tab,
    filter: &Filter,
    position: Option<usize>,
) -> Option<&'a SpecDocument> {
    let entries = visible(workspace, tab, filter);
    document_pool(workspace, tab)
        .get(*entries.get(position?)?)
        .copied()
}

fn artifact_root(
    workspace: &Workspace,
    tab: Tab,
    filter: &Filter,
    position: Option<usize>,
) -> Option<PathBuf> {
    if tab.holds_changes() {
        return selected_change(workspace, tab, filter, position).map(|c| c.path.clone());
    }
    selected_document(workspace, tab, filter, position)
        .and_then(|document| document.path.parent())
        .map(Path::to_path_buf)
}

/// Documents shown in the artifact tree for the current selection.
///
/// For a change that is its whole directory; for a specification it is the
/// capability directory holding it, which in a nested layout is the capability
/// and in a flat one is the set of sibling documents.
fn artifacts<'a>(
    workspace: &'a Workspace,
    tab: Tab,
    filter: &Filter,
    position: Option<usize>,
) -> Vec<&'a SpecDocument> {
    if tab.holds_changes() {
        return selected_change(workspace, tab, filter, position)
            .map(|change| change.documents.iter().collect())
            .unwrap_or_default();
    }

    // The project tab is a set, not a directory: its two documents can live in
    // different places, and listing the siblings of either would drag in the
    // whole workspace root.
    if tab == Tab::Project {
        return document_pool(workspace, tab);
    }

    // Siblings, not descendants: a prefix match at the workspace root would
    // sweep in every specification in the tree.
    let Some(root) = artifact_root(workspace, tab, filter, position) else {
        return Vec::new();
    };
    document_pool(workspace, tab)
        .into_iter()
        .filter(|document| document.path.parent() == Some(root.as_path()))
        .collect()
}

fn display(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    //! The state machine driven the way the event loop drives it.
    //!
    //! Nothing here draws a frame. The renderer has its own tests in
    //! `tui::ui::tests`, and keeping the two apart is what lets a transition be
    //! pinned without also pinning a layout. The two things a handler reads back
    //! out of the last frame — the region rectangles and the measured size of
    //! the viewer — are set on the application directly, which is also what
    //! keeps these tests independent of how large the developer's terminal is.

    use std::sync::atomic::{AtomicUsize, Ordering};

    use crossterm::event::{KeyEventKind, KeyEventState};

    use super::*;
    use crate::domain::openspec::{Finding, ItemReport, Severity, ValidationReport};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// A workspace on disk plus every process-wide switch an [`App`] touches.
    ///
    /// `App::new` reads the user's preferences and opens their activity record,
    /// and several handlers move the process locale. All three are global, so
    /// the guard both serialises the tests and puts each of them back: a test
    /// that switched the interface to Spanish and failed would otherwise take
    /// every later assertion on English text down with it.
    struct Fixture {
        root: PathBuf,
        settings: PathBuf,
        locale: String,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl Fixture {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let (guard, settings) =
                crate::domain::settings::redirect_for_test(&format!("app-{unique}"));
            let locale = i18n::current();
            let root = std::env::temp_dir().join(format!(
                "openspectation-app-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);

            let inner = root.join("openspec");
            for directory in [
                "specs/auth",
                "specs/billing",
                "changes/add-oauth/specs/auth",
                "changes/docs-only",
                "changes/offline-sync",
                "changes/archive/2026-01-01-old-thing",
            ] {
                std::fs::create_dir_all(inner.join(directory)).unwrap();
            }

            let files = [
                (
                    "config.yaml",
                    "schema: spec-driven\ncontext: |\n  A demo workspace.\n",
                ),
                (
                    "specs/auth/spec.md",
                    "# auth Specification\n\n## Requirements\n\n\
                     ### Requirement: Tokens SHALL be signed\n",
                ),
                (
                    "specs/billing/spec.md",
                    "# billing Specification\n\n## Requirements\n\n\
                     ### Requirement: Invoices MUST balance\n",
                ),
                (
                    "changes/add-oauth/proposal.md",
                    "# Add OAuth\n\n## Why\n\nBecause tokens expire.\n\n\
                     ## What changes\n\nThe refresh flow moves into the client.\n\n\
                     ## Non-goals\n\nNothing about billing.\n",
                ),
                (
                    "changes/add-oauth/tasks.md",
                    "## 1. Groundwork\n\n- [x] 1.1 Pick a library\n\
                     - [ ] 1.2 Wire the refresh flow\n",
                ),
                (
                    "changes/add-oauth/specs/auth/spec.md",
                    "## ADDED Requirements\n\n### Requirement: Tokens SHALL refresh silently\n\n\
                     #### Scenario: Expired token\n\n- **WHEN** the token expired\n\
                     - **THEN** it refreshes\n\n## REMOVED Requirements\n\n\
                     ### Requirement: Sessions SHALL live forever\n",
                ),
                (
                    "changes/docs-only/proposal.md",
                    "# Docs only\n\nNothing to tick and nothing to diff.\n",
                ),
                (
                    "changes/offline-sync/proposal.md",
                    "# Offline sync\n\nQueued writes survive a disconnect.\n",
                ),
                (
                    "changes/offline-sync/tasks.md",
                    "## 1. Queue\n\n- [ ] 1.1 Persist locally\n- [ ] 1.2 Drain on reconnect\n",
                ),
                (
                    "changes/archive/2026-01-01-old-thing/proposal.md",
                    "# Old thing\n\nShipped already.\n",
                ),
            ];
            for (relative, body) in files {
                std::fs::write(inner.join(relative), body).unwrap();
            }

            Self {
                root,
                settings,
                locale,
                _guard: guard,
            }
        }

        fn app(&self) -> App {
            App::new(crate::domain::workspace::Workspace::open(&self.root).unwrap())
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.root.join("openspec").join(relative)
        }

        /// The same document, spelled the way the workspace spells it.
        ///
        /// `Workspace::resolve_root` canonicalizes, and on two of the three
        /// platforms the canonical form of a temporary directory is not the
        /// one `std::env::temp_dir` hands out: macOS resolves `/var` through
        /// its symlink to `/private/var`, and Windows returns a verbatim
        /// `\\?\` path with the long form of a name the environment may give
        /// in 8.3 form. Comparing an app path against [`Fixture::path`]
        /// therefore only holds on Linux.
        fn resolved(&self, relative: &str) -> PathBuf {
            let path = self.path(relative);
            std::fs::canonicalize(&path).unwrap_or(path)
        }

        fn read(&self, relative: &str) -> String {
            std::fs::read_to_string(self.path(relative)).unwrap()
        }

        fn write(&self, relative: &str, body: &str) {
            std::fs::write(self.path(relative), body).unwrap();
        }

        fn stored(&self) -> String {
            std::fs::read_to_string(&self.settings).unwrap_or_default()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            rust_i18n::set_locale(&self.locale);
            // A test that pointed the preferences at an unwritable path must not
            // leave the variable there for whoever runs next.
            //
            // Safe: the guard is still held, so no other thread is looking.
            unsafe {
                std::env::set_var(crate::domain::settings::PATH_ENV, &self.settings);
            }
            let _ = std::fs::remove_dir_all(&self.root);
            let _ = std::fs::remove_file(&self.settings);
            let _ = std::fs::remove_file(crate::domain::settings::test_history_path());
        }
    }

    fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        app.on_key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
        .unwrap();
    }

    fn press(app: &mut App, code: KeyCode) {
        key(app, code, KeyModifiers::NONE);
    }

    fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            press(app, KeyCode::Char(character));
        }
    }

    fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
        app.on_mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }

    fn click(app: &mut App, column: u16, row: u16) {
        mouse(app, MouseEventKind::Down(MouseButton::Left), column, row);
    }

    fn wheel(app: &mut App, column: u16, row: u16, down: bool) {
        let kind = if down {
            MouseEventKind::ScrollDown
        } else {
            MouseEventKind::ScrollUp
        };
        mouse(app, kind, column, row);
    }

    /// The rectangles a drawn frame would have left behind, at a fixed size so
    /// no assertion here depends on the terminal running the suite.
    fn lay_out(app: &mut App) {
        app.regions = Regions {
            list: Rect::new(0, 0, 20, 10),
            tabs: [
                Rect::new(0, 10, 5, 1),
                Rect::new(5, 10, 5, 1),
                Rect::new(10, 10, 5, 1),
                Rect::new(15, 10, 5, 1),
            ],
            artifacts: Rect::new(0, 11, 20, 9),
            sidebar: Rect::new(0, 0, 20, 20),
            divider: Rect::new(19, 0, 1, 20),
            viewer: Rect::new(20, 0, 60, 20),
            log: Rect::new(20, 20, 60, 3),
            command: Rect::new(0, 23, 80, 1),
            settings: Rect::new(78, 0, 2, 1),
            setting_rows: [Rect::new(30, 5, 20, 1), Rect::new(30, 6, 20, 1)],
        };
        app.viewer_height = 10;
        app.viewer_width = 60;
    }

    fn log_text(app: &App) -> String {
        app.log
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn toast(app: &App) -> String {
        app.toast
            .as_ref()
            .map(|toast| toast.message.clone())
            .unwrap_or_default()
    }

    fn viewer_text(app: &App) -> String {
        app.cache
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The rendered lines including their styles, which is what a stale cache
    /// would leave untouched after a repaint.
    fn viewer_debug(app: &App) -> String {
        format!("{:?}", app.cache.lines)
    }

    fn change_ids(app: &App) -> Vec<String> {
        let pool = &app.workspace.changes;
        app.visible()
            .into_iter()
            .map(|index| pool[index].id.clone())
            .collect()
    }

    fn archived_id(app: &App) -> String {
        app.workspace
            .changes
            .iter()
            .find(|change| change.archived)
            .expect("an archived change")
            .id
            .clone()
    }

    // ---- Selection ------------------------------------------------------

    #[test]
    fn the_fixture_lists_what_every_tab_is_expected_to_hold() {
        let fixture = Fixture::new();
        let app = fixture.app();

        assert_eq!(change_ids(&app), ["add-oauth", "docs-only", "offline-sync"]);
        assert_eq!(app.tab_len(Tab::Archive), 1);
        assert_eq!(app.tab_len(Tab::Specs), 2);
        assert_eq!(app.tab_len(Tab::Project), 1);
        assert_eq!(app.list_state().selected(), Some(0));
    }

    #[test]
    fn the_palette_flag_installs_its_theme_alongside_its_name() {
        let fixture = Fixture::new();
        let app = fixture.app().with_palette(Palette::Nord);

        assert_eq!(app.palette, Palette::Nord);
        assert_eq!(app.theme.primary, Palette::Nord.theme().primary);
    }

    #[test]
    fn focusing_a_named_change_opens_the_tab_that_holds_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        assert!(!app.focus_change("no-such-change"));
        assert_eq!(app.tab, Tab::Changes);

        assert!(app.focus_change("offline-sync"));
        assert_eq!(app.tab, Tab::Changes);
        assert_eq!(app.selected_change().unwrap().id, "offline-sync");

        let archived = archived_id(&app);
        assert!(app.focus_change(&archived));
        assert_eq!(app.tab, Tab::Archive);
        assert!(app.selected_change().unwrap().archived);
    }

    /// Naming a change is asking to see it, so a filter that would hide it is
    /// dropped rather than left to swallow the request.
    #[test]
    fn focusing_a_change_drops_a_filter_that_would_hide_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":filter offline");
        assert_eq!(change_ids(&app), ["offline-sync"]);

        assert!(app.focus_change("add-oauth"));
        assert!(!app.filter.is_active());
        assert!(app.filter_text.is_empty());
        assert_eq!(app.selected_change().unwrap().id, "add-oauth");
    }

    #[test]
    fn stepping_the_tabs_wraps_at_both_ends() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        for expected in [Tab::Archive, Tab::Specs, Tab::Project, Tab::Changes] {
            press(&mut app, KeyCode::Char('s'));
            assert_eq!(app.tab, expected);
        }
        for expected in [Tab::Project, Tab::Specs, Tab::Archive, Tab::Changes] {
            press(&mut app, KeyCode::Char('S'));
            assert_eq!(app.tab, expected);
        }

        for (digit, expected) in [
            ('2', Tab::Archive),
            ('3', Tab::Specs),
            ('4', Tab::Project),
            ('1', Tab::Changes),
        ] {
            press(&mut app, KeyCode::Char(digit));
            assert_eq!(app.tab, expected);
        }

        press(&mut app, KeyCode::Char('A'));
        assert_eq!(app.tab, Tab::Archive);
        press(&mut app, KeyCode::Char('A'));
        assert_eq!(app.tab, Tab::Changes);
    }

    #[test]
    fn each_tab_keeps_its_own_selection() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.selected_change().unwrap().id, "docs-only");

        press(&mut app, KeyCode::Char('3'));
        assert_eq!(app.list_state().selected(), Some(0));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.list_state().selected(), Some(1));

        press(&mut app, KeyCode::Char('1'));
        assert_eq!(app.selected_change().unwrap().id, "docs-only");
        press(&mut app, KeyCode::Char('3'));
        assert_eq!(app.list_state().selected(), Some(1));
    }

    /// Selecting the tab already open is not a switch: it must not throw away
    /// the viewer mode the user is working in.
    #[test]
    fn reselecting_the_open_tab_changes_nothing() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('t'));
        assert_eq!(app.viewer, ViewerMode::Tasks);

        press(&mut app, KeyCode::Char('1'));
        assert_eq!(app.viewer, ViewerMode::Tasks, "the tab was re-entered");
        assert_eq!(app.pane, Pane::Viewer);

        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.viewer, ViewerMode::Document);
        assert_eq!(app.pane, Pane::List);
    }

    // ---- Viewer modes ---------------------------------------------------

    #[test]
    fn a_viewer_mode_with_nothing_behind_it_is_refused() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        assert!(app.focus_change("docs-only"));
        assert!(app.deltas.is_empty());
        assert!(app.tasks.is_empty());

        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.viewer, ViewerMode::Document);
        assert_eq!(toast(&app), t!("tui.toast.no_deltas").to_string());

        press(&mut app, KeyCode::Char('t'));
        assert_eq!(app.viewer, ViewerMode::Document);
        assert_eq!(toast(&app), t!("tui.toast.no_tasks").to_string());
    }

    #[test]
    fn pressing_a_viewer_key_twice_returns_to_the_document() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('t'));
        assert_eq!(app.viewer, ViewerMode::Tasks);
        press(&mut app, KeyCode::Char('t'));
        assert_eq!(app.viewer, ViewerMode::Document);

        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.viewer, ViewerMode::Diff);
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.viewer, ViewerMode::Document);

        // The document is where a repeated key lands, so it cannot toggle away
        // from itself.
        press(&mut app, KeyCode::Char('m'));
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.viewer, ViewerMode::Document);
    }

    #[test]
    fn moving_to_a_change_without_a_delta_falls_back_to_the_document() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.viewer, ViewerMode::Diff);

        // `d` moved the focus to the viewer; walk back to the list before
        // stepping it.
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.pane, Pane::List);

        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.selected_change().unwrap().id, "docs-only");
        assert_eq!(app.viewer, ViewerMode::Document);
    }

    #[test]
    fn moving_to_a_change_without_tasks_falls_back_to_the_document() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Char('j'));

        assert_eq!(app.selected_change().unwrap().id, "docs-only");
        assert_eq!(app.viewer, ViewerMode::Document);
    }

    // ---- Escape ---------------------------------------------------------

    #[test]
    fn escape_unwinds_the_interface_one_layer_at_a_time() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.view, View::Dashboard);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.view, View::Editor);

        press(&mut app, KeyCode::Char('t'));
        assert_eq!(app.viewer, ViewerMode::Tasks);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.viewer, ViewerMode::Document);

        app.run_command(":filter offline");
        assert!(app.filter.is_active());
        press(&mut app, KeyCode::Esc);
        assert!(!app.filter.is_active());

        app.run_command(":no-such-command");
        assert!(!app.log.is_empty());
        press(&mut app, KeyCode::Esc);
        assert!(app.log.is_empty());

        assert!(!app.should_quit);
        press(&mut app, KeyCode::Esc);
        assert!(app.should_quit, "the last layer is the session itself");
    }

    #[test]
    fn any_key_dismisses_the_help_overlay() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.mode, Mode::Help);
        press(&mut app, KeyCode::Char('q'));
        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.should_quit, "the overlay ate the key");

        press(&mut app, KeyCode::F(1));
        assert_eq!(app.mode, Mode::Help);
        press(&mut app, KeyCode::Char('z'));
        assert_eq!(app.mode, Mode::Normal);

        press(&mut app, KeyCode::Char('?'));
        key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(app.mode, Mode::Normal);
    }

    // ---- Settings -------------------------------------------------------

    #[test]
    fn the_preferences_panel_walks_its_rows_and_wraps() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char(','));
        assert_eq!(app.mode, Mode::Settings);
        assert_eq!(app.setting_cursor, 0);

        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.setting_cursor, 1);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.setting_cursor, 0, "the last row wraps to the first");
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.setting_cursor, 1);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.setting_cursor, 0);
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.setting_cursor, 1);

        // A form, not an overlay: a key that means nothing here leaves it open.
        press(&mut app, KeyCode::Char('z'));
        assert_eq!(app.mode, Mode::Settings);
        press(&mut app, KeyCode::F(2));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn the_theme_preference_cycles_in_both_directions() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.open_settings();
        let first = app.palette;
        press(&mut app, KeyCode::Char('l'));
        let second = app.palette;
        assert_ne!(second, first);
        assert_eq!(app.setting_value(Setting::Theme), second.name());

        press(&mut app, KeyCode::Char('h'));
        assert_eq!(app.palette, first, "the value walks back the way it came");
        assert_eq!(app.settings.theme.as_deref(), Some(first.name()));
    }

    #[test]
    fn cycling_the_language_preference_switches_the_catalogue_and_stores_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.open_settings();
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(Setting::ORDER[app.setting_cursor], Setting::Language);

        let before = i18n::current();
        press(&mut app, KeyCode::Enter);
        let after = i18n::current();

        assert_ne!(after, before);
        assert_eq!(app.settings.language.as_deref(), Some(after.as_str()));
        assert_eq!(
            app.setting_value(Setting::Language),
            i18n::display_name(&after)
        );
        assert!(fixture.stored().contains("language:"));

        press(&mut app, KeyCode::Left);
        assert_eq!(i18n::current(), before);
    }

    #[test]
    fn a_click_off_the_preference_rows_closes_the_panel() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        let gear = app.regions.settings;
        click(&mut app, gear.x, gear.y);
        assert_eq!(app.mode, Mode::Settings);

        let row = app.regions.setting_rows[1];
        let before = i18n::current();
        click(&mut app, row.x + 1, row.y);
        assert_eq!(
            app.setting_cursor, 1,
            "the click chose the row it landed on"
        );
        assert_ne!(i18n::current(), before);

        // The wheel is not a click, so it neither moves a value nor closes.
        wheel(&mut app, 0, 0, true);
        assert_eq!(app.mode, Mode::Settings);

        click(&mut app, 1, 19);
        assert_eq!(app.mode, Mode::Normal);
    }

    // ---- Sidebar width --------------------------------------------------

    #[test]
    fn storing_an_unchanged_sidebar_width_writes_nothing() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":width 60");
        assert_eq!(app.sidebar_width, Some(60));
        assert!(fixture.stored().contains("sidebar_width: 60"));

        std::fs::remove_file(&fixture.settings).unwrap();
        app.run_command(":width 60");
        assert!(
            !fixture.settings.exists(),
            "the same width was written a second time"
        );
    }

    #[test]
    fn a_width_below_the_floor_is_raised_and_reported_as_stored() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":width 4");
        assert_eq!(app.sidebar_width, Some(SIDEBAR_MIN));
        assert_eq!(
            toast(&app),
            t!("tui.toast.sidebar_width", columns = SIDEBAR_MIN).to_string(),
            "the message named the request rather than the outcome"
        );
    }

    #[test]
    fn handing_the_sidebar_back_to_the_layout_forgets_the_stored_width() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":width 60");
        assert!(fixture.stored().contains("sidebar_width"));

        app.run_command(":width auto");
        assert_eq!(app.sidebar_width, None);
        assert_eq!(toast(&app), t!("tui.toast.sidebar_auto").to_string());
        assert!(!fixture.stored().contains("sidebar_width"));
    }

    // ---- Search ---------------------------------------------------------

    #[test]
    fn typing_in_the_search_box_narrows_the_list_as_it_goes() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.mode, Mode::Search);

        type_text(&mut app, "offline");
        assert_eq!(change_ids(&app), ["offline-sync"]);
        assert_eq!(app.filter_text, "offline");

        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.draft, "offli");
        assert_eq!(change_ids(&app), ["offline-sync"]);

        press(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.filter.is_active(), "the filter outlives the box");

        // Re-opening the box seeds it with the expression already in force.
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.draft, "offli");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.filter.is_active());
        assert_eq!(change_ids(&app), ["add-oauth", "docs-only", "offline-sync"]);
    }

    #[test]
    fn escaping_an_empty_search_leaves_the_list_untouched() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Esc);

        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.filter.is_active());
        assert!(
            app.toast.is_none(),
            "nothing was cleared, so nothing is said"
        );
    }

    /// A filter narrow enough to match nothing leaves no selection, and the
    /// panes that hang off it have to empty with it.
    #[test]
    fn a_filter_matching_nothing_empties_everything_derived_from_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":filter zzzz");
        assert!(app.visible().is_empty());
        assert!(app.selected_change().is_none());
        assert!(app.artifacts().is_empty());
        assert!(app.open_document().is_none());
        assert!(app.open_path().is_none());
        assert!(app.tasks.is_empty());
        assert!(app.deltas.is_empty());

        app.sync_viewer(60);
        assert!(app.viewer_is_empty());

        app.run_command(":filter");
        assert_eq!(change_ids(&app), ["add-oauth", "docs-only", "offline-sync"]);
    }

    #[test]
    fn clearing_the_filter_restores_every_entry_and_says_so() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":filter docs");
        assert_eq!(change_ids(&app), ["docs-only"]);

        app.clear_filter();
        assert_eq!(change_ids(&app), ["add-oauth", "docs-only", "offline-sync"]);
        assert!(app.filter_text.is_empty());
        assert!(app.draft.is_empty());
        assert_eq!(toast(&app), t!("tui.toast.filter_cleared").to_string());
    }

    // ---- Command line ---------------------------------------------------

    #[test]
    fn the_command_line_recalls_what_was_typed_before() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char(':'));
        press(&mut app, KeyCode::Up);
        assert!(app.draft.is_empty(), "there is nothing to recall yet");
        press(&mut app, KeyCode::Esc);

        app.run_command("clear");
        app.run_command("filter docs");

        press(&mut app, KeyCode::Char(':'));
        press(&mut app, KeyCode::Down);
        assert!(app.draft.is_empty(), "forward from nowhere is nowhere");

        press(&mut app, KeyCode::Up);
        assert_eq!(app.draft, "filter docs");
        press(&mut app, KeyCode::Up);
        assert_eq!(app.draft, "clear");
        press(&mut app, KeyCode::Up);
        assert_eq!(app.draft, "clear", "the oldest line is the end of the road");
        press(&mut app, KeyCode::Down);
        assert_eq!(app.draft, "filter docs");

        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.draft, "filter doc");

        press(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.draft.is_empty());
        assert!(app.history_cursor.is_none());
    }

    #[test]
    fn the_same_command_twice_running_is_recorded_once() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":clear");
        app.run_command(":clear");
        app.run_command(":reload");
        app.run_command(":clear");

        assert_eq!(app.history, [":clear", ":reload", ":clear"]);
    }

    #[test]
    fn typing_a_command_and_pressing_enter_runs_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char(':'));
        type_text(&mut app, "tasks");
        assert_eq!(app.mode, Mode::Command);
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.viewer, ViewerMode::Tasks);
        assert!(app.draft.is_empty());
    }

    // ---- Deferred work --------------------------------------------------
    //
    // Everything that shells out is queued for the loop to run between two
    // frames. A handler that ran it inline would block with nothing on screen
    // saying why, so these assert on the queue rather than on the outcome.

    /// Pretends the upstream CLI is installed, which is what the actions that
    /// need it probe once at start-up.
    fn with_cli(app: &mut App) {
        app.cli_version = Some("openspec/1.0.0".to_string());
    }

    #[test]
    fn validating_queues_the_run_instead_of_shelling_out() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        with_cli(&mut app);

        press(&mut app, KeyCode::Char('v'));

        assert_eq!(
            app.pending,
            Some(Pending::Validate {
                strict: true,
                target: Some("add-oauth".to_string())
            })
        );
        assert!(log_text(&app).contains("add-oauth"));

        // The loop takes the action, and it is gone once taken.
        assert!(app.take_pending().is_some());
        assert_eq!(app.take_pending(), None);
    }

    #[test]
    fn validating_from_a_tab_without_changes_targets_the_whole_workspace() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        with_cli(&mut app);

        press(&mut app, KeyCode::Char('3'));
        app.run_command(":validate --loose");

        assert_eq!(
            app.pending,
            Some(Pending::Validate {
                strict: false,
                target: None
            })
        );
        assert!(log_text(&app).contains(&t!("tui.log.everything").to_string()));
    }

    #[test]
    fn a_missing_upstream_cli_is_reported_before_anything_is_queued() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        app.cli_version = None;

        press(&mut app, KeyCode::Char('v'));
        assert!(app.pending.is_none());
        assert!(log_text(&app).contains(&crate::error::Error::CliMissing.message()));

        app.log.clear();
        press(&mut app, KeyCode::Char('a'));
        assert!(app.pending.is_none());
        assert_eq!(
            app.mode,
            Mode::Normal,
            "nothing was asked, so nothing waits"
        );
        assert!(app.confirm.is_none());
        assert!(log_text(&app).contains(&crate::error::Error::CliMissing.message()));
    }

    #[test]
    fn archiving_asks_first_and_only_then_queues_the_run() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        with_cli(&mut app);

        press(&mut app, KeyCode::Char('a'));
        assert_eq!(app.mode, Mode::Confirm);
        assert!(app.pending.is_none(), "the question is not the answer");
        assert!(app.confirm.as_ref().unwrap().question.contains("add-oauth"));

        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(
            app.pending,
            Some(Pending::Archive {
                skip_specs: false,
                target: "add-oauth".to_string()
            })
        );
        assert!(
            !app.workspace.find_change("add-oauth").unwrap().archived,
            "the handler moved the change itself"
        );
    }

    #[test]
    fn declining_an_archive_queues_nothing_and_says_so() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        with_cli(&mut app);

        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('n'));

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.pending.is_none());
        assert!(app.confirm.is_none());
        assert!(log_text(&app).contains(&t!("tui.log.cancelled").to_string()));
    }

    #[test]
    fn enter_accepts_a_confirmation_and_a_stale_one_just_closes() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        with_cli(&mut app);

        app.run_command(":archive --skip-specs offline-sync");
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.pending,
            Some(Pending::Archive {
                skip_specs: true,
                target: "offline-sync".to_string()
            })
        );

        // A confirmation mode with no question behind it answers nothing.
        app.mode = Mode::Confirm;
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn archiving_an_already_archived_change_is_refused() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        with_cli(&mut app);

        press(&mut app, KeyCode::Char('A'));
        assert_eq!(app.tab, Tab::Archive);

        press(&mut app, KeyCode::Char('a'));
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.pending.is_none());
        assert_eq!(toast(&app), t!("tui.toast.already_archived").to_string());
    }

    #[test]
    fn archiving_with_no_change_in_reach_says_so() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        with_cli(&mut app);

        press(&mut app, KeyCode::Char('3'));
        press(&mut app, KeyCode::Char('a'));
        assert_eq!(toast(&app), t!("tui.toast.no_change").to_string());
        assert!(app.pending.is_none());

        // A named change is reachable from any tab.
        app.run_command(":archive offline-sync");
        assert_eq!(app.mode, Mode::Confirm);
        assert!(
            app.confirm
                .as_ref()
                .unwrap()
                .question
                .contains("offline-sync")
        );
    }

    #[test]
    fn the_external_editor_is_queued_with_the_document_under_the_cursor() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('E'));
        assert_eq!(
            app.pending,
            Some(Pending::Editor(
                fixture.resolved("changes/add-oauth/proposal.md")
            ))
        );

        app.run_command(":external tasks.md");
        assert_eq!(
            app.pending,
            Some(Pending::Editor(
                fixture.resolved("changes/add-oauth/tasks.md")
            ))
        );
    }

    #[test]
    fn asking_the_external_editor_for_a_file_that_is_not_there_queues_nothing() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":external nowhere.md");
        assert!(app.pending.is_none());
        assert_eq!(toast(&app), t!("tui.toast.no_document").to_string());

        app.run_command(":filter zzzz");
        press(&mut app, KeyCode::Char('E'));
        assert!(app.pending.is_none());
        assert_eq!(toast(&app), t!("tui.toast.no_document").to_string());
    }

    #[test]
    fn returning_from_the_external_editor_reloads_and_names_the_file() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        fixture.write(
            "changes/add-oauth/proposal.md",
            "# Add OAuth\n\nEdited elsewhere.\n",
        );
        app.after_editor(&fixture.path("changes/add-oauth/proposal.md"));

        assert!(log_text(&app).contains("proposal.md"));
        assert!(
            app.open_document()
                .unwrap()
                .body
                .contains("Edited elsewhere")
        );
    }

    /// The timeline is opened by a key, and the scan behind it walks a commit
    /// graph; running that inline would drop frames with nothing to show for it.
    #[test]
    fn opening_the_timeline_queues_the_scan_once() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('H'));
        assert_eq!(app.view, View::Activity);
        assert_eq!(app.take_pending(), Some(Pending::ScanHistory));
        assert!(log_text(&app).contains(&t!("tui.log.scanning").to_string()));

        press(&mut app, KeyCode::Char('H'));
        assert_eq!(app.view, View::Editor);
        app.run_command(":activity");
        assert!(app.take_pending().is_none(), "the scan ran a second time");
    }

    #[test]
    fn no_scan_is_started_from_under_an_open_buffer() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('e'));
        assert_eq!(app.mode, Mode::Edit);

        app.open_activity();
        assert_eq!(app.view, View::Activity);
        assert!(app.take_pending().is_none());
    }

    #[test]
    fn a_completed_scan_reports_what_version_control_gave_up() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.report_history(Recovered::default());
        assert!(log_text(&app).contains(&t!("tui.log.no_repository").to_string()));

        app.log.clear();
        app.report_history(Recovered {
            events: vec![Event::new(1_770_076_800, EventKind::Done, "add-oauth")],
            available: true,
            truncated: true,
            filtered: false,
            duration_ms: 5_000,
            ..Recovered::default()
        });

        let text = log_text(&app);
        assert!(text.contains(&t!("tui.log.recovered", count = 1).to_string()));
        assert!(text.contains(&t!("tui.activity.incomplete").to_string()));
        assert!(text.contains(&t!("tui.log.suggest_filters").to_string()));
        assert!(app.timeline().scanned, "the timeline was not rebuilt");
    }

    #[test]
    fn a_scan_of_a_workspace_outside_version_control_still_reports() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        let recovered = app.scan_history();
        app.report_history(recovered);
        assert!(!app.log.is_empty(), "the scan said nothing at all");
    }

    #[test]
    fn a_validation_report_lands_in_the_activity_log() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.report_validation(&ValidationReport {
            items: vec![
                ItemReport {
                    id: "add-oauth".to_string(),
                    kind: "change".to_string(),
                    valid: true,
                    findings: Vec::new(),
                    duration_ms: 12,
                },
                ItemReport {
                    id: "billing".to_string(),
                    kind: "spec".to_string(),
                    valid: false,
                    findings: vec![
                        Finding {
                            severity: Severity::Error,
                            location: "spec.md:3".to_string(),
                            message: "missing scenario".to_string(),
                        },
                        Finding {
                            severity: Severity::Warning,
                            location: "spec.md:9".to_string(),
                            message: "vague wording".to_string(),
                        },
                        Finding {
                            severity: Severity::Info,
                            location: "spec.md:11".to_string(),
                            message: "consider a table".to_string(),
                        },
                    ],
                    duration_ms: 30,
                },
            ],
            passed: 1,
            failed: 1,
        });

        let text = log_text(&app);
        assert!(text.contains("✔"));
        assert!(text.contains("✖ billing"));
        assert!(text.contains("missing scenario"));
        assert!(text.contains("vague wording"));
        assert!(text.contains("consider a table"));
        assert!(
            app.log.iter().any(|line| line.tone == Tone::Danger),
            "a failed run was logged as if it had passed"
        );
    }

    #[test]
    fn an_archive_report_reloads_the_workspace_and_says_so() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        std::fs::create_dir_all(fixture.path("changes/archive/2026-02-02-docs-only")).unwrap();
        std::fs::rename(
            fixture.path("changes/docs-only/proposal.md"),
            fixture.path("changes/archive/2026-02-02-docs-only/proposal.md"),
        )
        .unwrap();
        std::fs::remove_dir(fixture.path("changes/docs-only")).unwrap();

        app.report_archive(&["moved docs-only".to_string()]);

        let text = log_text(&app);
        assert!(text.contains("moved docs-only"));
        assert!(text.contains(&t!("tui.log.archived").to_string()));
        assert_eq!(change_ids(&app), ["add-oauth", "offline-sync"]);
    }

    #[test]
    fn a_failure_reports_its_message_and_the_advice_that_goes_with_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        let error = crate::error::Error::CliMissing;
        app.report_failure(&error);

        let text = log_text(&app);
        assert!(text.contains(&error.message()));
        assert!(text.contains(&error.hint().unwrap()));
    }

    // ---- Tasks ----------------------------------------------------------

    /// The addressed line is re-read before it is rewritten. An agent that
    /// edited `tasks.md` between two frames must not lose the edit to a
    /// keystroke aimed at where the checkbox used to be.
    #[test]
    fn toggling_a_task_that_moved_reports_it_and_writes_nothing() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('t'));
        assert_eq!(app.viewer, ViewerMode::Tasks);
        let task = app.tasks[0].clone();
        assert_eq!(task.line, 3);

        fixture.write(
            "changes/add-oauth/tasks.md",
            "## 1. Groundwork\n\nprose now, not a checkbox\n- [ ] 1.2 Wire the refresh flow\n",
        );

        press(&mut app, KeyCode::Char(' '));

        let error = crate::error::Error::TaskMoved(task.path.clone(), task.line);
        assert!(log_text(&app).contains(&error.message()));
        assert!(
            fixture
                .read("changes/add-oauth/tasks.md")
                .contains("prose now, not a checkbox"),
            "the keystroke overwrote work it never saw"
        );
    }

    #[test]
    fn space_opens_the_task_view_before_it_ticks_anything() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        assert_eq!(app.viewer, ViewerMode::Document);
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.viewer, ViewerMode::Tasks);
        assert!(
            fixture
                .read("changes/add-oauth/tasks.md")
                .contains("- [x] 1.1"),
            "the first press both opened the view and toggled"
        );

        press(&mut app, KeyCode::Char(' '));
        assert!(
            fixture
                .read("changes/add-oauth/tasks.md")
                .contains("- [ ] 1.1")
        );
        assert_eq!(
            toast(&app),
            t!("tui.toast.task_undone", task = "1.1 Pick a library").to_string()
        );
    }

    #[test]
    fn space_on_a_change_with_no_checkboxes_says_so() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        assert!(app.focus_change("docs-only"));
        press(&mut app, KeyCode::Char(' '));

        assert_eq!(app.viewer, ViewerMode::Document);
        assert_eq!(toast(&app), t!("tui.toast.no_tasks").to_string());
    }

    #[test]
    fn ticking_the_last_task_leaves_the_cursor_inside_the_list() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::End);
        assert_eq!(app.task_cursor, app.tasks.len() - 1);

        press(&mut app, KeyCode::Char(' '));
        assert!(app.task_cursor < app.tasks.len());
        assert!(
            fixture
                .read("changes/add-oauth/tasks.md")
                .contains("- [x] 1.2")
        );
    }

    // ---- Editing --------------------------------------------------------

    #[test]
    fn editing_a_document_with_no_file_behind_it_is_refused() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        std::fs::remove_file(fixture.path("changes/add-oauth/proposal.md")).unwrap();
        press(&mut app, KeyCode::Char('e'));

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.buffer.is_none());
        assert_eq!(toast(&app), t!("tui.toast.not_a_file").to_string());
    }

    #[test]
    fn a_file_too_large_to_edit_is_reported_rather_than_opened() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        let oversized =
            "x".repeat(usize::try_from(crate::domain::editor::MAX_EDITABLE_BYTES).unwrap() + 1);
        fixture.write("changes/add-oauth/proposal.md", &oversized);

        press(&mut app, KeyCode::Char('e'));

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.buffer.is_none());
        assert!(
            log_text(&app).contains(
                &crate::error::Error::TooLargeToEdit(
                    fixture.resolved("changes/add-oauth/proposal.md")
                )
                .message()
            )
        );
    }

    #[test]
    fn beginning_an_edit_with_nothing_open_says_so() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":filter zzzz");
        press(&mut app, KeyCode::Char('e'));

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(toast(&app), t!("tui.toast.no_document").to_string());
    }

    #[test]
    fn unsaved_work_survives_a_trip_to_the_preview() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        press(&mut app, KeyCode::Char('e'));
        assert_eq!(app.mode, Mode::Edit);
        assert_eq!(app.viewer, ViewerMode::Edit);
        assert_eq!(app.pane, Pane::Viewer);

        type_text(&mut app, "zz");
        assert!(app.buffer.as_ref().unwrap().dirty);

        press(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.viewer, ViewerMode::Document);
        assert_eq!(toast(&app), t!("tui.toast.unsaved").to_string());
        assert!(app.buffer.as_ref().unwrap().dirty, "the work was dropped");

        // The same file is resumed rather than re-read, which is the only way
        // the two characters can still be there.
        press(&mut app, KeyCode::Char('e'));
        assert!(app.buffer.as_ref().unwrap().text().starts_with("zz#"));

        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(!app.buffer.as_ref().unwrap().dirty);
        assert!(
            fixture
                .read("changes/add-oauth/proposal.md")
                .starts_with("zz#")
        );

        press(&mut app, KeyCode::Esc);
        assert!(app.buffer.is_none(), "a clean buffer is not worth keeping");
    }

    #[test]
    fn ctrl_e_and_ctrl_c_leave_the_editor_without_quitting() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('e'));
        key(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
        assert_eq!(app.mode, Mode::Normal);

        press(&mut app, KeyCode::Char('e'));
        key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.should_quit, "Ctrl+C ended the session, not the edit");
    }

    /// Reached only from the preview: the viewer is left showing the editor
    /// while the keyboard has gone back to normal.
    #[test]
    fn asking_to_edit_while_the_editor_is_already_shown_returns_to_the_preview() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('e'));
        app.mode = Mode::Normal;
        app.run_command(":edit");

        assert_eq!(app.viewer, ViewerMode::Document);
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn the_editing_keys_reach_the_buffer_and_nothing_else() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);
        app.viewer_height = 4;

        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::End);
        assert_eq!(app.buffer.as_ref().unwrap().column, "# Add OAuth".len());

        press(&mut app, KeyCode::Home);
        assert_eq!(app.buffer.as_ref().unwrap().column, 0);

        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Tab);
        assert!(app.buffer.as_ref().unwrap().text().starts_with("  \n"));

        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Delete);
        assert!(
            app.buffer
                .as_ref()
                .unwrap()
                .text()
                .starts_with("# Add OAuth"),
            "the inserted line was not undone"
        );

        press(&mut app, KeyCode::Down);
        assert_eq!(app.buffer.as_ref().unwrap().row, 1);

        // Line 1 is blank, so a step to the right is a step onto the next one.
        press(&mut app, KeyCode::Right);
        assert_eq!(app.buffer.as_ref().unwrap().row, 2);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.buffer.as_ref().unwrap().row, 1);

        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.buffer.as_ref().unwrap().row, 5);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.buffer.as_ref().unwrap().row, 1);

        press(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn the_editor_scrolls_sideways_to_keep_the_cursor_in_view() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);
        app.viewer_width = 10;

        press(&mut app, KeyCode::Char('e'));
        assert_eq!(app.edit_offset, 0);

        type_text(&mut app, "a long run of characters");
        assert!(app.edit_offset > 0, "the cursor walked off the pane");

        press(&mut app, KeyCode::Home);
        assert_eq!(app.edit_offset, 0);
    }

    #[test]
    fn saving_with_nothing_to_save_says_so() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(toast(&app), t!("tui.toast.nothing_to_save").to_string());

        press(&mut app, KeyCode::Char('e'));
        app.toast = None;
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(toast(&app), t!("tui.toast.nothing_to_save").to_string());
    }

    #[test]
    fn quitting_asks_only_when_there_is_unsaved_work() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('e'));
        type_text(&mut app, "z");
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Char('q'));
        assert_eq!(app.mode, Mode::Confirm);
        assert!(!app.should_quit);

        press(&mut app, KeyCode::Char('n'));
        assert!(!app.should_quit);

        // Written out, the same buffer is no longer a reason to ask.
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(app.buffer.is_some());
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn accepting_the_discard_question_quits() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('e'));
        type_text(&mut app, "z");
        press(&mut app, KeyCode::Esc);

        key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.mode, Mode::Confirm);
        press(&mut app, KeyCode::Char('Y'));
        assert!(app.should_quit);
    }

    #[test]
    fn ctrl_c_quits_a_session_with_nothing_open() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app.should_quit);
    }

    // ---- Mouse ----------------------------------------------------------

    #[test]
    fn the_wheel_answers_from_whichever_pane_is_under_the_pointer() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        wheel(&mut app, 5, 2, true);
        assert_eq!(app.pane, Pane::List);
        assert_eq!(app.selected_change().unwrap().id, "docs-only");
        wheel(&mut app, 5, 2, false);
        assert_eq!(app.selected_change().unwrap().id, "add-oauth");

        wheel(&mut app, 5, 13, true);
        assert_eq!(app.pane, Pane::Artifacts);
        assert!(app.open_path().is_some());

        app.sync_viewer(60);
        app.viewer_height = 2;
        wheel(&mut app, 40, 5, true);
        assert_eq!(app.pane, Pane::Viewer);
        assert!(app.scroll > 0);
        wheel(&mut app, 40, 5, false);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn the_wheel_moves_the_text_cursor_while_a_document_is_being_edited() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        press(&mut app, KeyCode::Char('e'));
        wheel(&mut app, 40, 5, true);
        assert_eq!(app.buffer.as_ref().unwrap().row, WHEEL_STEP as usize);

        // The sidebar is not the buffer: the wheel over it does nothing at all
        // rather than moving a selection out from under the typist.
        let before = app.selected_change().unwrap().id.clone();
        wheel(&mut app, 5, 2, true);
        wheel(&mut app, 5, 13, true);
        assert_eq!(app.selected_change().unwrap().id, before);
        assert_eq!(app.pane, Pane::Viewer);
    }

    #[test]
    fn clicking_the_list_selects_the_row_under_the_pointer() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        click(&mut app, 4, 2 * CHANGE_ROW_HEIGHT);
        assert_eq!(app.pane, Pane::List);
        assert_eq!(app.selected_change().unwrap().id, "offline-sync");

        // Past the last entry the click lands on nothing.
        click(&mut app, 4, 9);
        assert_eq!(app.selected_change().unwrap().id, "offline-sync");
    }

    #[test]
    fn clicking_a_tab_opens_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        let archive = app.regions.tabs[Tab::Archive.index()];
        click(&mut app, archive.x + 1, archive.y);
        assert_eq!(app.tab, Tab::Archive);
    }

    #[test]
    fn clicking_a_directory_in_the_tree_folds_it_and_a_file_opens_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        let rows = app.tree.rows().len();
        assert!(rows > 3, "the change has no nested delta to fold");

        let specs = app
            .tree
            .rows()
            .iter()
            .position(|row| row.kind == NodeKind::Directory)
            .expect("a directory row");
        click(&mut app, 4, 11 + u16::try_from(specs).unwrap());
        assert_eq!(app.pane, Pane::Artifacts);
        assert!(app.tree.rows().len() < rows, "the directory stayed open");

        click(&mut app, 4, 12);
        assert!(app.open_path().unwrap().ends_with("tasks.md"));

        // Below the last row there is nothing to select.
        let selected = app.tree.selected();
        click(&mut app, 4, 19);
        assert_eq!(app.tree.selected(), selected);
    }

    #[test]
    fn clicking_a_task_row_toggles_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        press(&mut app, KeyCode::Char('t'));
        click(&mut app, 25, 1);

        assert_eq!(app.task_cursor, 1);
        assert!(
            fixture
                .read("changes/add-oauth/tasks.md")
                .contains("- [x] 1.2")
        );

        // A click below the last task is a click on nothing.
        app.toast = None;
        click(&mut app, 25, 9);
        assert!(app.toast.is_none());
    }

    #[test]
    fn clicking_the_viewer_in_document_mode_only_moves_the_focus() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        click(&mut app, 40, 4);
        assert_eq!(app.pane, Pane::Viewer);
        assert_eq!(app.viewer, ViewerMode::Document);
    }

    #[test]
    fn clicking_the_command_bar_opens_the_command_line() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        click(&mut app, 10, 23);
        assert_eq!(app.mode, Mode::Command);
        assert!(app.draft.is_empty());
    }

    #[test]
    fn clicking_in_the_buffer_moves_the_text_cursor() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        press(&mut app, KeyCode::Char('e'));
        click(&mut app, 22, 4);
        assert_eq!(app.buffer.as_ref().unwrap().row, 4);
        assert_eq!(app.buffer.as_ref().unwrap().column, 2);

        // Outside the pane the click is not the buffer's business.
        click(&mut app, 4, 2);
        assert_eq!(app.buffer.as_ref().unwrap().row, 4);
    }

    /// An overlay covers the panes, so a stray click must not reach through it.
    #[test]
    fn a_click_under_an_overlay_only_dismisses_the_overlay() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        press(&mut app, KeyCode::Char('?'));
        click(&mut app, 4, 2 * CHANGE_ROW_HEIGHT);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(
            app.selected_change().unwrap().id,
            "add-oauth",
            "the click reached the list under the overlay"
        );

        press(&mut app, KeyCode::Char('g'));
        click(&mut app, 4, 2);
        assert_eq!(app.view, View::Dashboard);
    }

    #[test]
    fn the_wheel_scrolls_the_timeline_when_the_timeline_is_the_view() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        press(&mut app, KeyCode::Char('H'));
        app.take_pending();
        app.activity_total = 50;
        app.activity_height = 10;

        wheel(&mut app, 40, 5, true);
        assert_eq!(app.activity_scroll, WHEEL_STEP as u16);
        wheel(&mut app, 40, 5, false);
        assert_eq!(app.activity_scroll, 0);

        // A click in the timeline is not a click in a pane.
        click(&mut app, 40, 5);
        assert_eq!(app.view, View::Activity);
    }

    #[test]
    fn a_drag_that_never_reaches_the_divider_leaves_the_width_alone() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 40, 5);
        assert_eq!(app.sidebar_width, None);
        assert!(!app.dragging_divider);
    }

    #[test]
    fn dragging_the_divider_resizes_and_writes_once_it_is_let_go() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        click(&mut app, 19, 5);
        assert!(app.dragging_divider);

        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 49, 5);
        assert_eq!(app.sidebar_width, Some(50));
        assert!(
            !fixture.stored().contains("sidebar_width"),
            "a drag reports a column at a time; the file waits for the release"
        );

        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 49, 5);
        assert!(!app.dragging_divider);
        assert!(fixture.stored().contains("sidebar_width: 50"));
    }

    #[test]
    fn a_second_press_on_the_divider_hands_the_width_back_to_the_layout() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        app.run_command(":width 40");
        click(&mut app, 19, 5);
        for _ in 0..=DOUBLE_CLICK_TICKS {
            app.on_tick();
        }
        click(&mut app, 19, 5);
        assert_eq!(
            app.sidebar_width,
            Some(40),
            "two slow presses are two presses"
        );

        click(&mut app, 19, 5);
        assert_eq!(app.sidebar_width, None);
        assert_eq!(toast(&app), t!("tui.toast.sidebar_auto").to_string());
    }

    #[test]
    fn the_width_keys_answer_from_the_sidebar_and_nowhere_else() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        // A nudge measures from whatever the last frame drew.
        app.regions.sidebar = Rect::new(0, 0, 40, 20);
        press(&mut app, KeyCode::Char('>'));
        assert_eq!(app.sidebar_width, Some(41));
        press(&mut app, KeyCode::Char('<'));
        assert_eq!(app.sidebar_width, Some(40));

        app.pane = Pane::Viewer;
        let before = app.sidebar_width;
        press(&mut app, KeyCode::Char('>'));
        assert_eq!(app.sidebar_width, before, "the viewer is not the sidebar");

        // A frame that drew no sidebar has no width to nudge.
        app.pane = Pane::List;
        app.sidebar_width = None;
        app.regions.sidebar = Rect::new(0, 0, 0, 0);
        press(&mut app, KeyCode::Char('>'));
        assert_eq!(app.sidebar_width, None);
    }

    // ---- Navigation -----------------------------------------------------

    #[test]
    fn tab_cycles_the_panes_in_both_directions() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        for expected in [Pane::Artifacts, Pane::Viewer, Pane::List] {
            press(&mut app, KeyCode::Tab);
            assert_eq!(app.pane, expected);
        }
        for expected in [Pane::Viewer, Pane::Artifacts, Pane::List] {
            press(&mut app, KeyCode::BackTab);
            assert_eq!(app.pane, expected);
        }
    }

    #[test]
    fn the_arrow_keys_walk_between_the_panes() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Right);
        assert_eq!(app.pane, Pane::Artifacts);
        press(&mut app, KeyCode::Right);
        assert_eq!(app.pane, Pane::Viewer);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.pane, Pane::Artifacts);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.pane, Pane::List);

        // `l` means the same thing as the arrow outside the tree.
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.pane, Pane::Artifacts);
    }

    #[test]
    fn enter_walks_from_the_list_into_the_tree_and_into_the_viewer() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Enter);
        assert_eq!(app.pane, Pane::Artifacts);

        press(&mut app, KeyCode::Enter);
        assert_eq!(app.pane, Pane::Viewer);
        assert!(app.open_path().unwrap().ends_with("proposal.md"));

        // Nothing left to open: the viewer keeps the key.
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.pane, Pane::Viewer);

        // On a directory row the same key folds instead.
        app.pane = Pane::Artifacts;
        let directory = app
            .tree
            .rows()
            .iter()
            .position(|row| row.kind == NodeKind::Directory)
            .unwrap();
        app.tree.select(directory);
        let rows = app.tree.rows().len();
        press(&mut app, KeyCode::Enter);
        assert!(app.tree.rows().len() < rows);
        assert_eq!(app.pane, Pane::Artifacts);
    }

    #[test]
    fn enter_in_the_task_view_ticks_the_task_under_the_cursor() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('t'));
        assert_eq!(app.pane, Pane::Viewer);
        press(&mut app, KeyCode::Enter);

        assert!(
            fixture
                .read("changes/add-oauth/tasks.md")
                .contains("- [ ] 1.1")
        );
    }

    #[test]
    fn h_and_l_fold_and_unfold_inside_the_tree() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Tab);
        assert_eq!(app.pane, Pane::Artifacts);

        let directory = app
            .tree
            .rows()
            .iter()
            .position(|row| row.kind == NodeKind::Directory)
            .unwrap();
        app.tree.select(directory);

        let expanded = app.tree.rows().len();
        press(&mut app, KeyCode::Char('h'));
        assert!(
            app.tree.rows().len() < expanded,
            "the directory stayed open"
        );

        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.tree.rows().len(), expanded);

        // Already open: the key steps into the first child instead.
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.tree.selected(), Some(directory + 1));

        // On a file the fold key walks back up to the parent directory.
        app.tree.select(expanded - 1);
        press(&mut app, KeyCode::Char('h'));
        assert!(app.tree.selected().unwrap() < expanded - 1);
    }

    #[test]
    fn the_change_list_wraps_at_both_ends() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        for expected in ["docs-only", "offline-sync", "add-oauth"] {
            press(&mut app, KeyCode::Char('j'));
            assert_eq!(app.selected_change().unwrap().id, expected);
        }
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selected_change().unwrap().id, "offline-sync");

        // An empty list has nowhere to step to.
        app.run_command(":filter zzzz");
        press(&mut app, KeyCode::Char('j'));
        assert!(app.selected_change().is_none());
    }

    #[test]
    fn home_and_end_jump_to_the_ends_of_the_focused_pane() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        press(&mut app, KeyCode::End);
        assert_eq!(app.selected_change().unwrap().id, "offline-sync");
        press(&mut app, KeyCode::Home);
        assert_eq!(app.selected_change().unwrap().id, "add-oauth");

        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::End);
        assert_eq!(app.tree.selected(), Some(app.tree.rows().len() - 1));
        press(&mut app, KeyCode::Home);
        assert_eq!(app.tree.selected(), Some(0));

        app.pane = Pane::Viewer;
        app.sync_viewer(60);
        app.viewer_height = 2;
        press(&mut app, KeyCode::End);
        assert_eq!(app.scroll, app.max_scroll());
        assert!(app.scroll > 0);
        press(&mut app, KeyCode::Home);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn the_page_keys_move_the_viewer_by_a_screen() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        let body: String = (1..=200).map(|n| format!("Paragraph {n}.\n\n")).collect();
        fixture.write("changes/add-oauth/proposal.md", &body);
        app.run_command(":reload");
        app.pane = Pane::Viewer;
        app.sync_viewer(60);
        assert!(app.viewer_rows() > 100);

        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.scroll, 8, "a page is the pane less its two borders");
        key(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(app.scroll, 12);
        key(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(app.scroll, 8);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.scroll, 0);

        // A pane too short to have a page still moves by a row.
        app.viewer_height = 1;
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.scroll, 1);
    }

    #[test]
    fn the_task_cursor_wraps_and_drags_the_window_with_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);
        app.viewer_height = 1;

        press(&mut app, KeyCode::Char('t'));
        assert_eq!(app.pane, Pane::Viewer);
        assert_eq!(app.scroll, 0);

        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.task_cursor, 1);
        assert_eq!(app.scroll, 1, "the cursor left the visible window");

        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.task_cursor, 0);
        assert_eq!(app.scroll, 0);

        // In the task view a page is still one task: the cursor is the subject.
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.task_cursor, 1);
        press(&mut app, KeyCode::End);
        assert_eq!(app.task_cursor, app.tasks.len() - 1);
    }

    // ---- Viewer cache ---------------------------------------------------
    //
    // The cache is keyed on what is being shown, never on where it sits in a
    // list. Every test here moves something the key has to notice while leaving
    // the positions alone, which is exactly the shape of the bug a positional
    // key produced.

    #[test]
    fn switching_the_palette_rerenders_the_viewer() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.sync_viewer(60);
        let before = viewer_debug(&app);

        app.run_command(":theme nord");
        app.sync_viewer(60);

        assert_ne!(
            viewer_debug(&app),
            before,
            "the lines carry their colours, so the cache was stale"
        );
        assert_eq!(
            viewer_text(&app),
            viewer_text(&app),
            "the text is unchanged"
        );
    }

    #[test]
    fn switching_the_language_rerenders_the_diff_view() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('d'));
        app.sync_viewer(60);
        let before = viewer_text(&app);
        assert!(before.contains(&t!("tui.diff.title").to_string()));

        app.run_command(":lang es");
        app.sync_viewer(60);

        assert_ne!(viewer_text(&app), before);
        assert!(viewer_text(&app).contains(&t!("tui.diff.title").to_string()));
    }

    /// The path and the change are the same either side of this reload; only
    /// the bytes behind them moved.
    #[test]
    fn a_document_rewritten_on_disk_rerenders_after_a_reload() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.sync_viewer(60);
        assert!(viewer_text(&app).contains("Because tokens expire"));

        fixture.write(
            "changes/add-oauth/proposal.md",
            "# Add OAuth\n\nRewritten from scratch.\n",
        );
        app.run_command(":reload");
        app.sync_viewer(60);

        let text = viewer_text(&app);
        assert!(text.contains("Rewritten from scratch"));
        assert!(
            !text.contains("Because tokens expire"),
            "the previous body stayed on screen under the same path"
        );
    }

    #[test]
    fn narrowing_the_pane_rewraps_the_document() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        fixture.write(
            "changes/add-oauth/proposal.md",
            &format!("# Add OAuth\n\n{}\n", "word ".repeat(120)),
        );
        app.run_command(":reload");

        app.sync_viewer(100);
        let wide = app.viewer_total;
        app.sync_viewer(30);
        let narrow = app.viewer_total;

        assert!(
            narrow > wide,
            "the pre-wrapped lines were reused as they were"
        );
    }

    #[test]
    fn a_document_with_an_empty_body_renders_nothing_at_all() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        fixture.write("changes/docs-only/proposal.md", "\n   \n");
        app.run_command(":reload");
        assert!(app.focus_change("docs-only"));

        app.sync_viewer(60);
        assert_eq!(app.viewer_total, 0);
        assert!(app.viewer_is_empty());
        assert_eq!(app.max_scroll(), 0);
    }

    #[test]
    fn the_viewer_reports_the_rows_of_whichever_mode_is_open() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        press(&mut app, KeyCode::Char('t'));
        app.sync_viewer(60);
        assert_eq!(app.viewer_rows(), app.tasks.len());
        assert!(!app.viewer_is_empty());

        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('e'));
        app.sync_viewer(60);
        assert_eq!(app.viewer_rows(), app.buffer.as_ref().unwrap().len());
        assert!(!app.viewer_is_empty());

        app.buffer = None;
        assert!(app.viewer_is_empty());
    }

    #[test]
    fn the_visible_window_follows_the_scroll() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        let body: String = (1..=60).map(|n| format!("Line {n}\n\n")).collect();
        fixture.write("changes/add-oauth/proposal.md", &body);
        app.run_command(":reload");
        app.sync_viewer(60);

        let head = app.viewer_window(3);
        app.scroll = 3;
        let tail = app.viewer_window(3);

        assert_eq!(head.len(), 3);
        assert_ne!(format!("{head:?}"), format!("{tail:?}"));
    }

    // ---- Ticking --------------------------------------------------------

    #[test]
    fn a_toast_expires_on_its_own() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.notify("still here".to_string());
        for _ in 0..TOAST_TICKS - 1 {
            app.on_tick();
        }
        assert_eq!(toast(&app), "still here");

        app.on_tick();
        assert!(app.toast.is_none());
    }

    #[test]
    fn the_activity_log_forgets_its_oldest_lines() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        for index in 0..LOG_CAPACITY + 10 {
            app.say(format!("line {index}"), Tone::Muted);
        }

        assert_eq!(app.log.len(), LOG_CAPACITY);
        assert_eq!(app.log[0].text, "line 10");
        assert_eq!(
            app.log[LOG_CAPACITY - 1].text,
            format!("line {}", LOG_CAPACITY + 9)
        );
    }

    #[test]
    fn a_change_written_on_disk_is_picked_up_without_a_keystroke() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        std::fs::create_dir_all(fixture.path("changes/late-arrival")).unwrap();
        fixture.write(
            "changes/late-arrival/proposal.md",
            "# Late arrival\n\nWritten by an agent.\n",
        );

        for _ in 0..WATCH_INTERVAL {
            app.on_tick();
        }

        assert!(app.workspace.find_change("late-arrival").is_some());
        assert!(log_text(&app).contains("late-arrival"));
        assert!(
            !log_text(&app).contains(&t!("tui.log.detected").to_string()),
            "the generic line displaced the specific one"
        );
    }

    /// The generic line is the fallback, not the message: it is what is left to
    /// say when the tree moved but nothing identifiable happened in it.
    #[test]
    fn a_tree_that_moved_without_an_event_says_only_that_it_moved() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        fixture.write(
            "specs/billing/spec.md",
            "# billing Specification\n\n## Requirements\n\n\
             ### Requirement: Invoices MUST balance to the cent\n",
        );

        for _ in 0..WATCH_INTERVAL {
            app.on_tick();
        }

        assert_eq!(log_text(&app), t!("tui.log.detected").to_string());
    }

    #[test]
    fn the_watcher_holds_off_while_a_document_is_being_edited() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('e'));
        assert_eq!(app.mode, Mode::Edit);

        std::fs::create_dir_all(fixture.path("changes/late-arrival")).unwrap();
        fixture.write("changes/late-arrival/proposal.md", "# Late arrival\n");

        for _ in 0..WATCH_INTERVAL * 2 {
            app.on_tick();
        }

        assert!(
            app.workspace.find_change("late-arrival").is_none(),
            "the workspace was reloaded out from under an open buffer"
        );
    }

    // ---- Views ----------------------------------------------------------

    #[test]
    fn the_dashboard_and_the_timeline_toggle_off_the_keys_that_opened_them() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.view, View::Dashboard);
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.view, View::Editor);

        press(&mut app, KeyCode::Char('H'));
        assert_eq!(app.view, View::Activity);
        app.take_pending();
        press(&mut app, KeyCode::Char('H'));
        assert_eq!(app.view, View::Editor);

        // From the dashboard the timeline key still opens the timeline.
        press(&mut app, KeyCode::Char('g'));
        press(&mut app, KeyCode::Char('H'));
        assert_eq!(app.view, View::Activity);
    }

    #[test]
    fn the_timeline_scroll_stops_at_both_of_its_ends() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('H'));
        app.take_pending();
        app.activity_total = 30;
        app.activity_height = 10;

        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.activity_scroll, 1);
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.activity_scroll, 0);
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.activity_scroll, 0, "the top is the top");

        press(&mut app, KeyCode::End);
        assert_eq!(app.activity_scroll, 20);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.activity_scroll, 20, "the bottom is the bottom");
        press(&mut app, KeyCode::Home);
        assert_eq!(app.activity_scroll, 0);
    }

    #[test]
    fn a_timeline_shorter_than_its_pane_does_not_scroll() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('H'));
        app.take_pending();
        app.activity_total = 3;
        app.activity_height = 10;

        press(&mut app, KeyCode::End);
        assert_eq!(app.activity_scroll, 0);
    }

    // ---- Command line dispatch ------------------------------------------

    #[test]
    fn the_command_table_reaches_every_view_and_every_tab() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":dashboard");
        assert_eq!(app.view, View::Dashboard);
        app.run_command(":editor");
        assert_eq!(app.view, View::Editor);
        app.run_command(":activity");
        assert_eq!(app.view, View::Activity);
        app.take_pending();
        app.run_command(":editor");

        for (line, tab) in [
            (":specs", Tab::Specs),
            (":archived", Tab::Archive),
            (":project", Tab::Project),
            (":changes", Tab::Changes),
        ] {
            app.run_command(line);
            assert_eq!(app.tab, tab, "{line}");
        }

        app.run_command(":diff");
        assert_eq!(app.viewer, ViewerMode::Diff);
        app.run_command(":tasks");
        assert_eq!(app.viewer, ViewerMode::Tasks);

        app.run_command(":settings");
        assert_eq!(app.mode, Mode::Settings);
        app.mode = Mode::Normal;
        app.run_command(":help");
        assert_eq!(app.mode, Mode::Help);
        app.mode = Mode::Normal;

        app.say("noise", Tone::Muted);
        app.run_command(":clear");
        assert!(app.log.is_empty());

        app.run_command(":quit");
        assert!(app.should_quit);
    }

    #[test]
    fn a_command_with_an_unusable_argument_reports_the_reason() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":width wide");
        assert!(log_text(&app).contains(&t!("command.bad_width").to_string()));
        assert_eq!(app.sidebar_width, None);

        app.log.clear();
        app.run_command(":wat");
        assert!(log_text(&app).contains(&t!("command.unknown", name = "wat").to_string()));

        // An empty line is not a command at all, so nothing is recorded.
        app.run_command("   ");
        assert_eq!(app.history, [":width wide", ":wat"]);
    }

    #[test]
    fn an_unknown_palette_name_changes_nothing() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        let before = app.palette;
        app.run_command(":theme moonlight");
        assert_eq!(app.palette, before);
        assert!(log_text(&app).contains("moonlight"));

        app.run_command(":theme");
        assert_eq!(app.palette, before.next());
        assert_eq!(
            toast(&app),
            t!("tui.toast.theme_changed", theme = app.palette.name()).to_string()
        );
    }

    #[test]
    fn the_language_switches_from_a_key_and_from_a_command() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        let start = i18n::current();
        press(&mut app, KeyCode::Char('L'));
        let cycled = i18n::current();
        assert_ne!(cycled, start);
        assert_eq!(
            toast(&app),
            t!(
                "tui.toast.locale_changed",
                language = i18n::display_name(&cycled)
            )
            .to_string()
        );

        app.run_command(&format!(":lang {start}"));
        assert_eq!(i18n::current(), start);

        app.run_command(":lang");
        assert_ne!(i18n::current(), start);
    }

    #[test]
    fn the_reload_key_and_command_both_say_they_reloaded() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('r'));
        assert_eq!(toast(&app), t!("tui.toast.reloaded").to_string());

        app.toast = None;
        app.run_command(":reload");
        assert_eq!(toast(&app), t!("tui.toast.reloaded").to_string());
    }

    #[test]
    fn the_write_command_saves_the_open_buffer() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('e'));
        type_text(&mut app, "zz");
        press(&mut app, KeyCode::Esc);

        app.run_command(":w");
        assert!(
            fixture
                .read("changes/add-oauth/proposal.md")
                .starts_with("zz#")
        );
        assert!(!app.buffer.as_ref().unwrap().dirty);
    }

    #[test]
    fn the_edit_command_opens_in_place_and_a_path_hands_it_over() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":edit");
        assert_eq!(app.mode, Mode::Edit);
        press(&mut app, KeyCode::Esc);

        app.run_command(":edit tasks.md");
        assert_eq!(app.mode, Mode::Normal, "a path is somebody else's editor");
        assert_eq!(
            app.take_pending(),
            Some(Pending::Editor(
                fixture.resolved("changes/add-oauth/tasks.md")
            ))
        );
    }

    #[test]
    fn the_filter_command_reports_what_it_narrowed_to() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":filter status:draft");
        assert_eq!(toast(&app), app.filter.describe());
        assert!(app.draft.is_empty(), "the command line kept its own text");

        app.toast = None;
        app.run_command(":filter");
        assert!(!app.filter.is_active());
        assert!(app.toast.is_none(), "clearing is not a narrowing");
    }

    // ---- Scaffolding ----------------------------------------------------

    #[test]
    fn creating_a_change_writes_its_files_and_selects_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.run_command(":new Payment_Links");

        assert!(fixture.path("changes/payment-links/proposal.md").is_file());
        assert!(fixture.path("changes/payment-links/tasks.md").is_file());
        assert_eq!(app.selected_change().unwrap().id, "payment-links");
        assert_eq!(app.tab, Tab::Changes);
        assert!(
            log_text(&app)
                .contains(&t!("tui.log.change_created", change = "payment-links").to_string())
        );
    }

    #[test]
    fn creating_a_change_that_already_exists_leaves_it_alone() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        let before = fixture.read("changes/add-oauth/proposal.md");
        app.run_command(":new add-oauth");

        assert!(
            log_text(&app).contains(&t!("tui.log.change_exists", change = "add-oauth").to_string())
        );
        assert_eq!(fixture.read("changes/add-oauth/proposal.md"), before);
    }

    // ---- Free helpers ---------------------------------------------------

    #[test]
    fn a_tab_that_lists_changes_has_no_document_pool() {
        let fixture = Fixture::new();
        let app = fixture.app();

        assert!(document_pool(&app.workspace, Tab::Changes).is_empty());
        assert!(document_pool(&app.workspace, Tab::Archive).is_empty());
        assert_eq!(document_pool(&app.workspace, Tab::Specs).len(), 2);
    }

    #[test]
    fn a_specification_tab_with_nothing_selected_has_no_artifacts() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('3'));
        assert_eq!(app.artifacts().len(), 1);

        app.run_command(":filter zzzz");
        assert!(app.artifacts().is_empty());
        assert!(app.open_path().is_none());
    }

    #[test]
    fn every_label_the_interface_shows_is_translated_and_distinct() {
        let titles: Vec<String> = Tab::ORDER.iter().map(|tab| tab.title()).collect();
        assert!(titles.iter().all(|title| !title.is_empty()));
        assert_eq!(
            titles
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            titles.len()
        );

        for mode in [
            ViewerMode::Document,
            ViewerMode::Diff,
            ViewerMode::Tasks,
            ViewerMode::Edit,
        ] {
            assert!(!mode.label().is_empty(), "{mode:?}");
        }
        for setting in Setting::ORDER {
            assert!(!setting.label().is_empty());
            assert!(!setting.hint().is_empty());
        }
    }

    #[test]
    fn an_event_is_described_with_the_time_it_carries() {
        let event = Event::new(1_770_076_800, EventKind::Done, "add-oauth").about(
            crate::domain::history::TaskIdentity::of("1.1 Pick"),
            "1.1 Pick",
        );

        let sentence = describe(&event);
        assert!(sentence.starts_with("00:00"));
        assert!(sentence.contains("add-oauth"));
        assert!(sentence.contains("1.1 Pick"));
    }

    /// Un-ticking and adding tasks are two different colours in the log, and
    /// both come out of the same reload.
    #[test]
    fn a_reload_colours_each_event_by_what_it_was() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        fixture.write(
            "changes/add-oauth/tasks.md",
            "## 1. Groundwork\n\n- [ ] 1.1 Pick a library\n\
             - [ ] 1.2 Wire the refresh flow\n- [ ] 1.3 Ship it\n",
        );
        for _ in 0..WATCH_INTERVAL {
            app.on_tick();
        }

        let tones: Vec<Tone> = app.log.iter().map(|line| line.tone).collect();
        assert!(
            tones.contains(&Tone::Warning),
            "an un-ticked task is a warning"
        );
        assert!(
            tones.contains(&Tone::Muted),
            "a new task is neither good nor bad"
        );
    }

    // ---- Edges ----------------------------------------------------------

    #[test]
    fn a_complete_scan_of_a_complete_repository_says_only_what_it_found() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.report_history(Recovered {
            events: vec![Event::new(1_770_076_800, EventKind::Done, "add-oauth")],
            available: true,
            truncated: false,
            filtered: true,
            duration_ms: 5_000,
            ..Recovered::default()
        });

        let text = log_text(&app);
        assert!(text.contains(&t!("tui.log.recovered", count = 1).to_string()));
        assert!(!text.contains(&t!("tui.activity.incomplete").to_string()));
        assert!(
            !text.contains(&t!("tui.log.suggest_filters").to_string()),
            "advice about a repository that already carries the filters"
        );
    }

    #[test]
    fn a_validation_run_that_passed_is_logged_as_a_success() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        app.report_validation(&ValidationReport {
            items: vec![ItemReport {
                id: "add-oauth".to_string(),
                kind: "change".to_string(),
                valid: true,
                findings: Vec::new(),
                duration_ms: 7,
            }],
            passed: 1,
            failed: 0,
        });

        assert!(app.log.iter().all(|line| line.tone != Tone::Danger));
        assert_eq!(app.log.last().unwrap().tone, Tone::Success);
    }

    /// The preference is already live when it is written. A disk that refuses
    /// the write must cost a warning, not the setting.
    #[test]
    fn a_preference_that_cannot_be_written_is_still_applied() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        // A path whose parent is a file, so creating the directory has to fail.
        let unwritable = fixture.path("config.yaml").join("settings.yaml");
        // Safe: the fixture holds the guard that owns this variable.
        unsafe {
            std::env::set_var(crate::domain::settings::PATH_ENV, &unwritable);
        }

        let before = app.palette;
        app.open_settings();
        press(&mut app, KeyCode::Right);

        assert_ne!(app.palette, before, "the setting was rolled back");
        assert!(log_text(&app).contains(&t!("tui.log.settings_failed").to_string()));
        assert!(!unwritable.exists());
    }

    #[test]
    fn saving_into_a_directory_that_has_gone_reports_the_failure() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('e'));
        type_text(&mut app, "z");
        std::fs::remove_dir_all(fixture.path("changes/add-oauth")).unwrap();

        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);

        assert!(!log_text(&app).is_empty(), "the failure was swallowed");
        assert!(
            app.buffer.as_ref().unwrap().dirty,
            "the buffer was marked clean by a write that never happened"
        );
    }

    /// Edit mode with no buffer behind it is not a state the loop produces, but
    /// every handler that assumes one has to survive it.
    #[test]
    fn the_editing_handlers_survive_a_missing_buffer() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        press(&mut app, KeyCode::Char('e'));
        app.buffer = None;

        press(&mut app, KeyCode::Char('x'));
        click(&mut app, 30, 4);
        wheel(&mut app, 30, 4, true);
        app.reveal_cursor();

        assert_eq!(app.mode, Mode::Edit);
        assert_eq!(app.edit_offset, 0);
    }

    #[test]
    fn a_key_that_means_nothing_is_ignored_in_every_typing_mode() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Insert);
        assert_eq!(app.mode, Mode::Search);
        assert!(app.draft.is_empty());
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Char(':'));
        press(&mut app, KeyCode::Insert);
        assert_eq!(app.mode, Mode::Command);
        assert!(app.draft.is_empty());
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Char('e'));
        let before = app.buffer.as_ref().unwrap().text();
        press(&mut app, KeyCode::Insert);
        assert_eq!(app.buffer.as_ref().unwrap().text(), before);
        assert!(!app.buffer.as_ref().unwrap().dirty);

        // And in normal mode too, where the catch-all is the whole point.
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::F(9));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn stepping_the_tree_opens_documents_and_walks_over_directories() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.viewer, ViewerMode::Diff);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.pane, Pane::Artifacts);

        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.tree.selected(), Some(1));
        assert!(app.open_path().unwrap().ends_with("tasks.md"));
        assert_eq!(
            app.viewer,
            ViewerMode::Document,
            "opening a document left the diff on screen"
        );

        // A directory row has no document behind it, and the scroll is left
        // where it was rather than reset by a row that opened nothing.
        app.scroll = 4;
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(
            app.tree.selected_row().unwrap().kind,
            NodeKind::Directory,
            "the fixture has no directory row to step onto"
        );
        assert!(app.open_document().is_none());
        assert_eq!(app.scroll, 4);
    }

    #[test]
    fn stepping_the_viewer_in_document_mode_scrolls_it() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        lay_out(&mut app);

        let body: String = (1..=60).map(|n| format!("Line {n}\n\n")).collect();
        fixture.write("changes/add-oauth/proposal.md", &body);
        app.run_command(":reload");
        app.sync_viewer(60);

        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.pane, Pane::Viewer);

        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.scroll, 1);
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.scroll, 0);
    }

    /// Leaves the workspace unreadable, which is the one failure every reload
    /// path shares.
    fn break_the_workspace(fixture: &Fixture) {
        fixture.write("config.yaml", "context: [unterminated\n");
    }

    /// Whether the log carries a failure naming the file that stopped parsing.
    fn broke(app: &App) -> bool {
        app.log
            .iter()
            .any(|line| line.tone == Tone::Danger && line.text.contains("config.yaml"))
    }

    #[test]
    fn a_workspace_that_stopped_parsing_is_reported_rather_than_swallowed() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        break_the_workspace(&fixture);

        press(&mut app, KeyCode::Char('r'));
        assert!(broke(&app), "{}", log_text(&app));
        assert!(app.toast.is_none(), "a failed reload announced a success");

        app.log.clear();
        app.run_command(":reload");
        assert!(broke(&app));

        app.log.clear();
        app.after_editor(&fixture.path("changes/add-oauth/proposal.md"));
        assert!(broke(&app));

        app.log.clear();
        app.report_archive(&["moved something".to_string()]);
        assert!(broke(&app));
        assert!(!log_text(&app).contains(&t!("tui.log.archived").to_string()));

        // The session survives all of it: the last workspace read is still on
        // screen, so the user can fix the file and reload.
        assert_eq!(change_ids(&app), ["add-oauth", "docs-only", "offline-sync"]);
        assert!(!app.should_quit);
    }

    /// The write already happened when the re-read fails, so the failure has to
    /// be reported without pretending the work was lost.
    #[test]
    fn work_written_before_a_failed_reload_is_kept_and_the_failure_reported() {
        let fixture = Fixture::new();
        let mut app = fixture.app();

        press(&mut app, KeyCode::Char('t'));
        break_the_workspace(&fixture);
        press(&mut app, KeyCode::Char(' '));
        assert!(broke(&app));
        assert!(
            fixture
                .read("changes/add-oauth/tasks.md")
                .contains("- [ ] 1.1"),
            "the task was not written"
        );

        app.log.clear();
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('e'));
        type_text(&mut app, "zz");
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(broke(&app));
        assert!(
            fixture
                .read("changes/add-oauth/proposal.md")
                .starts_with("zz#")
        );
        assert!(!app.buffer.as_ref().unwrap().dirty);

        app.log.clear();
        press(&mut app, KeyCode::Esc);
        app.run_command(":new payment-links");
        assert!(broke(&app));
        assert!(fixture.path("changes/payment-links/proposal.md").is_file());
        assert!(
            !log_text(&app)
                .contains(&t!("tui.log.change_created", change = "payment-links").to_string()),
            "a change that could not be re-read was announced as created"
        );
    }
}
