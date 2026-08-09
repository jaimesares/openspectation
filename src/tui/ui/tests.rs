//! Render tests driven against an in-memory backend.
//!
//! They assert on the text a real terminal would show, which is the only thing
//! that can regress silently: a layout change that hides a pane compiles fine.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use crate::domain::workspace::Workspace;
use crate::tui::app::{App, Mode, Pane, Tab, View, ViewerMode};
use crate::tui::theme::{Palette, ansi_slot};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A nested workspace shaped the way the upstream CLI writes one.
///
/// It holds the [`redirect_for_test`](crate::domain::settings::redirect_for_test)
/// guard for its whole life, because everything here builds an [`App`] and an
/// `App` reads the user's preferences and appends to their activity record.
struct Fixture {
    root: PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
    settings: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let (guard, settings) = crate::domain::settings::redirect_for_test(&format!("ui-{unique}"));
        let root =
            std::env::temp_dir().join(format!("openspectation-ui-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let inner = root.join("openspec");
        std::fs::create_dir_all(inner.join("specs/billing")).unwrap();
        std::fs::create_dir_all(inner.join("changes/add-oauth/specs/auth")).unwrap();
        std::fs::create_dir_all(inner.join("changes/archive/2026-01-01-old-thing")).unwrap();

        std::fs::write(
            inner.join("config.yaml"),
            "schema: spec-driven\ncontext: |\n  A demo workspace.\n",
        )
        .unwrap();
        std::fs::write(
            inner.join("specs/billing/spec.md"),
            "# billing Specification\n\n## Requirements\n\n### Requirement: Invoices MUST balance\n",
        )
        .unwrap();
        std::fs::write(
            inner.join("changes/add-oauth/proposal.md"),
            "## Why\n\nBecause tokens expire.\n",
        )
        .unwrap();
        std::fs::write(
            inner.join("changes/add-oauth/tasks.md"),
            "## 1. Groundwork\n\n- [x] 1.1 Pick a library\n- [ ] 1.2 Wire the refresh flow\n",
        )
        .unwrap();
        std::fs::write(
            inner.join("changes/add-oauth/specs/auth/spec.md"),
            "## ADDED Requirements\n\n### Requirement: Tokens SHALL refresh silently\n\n\
             #### Scenario: Expired token\n\n- **WHEN** the token expired\n- **THEN** it refreshes\n\
             \n## REMOVED Requirements\n\n### Requirement: Sessions SHALL live forever\n",
        )
        .unwrap();
        std::fs::write(
            inner.join("changes/archive/2026-01-01-old-thing/proposal.md"),
            "# Old thing\n",
        )
        .unwrap();

        Self {
            root,
            _guard: guard,
            settings,
        }
    }

    fn app(&self) -> App {
        App::new(Workspace::open(&self.root).unwrap())
    }

    /// Seeds the activity record, so a view of the past has a past to show.
    fn with_history(self, events: &[crate::domain::history::Event]) -> Self {
        let body: String = events
            .iter()
            .map(|event| format!("{}\n", event.encode()))
            .collect();
        std::fs::write(crate::domain::settings::test_history_path(), body).unwrap();
        self
    }

    fn tasks(&self) -> String {
        std::fs::read_to_string(self.root.join("openspec/changes/add-oauth/tasks.md")).unwrap()
    }

    /// A second active change, so a filter has something to choose between.
    fn with_second_change(self) -> Self {
        let directory = self.root.join("openspec/changes/offline-sync");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("proposal.md"),
            "# Offline sync\n\nQueued writes survive a disconnect.\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("tasks.md"),
            "## 1. Queue\n\n- [ ] 1.1 Persist locally\n- [ ] 1.2 Drain on reconnect\n",
        )
        .unwrap();
        self
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_file(&self.settings);
        let _ = std::fs::remove_file(crate::domain::settings::test_history_path());
    }
}

fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| super::draw(frame, app)).unwrap();
    terminal.backend().to_string()
}

/// The same frame with its styles intact.
///
/// Text alone cannot see a row whose foregrounds have been repainted, which is
/// how that bug shipped; these assertions read the cells.
fn render_cells(app: &mut App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| super::draw(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn press(app: &mut App, code: KeyCode) {
    key(app, code, KeyModifiers::NONE);
}

fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.on_key(KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    })
    .unwrap();
}

fn wheel(app: &mut App, column: u16, row: u16, down: bool) {
    app.on_mouse(MouseEvent {
        kind: if down {
            MouseEventKind::ScrollDown
        } else {
            MouseEventKind::ScrollUp
        },
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn click(app: &mut App, column: u16, row: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.on_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

/// Row the panels of the working surface draw their top border on, under the
/// three-row header.
const PANEL_TOP: u16 = 3;

#[test]
fn the_editor_shows_every_pane() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let screen = render(&mut app, 120, 34);

    assert!(screen.contains("openspectation"));
    assert!(screen.contains("Changes"));
    assert!(screen.contains("Artifacts"));
    assert!(screen.contains("proposal.md"));
    // The header reports the split the landing page promises.
    assert!(screen.contains("1 active"));
    assert!(screen.contains("1 archived"));
}

#[test]
fn the_artifact_tree_sits_above_the_tab_strip() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let screen = render(&mut app, 120, 34);

    let artifacts = screen.find("Artifacts").expect("artifacts pane");
    let tabs = screen.find("Changes").expect("tab strip");
    assert!(artifacts < tabs, "the tree must head the sidebar");
}

#[test]
fn every_tab_is_on_screen_and_clickable() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let screen = render(&mut app, 120, 34);

    for tab in Tab::ORDER {
        assert!(screen.contains(&tab.title()), "{} is missing", tab.title());
    }

    let archive = app.regions.tabs[Tab::Archive.index()];
    click(&mut app, archive.x + 1, archive.y);
    let screen = render(&mut app, 120, 34);

    assert_eq!(app.tab, Tab::Archive);
    assert!(screen.contains("Old thing"));
}

#[test]
fn the_archive_holds_only_shipped_changes() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    assert_eq!(app.visible_len(), 1);
    press(&mut app, KeyCode::Char('A'));
    render(&mut app, 120, 34);

    assert_eq!(app.tab, Tab::Archive);
    assert_eq!(app.visible_len(), 1);
    assert!(app.selected_change().unwrap().archived);
}

#[test]
fn the_project_tab_holds_the_configuration_and_nothing_else() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    press(&mut app, KeyCode::Char('4'));
    let screen = render(&mut app, 120, 34);

    assert_eq!(app.tab, Tab::Project);
    assert!(screen.contains("config.yaml"));
    // The capability specs stay out of it, and so does the synthetic context
    // document: what is listed is a file the editor can write back.
    assert!(!screen.contains("billing Specification"));
    assert!(!screen.contains("Project context"));
    assert!(
        app.open_document()
            .is_some_and(|document| document.path.is_file())
    );
}

#[test]
fn the_specs_tab_lists_the_capabilities_alone() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    press(&mut app, KeyCode::Char('3'));
    let screen = render(&mut app, 120, 34);

    assert_eq!(app.tab, Tab::Specs);
    assert!(screen.contains("billing Specification"));
}

#[test]
fn the_artifact_tree_nests_the_delta_directory() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let screen = render(&mut app, 120, 34);

    assert!(screen.contains("tasks.md"));
    assert!(screen.contains("specs"));
}

#[test]
fn the_diff_view_lists_the_declared_operations() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    render(&mut app, 120, 34);

    press(&mut app, KeyCode::Char('d'));
    let screen = render(&mut app, 120, 34);

    assert_eq!(app.viewer, ViewerMode::Diff);
    assert!(screen.contains("ADDED"));
    assert!(screen.contains("REMOVED"));
    assert!(screen.contains("auth"));
}

#[test]
fn the_task_view_lists_checkboxes_with_their_state() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    render(&mut app, 120, 34);

    press(&mut app, KeyCode::Char('t'));
    let screen = render(&mut app, 120, 34);

    assert_eq!(app.viewer, ViewerMode::Tasks);
    assert!(screen.contains("[x]"));
    assert!(screen.contains("[ ]"));
    assert!(screen.contains("Pick a library"));
}

#[test]
fn space_writes_the_toggled_task_back_to_disk() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    render(&mut app, 120, 34);

    press(&mut app, KeyCode::Char('t'));
    render(&mut app, 120, 34);
    press(&mut app, KeyCode::Char(' '));

    assert!(fixture.tasks().contains("- [ ] 1.1 Pick a library"));
}

#[test]
fn the_scroll_reaches_the_last_line_of_a_document() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    let body: String = (1..=200).map(|n| format!("Paragraph {n}.\n\n")).collect();
    std::fs::write(
        fixture.root.join("openspec/changes/add-oauth/proposal.md"),
        body,
    )
    .unwrap();
    app.run_command("reload");
    render(&mut app, 120, 34);

    let viewer = app.regions.viewer;
    for _ in 0..400 {
        wheel(&mut app, viewer.x + 2, viewer.y + 2, true);
    }
    render(&mut app, 120, 34);

    assert!(app.viewer_rows() > usize::from(app.viewer_height));
    assert_eq!(app.scroll, app.max_scroll(), "the wheel stopped short");

    let screen = render(&mut app, 120, 34);
    assert!(screen.contains("Paragraph 200."));
}

#[test]
fn editing_writes_the_document_including_its_frontmatter() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    std::fs::write(
        fixture.root.join("openspec/changes/add-oauth/proposal.md"),
        "---\ntitle: Add OAuth\n---\n\n## Why\n\nBecause.\n",
    )
    .unwrap();
    app.run_command("reload");
    render(&mut app, 120, 34);

    press(&mut app, KeyCode::Char('e'));
    render(&mut app, 120, 34);
    assert_eq!(app.mode, Mode::Edit);
    assert_eq!(app.viewer, ViewerMode::Edit);

    // Typing lands in the buffer rather than triggering a command.
    for _ in 0..6 {
        press(&mut app, KeyCode::Down);
    }
    press(&mut app, KeyCode::End);
    for character in " typed".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    assert!(app.buffer.as_ref().unwrap().dirty);

    key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
    let raw = std::fs::read_to_string(fixture.root.join("openspec/changes/add-oauth/proposal.md"))
        .unwrap();

    // The frontmatter is part of the file, and the parsed body is not the file:
    // saving the body back would delete the block.
    assert_eq!(
        raw,
        "---\ntitle: Add OAuth\n---\n\n## Why\n\nBecause. typed\n"
    );
    assert!(!app.buffer.as_ref().unwrap().dirty);
}

#[test]
fn leaving_the_editor_keeps_unsaved_work_and_guards_the_exit() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    render(&mut app, 120, 34);

    press(&mut app, KeyCode::Char('e'));
    render(&mut app, 120, 34);
    press(&mut app, KeyCode::Char('x'));

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.buffer.as_ref().unwrap().dirty);

    press(&mut app, KeyCode::Char('q'));
    let screen = render(&mut app, 120, 34);
    assert!(!app.should_quit);
    assert!(screen.contains("Confirm"));

    press(&mut app, KeyCode::Char('n'));
    assert!(!app.should_quit);
}

#[test]
fn the_dashboard_replaces_the_editor_and_comes_back() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    press(&mut app, KeyCode::Char('g'));
    let screen = render(&mut app, 120, 34);
    assert_eq!(app.view, View::Dashboard);
    assert!(screen.contains("Status distribution"));

    press(&mut app, KeyCode::Esc);
    let screen = render(&mut app, 120, 34);
    assert_eq!(app.view, View::Editor);
    assert!(screen.contains("Artifacts"));
}

#[test]
fn the_command_line_opens_and_reports_an_unknown_command() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    press(&mut app, KeyCode::Char(':'));
    for character in "wat".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    let typing = render(&mut app, 120, 34);
    assert!(typing.contains("wat"));

    press(&mut app, KeyCode::Enter);
    let screen = render(&mut app, 120, 34);
    assert!(screen.contains("Activity"));
    assert!(screen.contains("wat"));
}

#[test]
fn the_theme_command_selects_cycles_and_rejects() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    assert_eq!(
        app.palette,
        Palette::Terminal,
        "the default follows the terminal"
    );

    app.run_command("theme nord");
    render(&mut app, 120, 34);
    assert_eq!(app.palette, Palette::Nord);
    assert_eq!(app.theme.primary, Palette::Nord.theme().primary);

    // A bare `:theme` walks to the next palette.
    app.run_command("theme");
    assert_eq!(app.palette, Palette::Gruvbox);

    app.run_command("theme solarized");
    let screen = render(&mut app, 120, 34);
    assert_eq!(app.palette, Palette::Gruvbox, "a bad name changes nothing");
    assert!(screen.contains("solarized"));
}

#[test]
fn a_filter_expression_narrows_the_list() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    app.run_command("filter tasks<100%");
    let screen = render(&mut app, 120, 34);

    assert_eq!(app.visible_len(), 1);
    assert!(screen.contains("Add Oauth"));
}

#[test]
fn the_help_overlay_covers_the_body() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    press(&mut app, KeyCode::Char('?'));
    let screen = render(&mut app, 120, 34);
    assert!(screen.contains("Toggle the task under the cursor"));

    // The catalogue is all-or-nothing, so it needs a viewport tall enough.
    let tall = render(&mut app, 120, 52);
    assert!(tall.contains("Command line"));
    assert!(tall.contains(":validate"));
    assert!(tall.contains(":theme"));
}

#[test]
fn archiving_asks_before_acting() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    render(&mut app, 120, 34);

    press(&mut app, KeyCode::Char('a'));
    let screen = render(&mut app, 120, 34);

    // Without the upstream CLI the request is refused instead of prompting;
    // either way nothing is archived without an explicit answer.
    if app.cli_version.is_some() {
        assert!(screen.contains("Confirm"));
        assert!(screen.contains("add-oauth"));
        press(&mut app, KeyCode::Char('n'));
        assert!(app.take_pending().is_none());
    } else {
        assert!(screen.contains("openspec"));
    }
}

#[test]
fn narrow_and_tiny_viewports_survive() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    for (width, height) in [(20, 6), (40, 12), (70, 18), (200, 60)] {
        let screen = render(&mut app, width, height);
        assert!(!screen.is_empty());
    }

    // Below the split threshold only the focused pane is drawn.
    app.pane = Pane::Viewer;
    let narrow = render(&mut app, 60, 20);
    assert!(!narrow.contains("Artifacts"));
}

/// A filter can leave the selected index alone while changing what that index
/// resolves to. Keying the viewer cache on positions called that a cache hit,
/// so the previous document stayed on screen under the new title.
#[test]
fn filtering_to_another_change_rerenders_the_viewer() {
    let fixture = Fixture::new().with_second_change();
    let mut app = fixture.app();

    let before = render(&mut app, 120, 34);
    assert!(before.contains("Because tokens expire"));

    // add-oauth is half done; offline-sync has nothing ticked off.
    app.run_command(":filter tasks<50%");
    let after = render(&mut app, 120, 34);

    assert!(
        after.contains("Queued writes survive"),
        "the surviving change is not on screen"
    );
    assert!(
        !after.contains("Because tokens expire"),
        "the filtered-out change is still rendered"
    );
}

/// The selected row is the row being worked on, so it is the row whose colours
/// carry the most. `highlight_style` painted every one of them over.
#[test]
fn a_selected_change_keeps_the_colours_it_encodes() {
    let fixture = Fixture::new();

    // `terminal` is where the fix has to hold: it draws from the sixteen ANSI
    // slots alone, so several colour roles land on the same one.
    for palette in [Palette::Terminal, Palette::Nord] {
        let mut app = fixture.app();
        app.run_command(&format!("theme {}", palette.name()));
        let cells = render_cells(&mut app, 120, 34);

        assert_eq!(app.palette, palette);
        assert_eq!(app.list_state().selected(), Some(0), "nothing is selected");

        let theme = app.theme;
        let list = app.regions.list;
        let status = app.selected_change().expect("a selected change").status;

        let glyph = cells.cell((list.x, list.y)).expect("the status glyph");
        assert_eq!(
            glyph.bg, theme.selection,
            "the selected row lost its highlight under {palette:?}"
        );
        assert_eq!(
            glyph.fg,
            theme.status(status),
            "the status glyph lost its status colour under {palette:?}"
        );

        // The rail of the progress bar, filled part first.
        let rail: Vec<_> = (list.x..list.right())
            .filter_map(|x| cells.cell((x, list.y + 1)))
            .filter(|cell| matches!(cell.symbol(), "━" | "╸"))
            .collect();
        assert!(rail.len() >= 2, "no progress bar under {palette:?}");
        let empty = rail[rail.len() - 1];
        assert_ne!(
            ansi_slot(rail[0].fg),
            ansi_slot(empty.fg),
            "the progress bar collapsed to one colour under {palette:?}"
        );

        // The empty rail is drawn on the selection band, so its colour has to
        // be picked against that background and not against the panel.
        assert_eq!(empty.bg, theme.selection);
        assert_ne!(
            ansi_slot(empty.fg),
            ansi_slot(empty.bg),
            "the empty rail vanished into the selection band under {palette:?}"
        );
    }
}

#[test]
fn a_selected_document_keeps_its_path_subordinate() {
    let fixture = Fixture::new();

    for palette in [Palette::Terminal, Palette::Nord] {
        let mut app = fixture.app();
        app.run_command(&format!("theme {}", palette.name()));
        press(&mut app, KeyCode::Char('3'));
        let cells = render_cells(&mut app, 120, 34);

        assert_eq!(app.tab, Tab::Specs);
        assert_eq!(app.list_state().selected(), Some(0), "nothing is selected");

        let theme = app.theme;
        let list = app.regions.list;

        // Title on the head line, path on the line below it.
        let title = cells.cell((list.x + 2, list.y)).expect("the title");
        let detail = cells.cell((list.x + 3, list.y + 1)).expect("the path");
        assert_eq!(title.bg, theme.selection, "the row lost its highlight");
        assert_eq!(title.fg, theme.text_strong);
        assert_eq!(
            detail.fg, theme.text_dim,
            "the path was promoted to the title's colour under {palette:?}"
        );
        assert_ne!(
            detail.fg, title.fg,
            "the path and the title share a colour under {palette:?}"
        );
    }
}

#[test]
fn the_activity_view_shows_the_timeline_and_what_it_implies() {
    use crate::domain::history::{Event, EventKind, TaskIdentity};

    let label = "1.1 Pick a library";
    let identity = TaskIdentity::of(label);
    // Appeared at midnight on 2026-02-03, completed two hours later.
    let appeared = 1_770_076_800;

    let fixture = Fixture::new().with_history(&[
        Event::new(appeared, EventKind::TaskAdded, "add-oauth").about(identity, label),
        Event::new(appeared + 7_200, EventKind::Done, "add-oauth").about(identity, label),
    ]);
    let mut app = fixture.app();

    press(&mut app, KeyCode::Char('H'));
    let screen = render(&mut app, 120, 34);

    assert_eq!(app.view, View::Activity);
    assert!(screen.contains("Timeline"));
    // The day heads its group, and the entry names its change and its task.
    assert!(screen.contains("2026-02-03"), "{screen}");
    assert!(screen.contains("add-oauth"));
    assert!(screen.contains("Pick a library"));
    // Two hours between appearing and being done, and the screen says what a
    // duration measures.
    assert!(screen.contains("2 h"), "{screen}");
    assert!(screen.contains("elapsed time"));

    // The derived half: progress out of the workspace, and an estimate for the
    // task still outstanding built from the one already done.
    assert!(screen.contains("By change"));
    assert!(screen.contains("1 of 2 tasks"), "{screen}");
    assert!(screen.contains("median"));

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.view, View::Editor);
}

#[test]
fn an_empty_history_says_so_rather_than_showing_a_blank() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    app.run_command("activity");
    let screen = render(&mut app, 120, 34);

    assert_eq!(app.view, View::Activity);
    assert!(screen.contains("Nothing has been recorded"), "{screen}");
}

/// The scan is a subprocess; a key handler that ran it would freeze the loop.
#[test]
fn opening_the_timeline_defers_the_version_control_scan() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    press(&mut app, KeyCode::Char('H'));
    let screen = render(&mut app, 120, 34);
    assert!(screen.contains("reading task history"), "{screen}");
    assert!(app.take_pending().is_some(), "the scan was not queued");

    // Only on the first visit: leaving and returning must not rescan.
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('H'));
    assert!(app.take_pending().is_none());
}

/// Reloading the workspace under an open buffer is refused, and so is scanning.
#[test]
fn no_scan_is_started_while_a_document_is_being_edited() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    render(&mut app, 120, 34);

    press(&mut app, KeyCode::Char('e'));
    render(&mut app, 120, 34);
    assert_eq!(app.mode, Mode::Edit);

    app.run_command("activity");
    assert!(app.take_pending().is_none());
}

/// The generic line is what is left to say when nothing identifiable happened.
#[test]
fn the_activity_panel_names_the_task_it_saw_completed() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    render(&mut app, 120, 34);

    press(&mut app, KeyCode::Char('t'));
    render(&mut app, 120, 34);
    // 1.1 is already done; move to the outstanding one and tick it.
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char(' '));

    let screen = render(&mut app, 120, 34);
    assert!(screen.contains("Activity"));
    assert!(
        screen.contains("done: 1.2 Wire the refresh flow"),
        "{screen}"
    );
    assert!(
        !screen.contains("workspace changed on disk"),
        "the generic line displaced the specific one"
    );

    // And it outlives the session.
    let reopened = fixture.app();
    assert!(
        reopened
            .record
            .events()
            .iter()
            .any(|event| event.label.contains("Wire the refresh flow"))
    );
}

#[test]
fn the_settings_panel_stores_what_it_changes() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    press(&mut app, KeyCode::Char(','));
    let screen = render(&mut app, 120, 34);
    assert!(screen.contains("Settings"));
    assert!(screen.contains("Theme"));
    assert!(screen.contains("Language"));
    assert!(screen.contains("terminal"), "the live palette is the value");

    let before = app.palette;
    press(&mut app, KeyCode::Right);
    assert_ne!(app.palette, before);
    assert_eq!(app.settings.theme.as_deref(), Some(app.palette.name()));
    assert!(
        std::fs::read_to_string(&fixture.settings)
            .unwrap()
            .contains("theme:"),
        "the change was applied but never written"
    );

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn the_gear_opens_the_panel_and_a_row_click_advances_it() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    // Regions come from the frame just drawn.
    render(&mut app, 120, 34);
    let gear = app.regions.settings;
    click(&mut app, gear.x + 1, gear.y);
    assert_eq!(app.mode, Mode::Settings);

    render(&mut app, 120, 34);
    let row = app.regions.setting_rows[0];
    let before = app.palette;
    click(&mut app, row.x + 1, row.y);
    assert_ne!(app.palette, before, "clicking a row must move its value");
}

#[test]
fn a_chosen_sidebar_width_outgrows_the_automatic_cap_and_stops_at_the_minimum() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    // Left to itself the sidebar stops at 42 columns on a terminal this wide.
    render(&mut app, 140, 34);
    assert_eq!(app.regions.sidebar.width, 42);

    app.run_command(":width 60");
    let cells = render_cells(&mut app, 140, 34);
    assert_eq!(app.regions.sidebar.width, 60);
    // The panes meet where the sidebar ends: its corner, then the viewer's.
    assert_eq!(cells.cell((59, PANEL_TOP)).unwrap().symbol(), "╮");
    assert_eq!(cells.cell((60, PANEL_TOP)).unwrap().symbol(), "╭");

    app.run_command(":width 10");
    render(&mut app, 140, 34);
    assert_eq!(app.regions.sidebar.width, 28);

    // Wider than the terminal can hold: drawn against the viewer's reserve,
    // and the request itself left alone for a screen that can honour it.
    app.run_command(":width 200");
    render(&mut app, 140, 34);
    assert_eq!(app.regions.sidebar.width, 100);
    assert_eq!(app.sidebar_width, Some(200));
}

#[test]
fn dragging_the_divider_resizes_the_sidebar_and_writes_once_it_is_let_go() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    render(&mut app, 140, 34);

    let divider = app.regions.divider;
    click(&mut app, divider.x + 1, divider.y + 4);
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        79,
        divider.y + 4,
    );

    assert_eq!(app.sidebar_width, Some(80));
    assert!(
        !std::fs::read_to_string(&fixture.settings)
            .unwrap_or_default()
            .contains("sidebar_width"),
        "a drag reports a column at a time; the file waits for the release"
    );

    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        79,
        divider.y + 4,
    );
    assert!(
        std::fs::read_to_string(&fixture.settings)
            .unwrap()
            .contains("sidebar_width: 80")
    );

    render(&mut app, 140, 34);
    assert_eq!(app.regions.sidebar.width, 80);
}

#[test]
fn a_second_press_on_the_divider_returns_the_sidebar_to_automatic() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    app.run_command(":width 70");
    render(&mut app, 140, 34);
    let divider = app.regions.divider;

    // Two presses far apart are two presses: the first only takes the grab.
    click(&mut app, divider.x + 1, divider.y + 4);
    for _ in 0..3 {
        app.on_tick();
    }
    click(&mut app, divider.x + 1, divider.y + 4);
    assert_eq!(app.sidebar_width, Some(70));

    click(&mut app, divider.x + 1, divider.y + 4);
    assert_eq!(app.sidebar_width, None);
    assert!(
        !std::fs::read_to_string(&fixture.settings)
            .unwrap()
            .contains("sidebar_width")
    );

    render(&mut app, 140, 34);
    assert_eq!(app.regions.sidebar.width, 42);
}

#[test]
fn the_width_keys_answer_from_the_sidebar_and_nowhere_else() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    render(&mut app, 140, 34);

    press(&mut app, KeyCode::Char('>'));
    assert_eq!(app.sidebar_width, Some(43));
    press(&mut app, KeyCode::Char('<'));
    press(&mut app, KeyCode::Char('<'));
    assert_eq!(app.sidebar_width, Some(41));

    app.pane = Pane::Viewer;
    press(&mut app, KeyCode::Char('>'));
    assert_eq!(app.sidebar_width, Some(41), "the viewer is not the sidebar");

    // The editor takes every printable key, so the pair cannot reach the
    // sidebar from a buffer even though the tree still has the focus.
    app.pane = Pane::Artifacts;
    press(&mut app, KeyCode::Char('e'));
    render(&mut app, 140, 34);
    assert_eq!(app.mode, Mode::Edit);
    press(&mut app, KeyCode::Char('>'));
    assert_eq!(app.sidebar_width, Some(41));
    assert!(app.buffer.as_ref().is_some_and(|buffer| buffer.dirty));
}

#[test]
fn a_terminal_too_narrow_for_the_sidebar_leaves_the_stored_width_alone() {
    let fixture = Fixture::new();
    let mut app = fixture.app();

    app.run_command(":width 60");
    render(&mut app, 140, 34);
    assert_eq!(app.regions.sidebar.width, 60);

    // One pane takes the terminal, so there is no sidebar and no divider.
    let screen = render(&mut app, 80, 34);
    assert_eq!(app.regions.sidebar.width, 0);
    assert_eq!(app.regions.divider.width, 0);
    assert!(screen.contains("Changes"));

    press(&mut app, KeyCode::Char('>'));
    click(&mut app, 59, 10);
    assert_eq!(app.sidebar_width, Some(60), "nothing on screen to resize");
    assert!(
        std::fs::read_to_string(&fixture.settings)
            .unwrap()
            .contains("sidebar_width: 60")
    );

    render(&mut app, 140, 34);
    assert_eq!(app.regions.sidebar.width, 60);
}

/// Two tasks finished two days apart, so the timeline has more than one day to
/// group and the derived half has more than one sample to work from.
fn two_days_of_history() -> Fixture {
    use crate::domain::history::{Event, EventKind, TaskIdentity};

    let first = "1.1 Pick a library";
    let second = "1.2 Wire the refresh flow";
    // Midnight on 2026-02-03.
    let day_one = 1_770_076_800;
    let day_three = day_one + 2 * 86_400;

    Fixture::new().with_history(&[
        Event::new(day_one, EventKind::TaskAdded, "add-oauth")
            .about(TaskIdentity::of(first), first),
        Event::new(day_one + 3_600, EventKind::Done, "add-oauth")
            .about(TaskIdentity::of(first), first),
        Event::new(day_three, EventKind::TaskAdded, "add-oauth")
            .about(TaskIdentity::of(second), second),
        Event::new(day_three + 7_200, EventKind::Done, "add-oauth")
            .about(TaskIdentity::of(second), second),
    ])
}

#[test]
fn the_timeline_heads_every_day_it_covers() {
    let fixture = two_days_of_history();
    let mut app = fixture.app();

    press(&mut app, KeyCode::Char('H'));
    let screen = render(&mut app, 120, 40);

    assert_eq!(app.view, View::Activity);
    assert!(screen.contains("2026-02-03"), "{screen}");
    assert!(screen.contains("2026-02-05"), "{screen}");
    assert!(screen.contains("Pick a library"));
    assert!(screen.contains("Wire the refresh flow"));
}

#[test]
fn a_narrow_terminal_keeps_the_timeline_and_drops_the_derived_half() {
    // Half a table of figures is worse than none.
    let fixture = two_days_of_history();
    let mut app = fixture.app();

    press(&mut app, KeyCode::Char('H'));
    let wide = render(&mut app, 120, 40);
    assert!(wide.contains("By change"));

    let narrow = render(&mut app, super::NARROW_WIDTH - 1, 40);
    assert!(narrow.contains("Timeline"));
    assert!(!narrow.contains("By change"), "{narrow}");
}

#[test]
fn selecting_a_directory_row_summarises_the_change_instead_of_a_document() {
    // A directory resolves to no document, and the viewer must say something
    // about the change rather than going blank.
    let fixture = Fixture::new();
    let mut app = fixture.app();
    render(&mut app, 120, 34);

    let directory = app
        .tree
        .rows()
        .iter()
        .position(|row| row.kind == crate::tui::tree::NodeKind::Directory)
        .expect("the delta directory");
    app.tree.select(directory);

    assert_eq!(app.tree.selected_document(), None);
    let screen = render(&mut app, 120, 34);

    for label in ["Identifier", "Documents", "Shape", "Tasks", "Deltas"] {
        assert!(screen.contains(label), "{label} missing from the summary");
    }
    assert!(screen.contains("add-oauth"));
}

#[test]
fn the_label_column_widens_to_its_longest_label_and_stops_there() {
    let labels = |values: &[&str]| -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    };

    // Longest label plus the gap that separates it from its value.
    assert_eq!(super::label_column(&labels(&["id", "documents"]), 40), 11);
    // A label longer than the pane cannot push the values off the edge.
    assert_eq!(
        super::label_column(&labels(&["a-very-long-field-name"]), 12),
        8
    );
    // Nothing to measure still leaves a usable column.
    assert_eq!(super::label_column(&[], 40), 6);
    // And a pane too narrow for even that falls back to the floor.
    assert_eq!(super::label_column(&labels(&["id"]), 4), 4);
}

#[test]
fn a_centered_rectangle_stays_inside_the_area_it_is_given() {
    use ratatui::layout::Rect;

    let area = Rect::new(0, 0, 100, 100);
    assert_eq!(
        super::centered_rect(area, 50, 50),
        Rect::new(25, 25, 50, 50)
    );
    assert_eq!(super::centered_rect(area, 100, 100), area);

    // Offset areas keep their origin rather than snapping to the screen.
    let offset = Rect::new(10, 5, 80, 40);
    let centered = super::centered_rect(offset, 50, 50);
    assert!(centered.x >= offset.x && centered.y >= offset.y);
    assert!(centered.right() <= offset.right() && centered.bottom() <= offset.bottom());
}
