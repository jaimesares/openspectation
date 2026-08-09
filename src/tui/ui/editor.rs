//! The working surface: artifact tree, sidebar tabs, viewer and activity log.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use rust_i18n::t;

use crate::domain::change::ChangeProposal;
use crate::domain::editor::Buffer;
use crate::domain::spec::{DocKind, SpecDocument};
use crate::domain::tasks::TaskLine;
use crate::domain::util;
use crate::tui::app::{App, CHANGE_ROW_HEIGHT, Mode, Pane, Tab, Tone, ViewerMode};
use crate::tui::theme::Theme;
use crate::tui::tree::{NodeKind, Row};
use crate::tui::ui::{
    NARROW_WIDTH, SHORT_HEIGHT, SIDEBAR_MAX, SIDEBAR_MIN, Track, VIEWER_RESERVE, draw_scrollbar,
    empty_paragraph, field, focused, label_column, panel_block, viewer_badge,
};

/// Tallest the activity log is allowed to grow, borders included.
const LOG_MAX: u16 = 10;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    // A narrow terminal cannot hold three columns; the focused one takes over.
    if area.width < NARROW_WIDTH {
        app.regions = crate::tui::app::Regions {
            command: app.regions.command,
            ..Default::default()
        };
        match app.pane {
            Pane::List => draw_list(frame, app, area),
            Pane::Artifacts => draw_artifacts(frame, app, area),
            Pane::Viewer => draw_main(frame, app, area),
        }
        return;
    }

    let columns = Layout::horizontal([
        Constraint::Length(sidebar_width(app.sidebar_width, area.width)),
        Constraint::Min(VIEWER_RESERVE),
    ])
    .split(area);

    app.regions.sidebar = columns[0];
    // The divider is one column of border, which is accurate to aim at and
    // unpleasant to hit. Widening the target by a cell on each side changes
    // what is tested, not what is drawn.
    app.regions.divider = Rect {
        x: columns[0].right().saturating_sub(2),
        y: columns[0].y,
        width: 3,
        height: columns[0].height,
    };

    // The files are what gets worked on, so the tree takes the sidebar and the
    // entry list sits under it as a chooser. Its height is fixed rather than
    // sized to its contents: a pane that moves when the selection changes turns
    // clicking through a list into a game of hitting a moving target.
    let list_height = (columns[0].height / 3).clamp(7, 15);

    let sidebar =
        Layout::vertical([Constraint::Min(6), Constraint::Length(list_height)]).split(columns[0]);

    draw_artifacts(frame, app, sidebar[0]);
    draw_list(frame, app, sidebar[1]);
    draw_main(frame, app, columns[1]);
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

/// How wide the sidebar is drawn, given what the user asked for.
///
/// A chosen width is clamped to what fits and never written back: the stored
/// preference is the request, not what last fitted, so moving to a narrower
/// terminal and back leaves it where the user put it.
fn sidebar_width(chosen: Option<u16>, total: u16) -> u16 {
    match chosen {
        Some(width) => width.clamp(
            SIDEBAR_MIN,
            total.saturating_sub(VIEWER_RESERVE).max(SIDEBAR_MIN),
        ),
        None => (total / 3).clamp(SIDEBAR_MIN, SIDEBAR_MAX),
    }
}

fn draw_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let is_focused = focused(app, Pane::List);

    // No border title: the tab strip already names the pane, and saying it
    // twice reads as two different things.
    let count = app.visible_len();
    let block = panel_block(theme, "", is_focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // The tab strip is a row of the pane rather than a hint on its border: the
    // previous design switched sources on a click nobody could see was there.
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
    draw_tabs(frame, app, rows[0]);

    let inner = rows[1];
    app.regions.list = inner;

    if count == 0 {
        frame.render_widget(empty_paragraph(theme, &empty_message(app)), inner);
        return;
    }

    let width = usize::from(inner.width);
    let selected = app.list_state().selected();
    // Selection is the item's own style rather than `highlight_style`, which the
    // widget applies over the finished row and which would repaint every
    // foreground the row encodes. See the note on `selection_style`.
    let is_selected = |row: usize| selected == Some(row);
    let items: Vec<ListItem<'static>> = if app.tab.holds_changes() {
        app.visible()
            .iter()
            .filter_map(|index| app.workspace.changes.get(*index))
            .enumerate()
            .map(|(row, change)| change_item(theme, change, width, is_selected(row)))
            .collect()
    } else {
        let pool = crate::tui::app::document_pool(&app.workspace, app.tab);
        app.visible()
            .iter()
            .filter_map(|index| pool.get(*index).copied())
            .enumerate()
            .map(|(row, document)| spec_item(theme, document, width, is_selected(row)))
            .collect()
    };

    let list = List::new(items);
    frame.render_stateful_widget(list, inner, &mut app.states[app.tab.index()]);

    draw_scrollbar(
        frame,
        theme,
        area,
        count,
        selected.unwrap_or(0),
        usize::from(CHANGE_ROW_HEIGHT),
        Track::Selection,
    );
}

fn empty_message(app: &App) -> String {
    let key = match app.tab {
        _ if app.filter.is_active() => "tui.empty.filtered",
        Tab::Changes => "tui.empty.changes",
        Tab::Archive => "tui.empty.archive",
        Tab::Specs => "tui.empty.specs",
        Tab::Project => "tui.empty.project",
    };
    t!(key).to_string()
}

/// Draws the tab strip and records where each tab was put, so it can be clicked.
fn draw_tabs(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let labels: Vec<String> = Tab::ORDER.iter().map(|tab| tab.title()).collect();
    let counts: Vec<usize> = Tab::ORDER.iter().map(|tab| app.tab_len(*tab)).collect();

    // Each tab costs its label, a space either side, and its count. The counts
    // of the inactive tabs go first when the sidebar is tight — the one that
    // matters is the size of the list actually on screen.
    let plain: usize = labels.iter().map(|label| label.chars().count() + 2).sum();
    let digits = |value: usize| value.to_string().chars().count() + 1;
    let numbered: usize = plain + counts.iter().copied().map(digits).sum::<usize>();

    let room = usize::from(area.width);
    let with_counts = numbered <= room;
    let budget = if with_counts {
        numbered
    } else {
        plain + digits(counts[app.tab.index()])
    };
    // Last resort: share what there is, evenly.
    let cap = if budget <= room {
        usize::MAX
    } else {
        (room / Tab::ORDER.len()).saturating_sub(2).max(3)
    };

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut x = area.x;

    for (index, tab) in Tab::ORDER.iter().enumerate() {
        let mut text = format!(" {}", util::truncate(&labels[index], cap));
        if with_counts || *tab == app.tab {
            text.push_str(&format!(" {}", counts[index]));
        }
        text.push(' ');

        let width = u16::try_from(text.chars().count()).unwrap_or(0);
        let style = if *tab == app.tab {
            Style::new()
                .fg(theme.background)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme.text_dim)
        };

        app.regions.tabs[tab.index()] = Rect {
            x,
            y: area.y,
            width: width.min(area.width.saturating_sub(x - area.x)),
            height: 1,
        };
        x = x.saturating_add(width);

        spans.push(Span::styled(text, style));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The style a sidebar row carries when it is the selected one.
///
/// A background and nothing else: the widget lays the item style down before
/// the content, so every span still paints its own foreground over it, and the
/// cells no span reaches keep the highlight to the full row width.
fn selection_style(theme: Theme, selected: bool) -> Style {
    if selected {
        Style::new().bg(theme.selection)
    } else {
        Style::new()
    }
}

fn change_item(
    theme: Theme,
    change: &ChangeProposal,
    width: usize,
    selected: bool,
) -> ListItem<'static> {
    let text_width = width.saturating_sub(4).max(8);
    let progress = change.progress();

    // The counter is the point of the line, so it claims its width before the
    // bar does; a bar clipped by two characters is still a bar.
    let counter = if change.tasks_total > 0 {
        format!("  {}/{}", change.tasks_done, change.tasks_total)
    } else {
        String::new()
    };
    let bar_width = text_width
        .saturating_sub(8 + counter.chars().count())
        .clamp(4, 26);
    let bar = util::bar(progress, bar_width);

    let head = Line::from(vec![
        Span::styled(
            format!("{} ", change.status.glyph()),
            Style::new().fg(theme.status(change.status)),
        ),
        Span::styled(
            util::truncate(&change.title, text_width),
            Style::new().fg(theme.text_strong),
        ),
    ]);

    // `border` is a near-background role and so is the selection: on the
    // selection band the track needs a colour picked against it, which is what
    // `track_selected` is for.
    let track = if selected {
        theme.track_selected
    } else {
        theme.border
    };

    let detail = Line::from(vec![
        Span::raw("  "),
        Span::styled(bar.filled, Style::new().fg(theme.progress(progress))),
        Span::styled(bar.track, Style::new().fg(track)),
        Span::styled(format!(" {:>3.0}%", progress * 100.0), theme.dim()),
        Span::styled(counter, theme.dim()),
    ]);

    ListItem::new(vec![head, detail]).style(selection_style(theme, selected))
}

fn spec_item(
    theme: Theme,
    document: &SpecDocument,
    width: usize,
    selected: bool,
) -> ListItem<'static> {
    let text_width = width.saturating_sub(4).max(8);

    let head = Line::from(vec![
        Span::styled(
            format!("{} ", document.status.glyph()),
            Style::new().fg(theme.status(document.status)),
        ),
        Span::styled(
            util::truncate(&document.title, text_width),
            Style::new().fg(theme.text_strong),
        ),
    ]);

    let detail = Line::from(vec![
        Span::raw("   "),
        Span::styled(
            util::truncate(&document.display_path(), text_width.saturating_sub(12)),
            theme.dim(),
        ),
        Span::styled("  ·  ", theme.dim()),
        Span::styled(util::format_size(document.size), theme.dim()),
    ]);

    ListItem::new(vec![head, detail]).style(selection_style(theme, selected))
}

fn draw_artifacts(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let is_focused = focused(app, Pane::Artifacts);

    let block = panel_block(theme, &t!("tui.panel.artifacts"), is_focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    app.regions.artifacts = inner;

    if app.tree.is_empty() {
        frame.render_widget(empty_paragraph(theme, &t!("tui.empty.artifacts")), inner);
        return;
    }

    let selected = app.tree.selected().unwrap_or(0);
    let width = usize::from(inner.width);

    let lines: Vec<Line<'static>> = app
        .tree
        .rows()
        .iter()
        .enumerate()
        .skip(scroll_offset(
            selected,
            usize::from(inner.height),
            app.tree.rows().len(),
        ))
        .take(usize::from(inner.height))
        .map(|(index, row)| tree_line(theme, row, index == selected, width))
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
    draw_scrollbar(
        frame,
        theme,
        area,
        app.tree.rows().len(),
        selected,
        1,
        Track::Selection,
    );
}

/// First visible row so the cursor stays inside a viewport of `height`.
fn scroll_offset(selected: usize, height: usize, total: usize) -> usize {
    if height == 0 || total <= height {
        return 0;
    }
    selected
        .saturating_sub(height / 2)
        .min(total.saturating_sub(height))
}

fn tree_line(theme: Theme, row: &Row, selected: bool, width: usize) -> Line<'static> {
    let indent = "  ".repeat(row.depth);
    let marker = match row.kind {
        NodeKind::Directory if row.expanded => "▾ ",
        NodeKind::Directory => "▸ ",
        NodeKind::Document => "  ",
    };

    let suffix = if row.tasks_total > 0 {
        format!(" {}/{}", row.tasks_done, row.tasks_total)
    } else if row.kind == NodeKind::Directory {
        format!(" {}", row.documents)
    } else {
        String::new()
    };

    let room = width
        .saturating_sub(indent.chars().count() + 2 + suffix.chars().count() + 1)
        .max(4);
    let label = util::truncate(&row.label, room);

    let name_style = if selected {
        Style::new()
            .fg(theme.text_strong)
            .add_modifier(Modifier::BOLD)
    } else if row.kind == NodeKind::Directory {
        Style::new().fg(theme.text)
    } else {
        Style::new().fg(theme.text_dim)
    };

    let mut spans = vec![
        Span::styled(
            if selected { "▌" } else { " " },
            Style::new().fg(theme.border_focus),
        ),
        Span::raw(indent),
        Span::styled(marker.to_string(), Style::new().fg(theme.accent)),
        Span::styled(label, name_style),
    ];
    if !suffix.is_empty() {
        spans.push(Span::styled(suffix, theme.dim()));
    }

    let line = Line::from(spans);
    if selected {
        line.style(Style::new().bg(theme.selection))
    } else {
        line
    }
}

// ---------------------------------------------------------------------------
// Main column
// ---------------------------------------------------------------------------

fn draw_main(frame: &mut Frame, app: &mut App, area: Rect) {
    // The log is dropped while editing: the pane is a text editor now, and
    // shrinking it to make room for old news moves every line under the cursor.
    let log_height = if app.log.is_empty() || app.viewer == ViewerMode::Edit {
        0
    } else {
        let wanted = u16::try_from(app.log.len()).unwrap_or(LOG_MAX) + 2;
        let ceiling = if area.height < SHORT_HEIGHT {
            (area.height / 2).max(3)
        } else {
            LOG_MAX
        };
        wanted.min(ceiling)
    };

    let rows = Layout::vertical([Constraint::Min(4), Constraint::Length(log_height)]).split(area);

    draw_viewer(frame, app, rows[0]);
    if log_height > 0 {
        app.regions.log = rows[1];
        draw_log(frame, app, rows[1]);
    }
}

fn draw_viewer(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let is_focused = focused(app, Pane::Viewer) || app.mode == Mode::Edit;

    // The title names what the pane is actually showing, which in the delta
    // and task modes is the change rather than the document under the cursor.
    let fallback = || match app.selected_change() {
        Some(change) => change.id.clone(),
        None => app.tab.title().to_lowercase(),
    };
    let title = match app.viewer {
        ViewerMode::Diff | ViewerMode::Tasks => fallback(),
        ViewerMode::Edit => app
            .buffer
            .as_ref()
            .map_or_else(fallback, |buffer| buffer.file_name()),
        ViewerMode::Document => app.open_path().unwrap_or_else(fallback),
    };
    let dirty = app.buffer.as_ref().is_some_and(|buffer| buffer.dirty);
    let title = if dirty && app.viewer == ViewerMode::Edit {
        format!("● {title}")
    } else {
        title
    };

    let mut block = panel_block(
        theme,
        &util::truncate(&title, usize::from(area.width).saturating_sub(24)),
        is_focused,
    );

    if let Some((badge, colour)) = viewer_badge(app) {
        block = block.title_top(
            Line::styled(
                format!(" {badge} "),
                Style::new().fg(colour).add_modifier(Modifier::BOLD),
            )
            .right_aligned(),
        );
    }
    if let Some(footer) = metadata_footer(app) {
        block = block.title_bottom(footer.right_aligned());
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    app.regions.viewer = inner;
    app.viewer_height = inner.height.max(1);
    app.viewer_width = inner.width.max(1);
    app.sync_viewer(inner.width);
    app.clamp_scroll();

    if app.viewer == ViewerMode::Edit {
        draw_buffer(frame, app, inner);
        draw_scrollbar(
            frame,
            theme,
            area,
            app.viewer_rows(),
            usize::from(app.scroll),
            1,
            Track::Offset,
        );
        return;
    }

    if app.viewer_is_empty() {
        // A directory row has no document behind it, so the pane reports on
        // the change itself rather than going blank.
        if app.viewer == ViewerMode::Document
            && let Some(change) = app.selected_change()
        {
            frame.render_widget(Paragraph::new(overview(app, change, inner.width)), inner);
            return;
        }

        let message = match app.viewer {
            ViewerMode::Diff => t!("tui.empty.diff"),
            ViewerMode::Tasks => t!("tui.empty.tasks"),
            _ => t!("tui.empty.preview"),
        };
        frame.render_widget(empty_paragraph(theme, &message), inner);
        return;
    }

    if app.viewer == ViewerMode::Tasks {
        draw_tasks(frame, app, inner);
    } else {
        let lines = app.viewer_window(inner.height);
        frame.render_widget(Paragraph::new(lines), inner);
    }

    draw_scrollbar(
        frame,
        theme,
        area,
        app.viewer_rows(),
        usize::from(app.scroll),
        1,
        Track::Offset,
    );
}

/// Draws the text buffer with a gutter and a block cursor.
fn draw_buffer(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let Some(buffer) = &app.buffer else {
        frame.render_widget(empty_paragraph(theme, &t!("tui.empty.preview")), area);
        return;
    };

    // Validation reports failures by line, so the gutter is not decoration.
    let gutter = u16::try_from(buffer.len().to_string().chars().count() + 1).unwrap_or(4);
    let gutter = gutter.min(area.width.saturating_sub(4));
    let columns = Layout::horizontal([Constraint::Length(gutter), Constraint::Min(1)]).split(area);
    let text_area = columns[1];

    let width = usize::from(text_area.width);
    let cursor_visible = app.mode == Mode::Edit;

    let mut numbers: Vec<Line<'static>> = Vec::new();
    let mut lines: Vec<Line<'static>> = Vec::new();

    for row in 0..text_area.height {
        let index = usize::from(app.scroll) + usize::from(row);
        let current = index == buffer.row;

        if index >= buffer.len() {
            numbers.push(Line::default());
            lines.push(Line::from(Span::styled("~", Style::new().fg(theme.border))));
            continue;
        }

        let style = if current {
            Style::new().fg(theme.accent)
        } else {
            theme.dim()
        };
        numbers.push(Line::from(Span::styled(format!("{} ", index + 1), style)).right_aligned());
        lines.push(buffer_line(
            theme,
            buffer,
            index,
            app.edit_offset,
            width,
            cursor_visible && current,
        ));
    }

    frame.render_widget(Paragraph::new(numbers), columns[0]);
    frame.render_widget(Paragraph::new(lines), text_area);

    // The click handler maps a column back to a character, so it has to know
    // how much of the pane the gutter took.
    app.regions.viewer = text_area;
    app.viewer_width = text_area.width.max(1);
}

fn buffer_line(
    theme: Theme,
    buffer: &Buffer,
    index: usize,
    offset: usize,
    width: usize,
    cursor: bool,
) -> Line<'static> {
    let characters: Vec<char> = buffer.line(index).chars().collect();
    let visible: Vec<char> = characters
        .iter()
        .skip(offset)
        .take(width)
        .copied()
        .collect();

    if !cursor {
        return Line::from(Span::styled(
            visible.into_iter().collect::<String>(),
            Style::new().fg(theme.text),
        ));
    }

    let column = buffer.column.saturating_sub(offset);
    let before: String = visible.iter().take(column).collect();
    let at = visible.get(column).copied().unwrap_or(' ');
    let after: String = visible.iter().skip(column + 1).collect();

    Line::from(vec![
        Span::styled(before, Style::new().fg(theme.text)),
        Span::styled(
            at.to_string(),
            Style::new().fg(theme.background).bg(theme.accent),
        ),
        Span::styled(after, Style::new().fg(theme.text)),
    ])
}

/// One row per task, so a click lands on exactly the task under the pointer.
fn draw_tasks(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let width = usize::from(area.width);

    let lines: Vec<Line<'static>> = app
        .tasks
        .iter()
        .enumerate()
        .skip(usize::from(app.scroll))
        .take(usize::from(area.height))
        .map(|(index, task)| task_line(theme, task, index == app.task_cursor, width))
        .collect();

    frame.render_widget(Paragraph::new(lines), area);
}

fn task_line(theme: Theme, task: &TaskLine, selected: bool, width: usize) -> Line<'static> {
    let box_span = if task.done {
        Span::styled("[x] ", Style::new().fg(theme.success))
    } else {
        Span::styled("[ ] ", Style::new().fg(theme.text_dim))
    };

    // The section says more than the file name once a change has several task
    // groups, and every proposal keeps its tasks in one file anyway.
    let context = task.section.clone().unwrap_or_else(|| task.file_name());
    let context = util::truncate(&context, width / 3);
    let room = width.saturating_sub(context.chars().count() + 8).max(8);

    let label_style = if task.done {
        Style::new().fg(theme.text_dim)
    } else if selected {
        Style::new()
            .fg(theme.text_strong)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.text)
    };

    let line = Line::from(vec![
        Span::styled(
            if selected { "▌" } else { " " },
            Style::new().fg(theme.border_focus),
        ),
        box_span,
        Span::styled(
            util::pad_right(&util::truncate(&task.label, room), room + 1),
            label_style,
        ),
        Span::styled(context, theme.dim()),
    ]);

    if selected {
        line.style(Style::new().bg(theme.selection))
    } else {
        line
    }
}

fn draw_log(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;

    let block = panel_block(theme, &t!("tui.panel.activity"), false).title_bottom(
        Line::styled(format!(" {} ", t!("tui.panel.clear_log")), theme.dim()).right_aligned(),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let height = usize::from(inner.height);
    let start = app.log.len().saturating_sub(height);
    let width = usize::from(inner.width);

    let lines: Vec<Line<'static>> = app.log[start..]
        .iter()
        .map(|entry| {
            let colour = match entry.tone {
                Tone::Muted => theme.text_dim,
                Tone::Accent => theme.accent,
                Tone::Success => theme.success,
                Tone::Warning => theme.warning,
                Tone::Danger => theme.danger,
            };
            Line::from(Span::styled(
                util::truncate(&entry.text, width),
                Style::new().fg(colour),
            ))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Summary of a change, shown when no single document is open.
fn overview(app: &App, change: &ChangeProposal, width: u16) -> Vec<Line<'static>> {
    let theme = app.theme;
    let column = usize::from(width) / 2;

    let labels: Vec<String> = [
        "tui.field.id",
        "tui.field.documents",
        "tui.field.shape",
        "tui.field.updated",
        "tui.field.tasks",
        "tui.field.deltas",
        "tui.field.lines",
        "tui.field.headings",
    ]
    .iter()
    .map(|key| t!(*key).to_string())
    .collect();
    let label_width = label_column(&labels, column);

    let shape = if change.archived {
        t!("list.archived").to_string()
    } else if change.single_file {
        t!("tui.shape.file").to_string()
    } else {
        t!("tui.shape.directory").to_string()
    };

    let tasks = if change.tasks_total > 0 {
        format!("{} / {}", change.tasks_done, change.tasks_total)
    } else {
        t!("common.none").to_string()
    };
    let deltas: usize = app
        .deltas
        .iter()
        .map(|capability| capability.requirements.len())
        .sum();
    let lines: usize = change.documents.iter().map(|document| document.lines).sum();
    let headings: usize = change
        .documents
        .iter()
        .map(|document| document.headings)
        .sum();

    let row = |left: (&str, String, ratatui::style::Color),
               right: (&str, String, ratatui::style::Color)| {
        Line::from(
            [
                field(theme, &t!(left.0), &left.1, left.2, column, label_width),
                field(theme, &t!(right.0), &right.1, right.2, column, label_width),
            ]
            .concat(),
        )
    };

    vec![
        Line::from(Span::styled(
            util::truncate(&change.title, usize::from(width)),
            theme.strong(),
        )),
        Line::default(),
        row(
            ("tui.field.id", change.id.clone(), theme.primary),
            (
                "tui.field.documents",
                change.documents.len().to_string(),
                theme.info,
            ),
        ),
        row(
            ("tui.field.shape", shape, theme.text),
            (
                "tui.field.updated",
                util::format_timestamp(change.modified),
                theme.text,
            ),
        ),
        row(
            ("tui.field.tasks", tasks, theme.success),
            ("tui.field.deltas", deltas.to_string(), theme.accent),
        ),
        row(
            ("tui.field.lines", lines.to_string(), theme.text),
            ("tui.field.headings", headings.to_string(), theme.text),
        ),
    ]
}

/// Compact metadata strip carried by the viewer's bottom border.
///
/// The delta, task and edit modes describe something other than the parsed
/// document, and their own content already carries the counts, so they get no
/// strip at all.
fn metadata_footer(app: &App) -> Option<Line<'static>> {
    let theme = app.theme;
    if app.viewer != ViewerMode::Document {
        return None;
    }
    let document = app.open_document()?;

    let kind = match document.kind {
        DocKind::Markdown => "MD",
        DocKind::Yaml => "YAML",
    };

    let mut spans = vec![
        Span::styled(
            format!(" {} {} ", document.status.glyph(), document.status.label()),
            Style::new().fg(theme.status(document.status)),
        ),
        Span::styled("· ", theme.dim()),
        Span::styled(
            format!("{} ", util::format_size(document.size)),
            Style::new().fg(theme.info),
        ),
        Span::styled("· ", theme.dim()),
        Span::styled(
            format!(
                "{} {} ",
                document.requirements,
                t!("tui.field.requirements")
            ),
            Style::new().fg(theme.accent),
        ),
        Span::styled("· ", theme.dim()),
        Span::styled(format!("{} L ", document.lines), theme.dim()),
        Span::styled("· ", theme.dim()),
        Span::styled(
            format!("{} ", util::format_relative(document.modified)),
            theme.dim(),
        ),
        Span::styled(format!("· {kind} "), theme.dim()),
    ];

    if document.truncated {
        spans.push(Span::styled(
            format!("· ⚠ {} ", t!("tui.warn.truncated")),
            Style::new().fg(theme.warning),
        ));
    }

    Some(Line::from(spans))
}
