//! Rendering of the interactive dashboard.
//!
//! The frame is fixed: a header, the active view, the command line and the key
//! hints. [`editor`] is the working surface — change list, artifact tree,
//! viewer and activity log — and [`dashboard`] is the same workspace seen as
//! numbers.

pub mod activity;
pub mod dashboard;
pub mod editor;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    Wrap,
};
use rust_i18n::t;

use crate::domain::util;
use crate::tui::app::{App, Mode, Pane, Setting, View, ViewerMode};
use crate::tui::command;
use crate::tui::theme::Theme;

/// Below this width the panes collapse into whichever one holds the focus.
pub const NARROW_WIDTH: u16 = 84;

/// Below this height the secondary sections are dropped.
pub const SHORT_HEIGHT: u16 = 22;

/// Narrowest the sidebar is drawn, whoever asked for the width.
pub const SIDEBAR_MIN: u16 = 28;

/// Widest the sidebar sizes itself to on its own.
///
/// A ceiling for the automatic width only, so the sidebar does not swallow a
/// wide terminal before anyone has said it should. A width the user asked for
/// is bounded by [`VIEWER_RESERVE`] instead: stopping the divider a third of
/// the way across reads as a broken drag rather than as a limit.
pub const SIDEBAR_MAX: u16 = 42;

/// Columns the viewer keeps however wide the sidebar is asked to be.
pub const VIEWER_RESERVE: u16 = 40;

/// Gear that opens the preferences panel.
const SETTINGS_GLYPH: &str = "⚙";

/// The glyph plus a cell of air on each side, and one more before it: while the
/// panel is open the button is filled, and without that gap its fill runs
/// straight into the counts and the two read as one block.
const SETTINGS_BUTTON_WIDTH: u16 = 4;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let theme = app.theme;

    frame.render_widget(Block::new().style(theme.base()), area);

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    draw_header(frame, app, rows[0]);

    match app.view {
        View::Editor => editor::draw(frame, app, rows[1]),
        View::Dashboard => dashboard::draw(frame, app, rows[1]),
        View::Activity => activity::draw(frame, app, rows[1]),
    }

    app.regions.command = rows[2];
    draw_command_line(frame, app, rows[2]);
    draw_footer(frame, app, rows[3]);

    match app.mode {
        Mode::Help => draw_help(frame, app, area),
        Mode::Confirm => draw_confirm(frame, app, area),
        Mode::Settings => draw_settings(frame, app, area),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Chrome
// ---------------------------------------------------------------------------

fn draw_header(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.border))
        .style(theme.panel())
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let counts = t!(
        "tui.header.counts",
        active = app.workspace.active_changes(),
        archived = app.workspace.archived_changes()
    )
    .to_string();
    let right_width = u16::try_from(counts.chars().count() + 1).unwrap_or(0);

    // The gear is a button, so it is given the width of one: a glyph with a
    // cell of air on each side, which is what makes it look pressable and what
    // makes it big enough to hit with a mouse.
    let columns = Layout::horizontal([
        Constraint::Min(10),
        Constraint::Length(right_width),
        Constraint::Length(SETTINGS_BUTTON_WIDTH),
    ])
    .split(inner);

    let project = app.workspace.project_name();
    let warning =
        (!app.workspace.has_config_file).then(|| format!("⚠ {}", t!("tui.warn.no_config")));

    let fixed = 2
        + "openspectation".chars().count()
        + 5
        + project.chars().count()
        + 5
        + warning.as_ref().map_or(0, |text| text.chars().count() + 5);
    let room = usize::from(columns[0].width).saturating_sub(fixed).max(8);

    let mut title = vec![
        Span::styled("◆ ", theme.accented()),
        Span::styled("openspectation", theme.strong()),
        Span::styled("  ·  ", theme.dim()),
        Span::styled(project, theme.primary_style()),
        Span::styled("  ·  ", theme.dim()),
        Span::styled(
            util::truncate(&app.workspace.display_root(), room),
            theme.dim(),
        ),
    ];

    if let Some(warning) = warning {
        title.push(Span::styled("  ·  ", theme.dim()));
        title.push(Span::styled(warning, Style::new().fg(theme.warning)));
    }

    frame.render_widget(Paragraph::new(Line::from(title)), columns[0]);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            counts,
            Style::new().fg(theme.accent),
        )))
        .alignment(Alignment::Right),
        columns[1],
    );

    app.regions.settings = columns[2];
    let pressed = app.mode == Mode::Settings;
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {SETTINGS_GLYPH} "),
            if pressed {
                Style::new()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                // No fill at rest: on the light palettes the block read as a
                // pale slab wider than the glyph it framed.
                Style::new().fg(theme.accent)
            },
        )))
        .alignment(Alignment::Right),
        columns[2],
    );
}

/// The `:` line, which doubles as the search box.
fn draw_command_line(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;

    let line = match app.mode {
        Mode::Command => Line::from(vec![
            Span::styled(
                " : ",
                Style::new()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(app.draft.clone(), Style::new().fg(theme.text_strong)),
            Span::styled("▌", Style::new().fg(theme.accent)),
        ]),
        Mode::Search => {
            let matches = app.visible_len();
            let mut spans = vec![
                Span::styled(
                    format!(" {} ", t!("tui.search.prompt")),
                    Style::new()
                        .fg(theme.background)
                        .bg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(app.draft.clone(), Style::new().fg(theme.text_strong)),
                Span::styled("▌", Style::new().fg(theme.accent)),
                Span::styled(
                    format!("   {}", t!("tui.search.matches", count = matches)),
                    Style::new().fg(if matches == 0 {
                        theme.warning
                    } else {
                        theme.success
                    }),
                ),
            ];
            if area.width >= NARROW_WIDTH {
                spans.push(Span::styled(
                    format!("   {}", app.filter.describe()),
                    theme.dim(),
                ));
            }
            Line::from(spans)
        }
        _ => {
            let mut spans = vec![
                Span::styled(" : ", Style::new().fg(theme.accent)),
                Span::styled(t!("tui.command.hint").to_string(), theme.dim()),
            ];
            if app.filter.is_active() {
                spans.push(Span::styled("   ", theme.dim()));
                spans.push(Span::styled(
                    format!(" {} ", app.filter.describe()),
                    Style::new().fg(theme.background).bg(theme.primary),
                ));
                spans.push(Span::styled(
                    format!(" {}", t!("tui.search.matches", count = app.visible_len())),
                    theme.dim(),
                ));
            }
            Line::from(spans)
        }
    };

    frame.render_widget(Paragraph::new(line), area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;

    if let Some(toast) = &app.toast {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ● ", Style::new().fg(theme.success)),
                Span::styled(toast.message.clone(), Style::new().fg(theme.text_strong)),
            ])),
            area,
        );
        return;
    }

    let hints: Vec<(&str, String)> = if app.mode == Mode::Edit {
        // Editing is modal, so the way out has to be on screen.
        vec![
            ("Ctrl+S", t!("tui.key.save").to_string()),
            ("Esc", t!("tui.key.preview").to_string()),
            ("↑↓←→", t!("tui.key.move").to_string()),
        ]
    } else if app.view == View::Activity {
        vec![
            ("↑↓", t!("tui.key.navigate").to_string()),
            ("H", t!("tui.key.editor").to_string()),
            ("Esc", t!("tui.key.back").to_string()),
            ("q", t!("tui.key.quit").to_string()),
        ]
    } else if app.view == View::Dashboard {
        vec![
            ("g", t!("tui.key.editor").to_string()),
            ("Esc", t!("tui.key.back").to_string()),
            ("L", t!("tui.key.language").to_string()),
            ("q", t!("tui.key.quit").to_string()),
        ]
    } else if area.width < NARROW_WIDTH + 26 {
        vec![
            ("↑↓", t!("tui.key.navigate").to_string()),
            ("e", t!("tui.key.edit").to_string()),
            ("d", t!("tui.key.diff").to_string()),
            ("v", t!("tui.key.validate").to_string()),
            (":", t!("tui.key.command").to_string()),
            ("?", t!("tui.key.help").to_string()),
        ]
    } else {
        vec![
            ("j/k", t!("tui.key.navigate").to_string()),
            ("↵", t!("tui.key.open").to_string()),
            ("e", t!("tui.key.edit").to_string()),
            ("d", t!("tui.key.diff").to_string()),
            ("v", t!("tui.key.validate").to_string()),
            ("space", t!("tui.key.toggle").to_string()),
            ("a", t!("tui.key.archive").to_string()),
            ("1-4", t!("tui.key.tabs").to_string()),
            ("/", t!("tui.key.search").to_string()),
            (":", t!("tui.key.command").to_string()),
            (",", t!("tui.key.settings").to_string()),
            ("?", t!("tui.key.help").to_string()),
        ]
    };

    let mut spans = vec![Span::raw(" ")];
    let mut used = 1usize;
    let room = usize::from(area.width);

    for (key, label) in &hints {
        // Hints are dropped whole rather than clipped: half a word reads as a
        // rendering bug, a shorter list reads as a shorter list.
        let separator = usize::from(!spans.is_empty() && used > 1) * 5;
        let cost = separator + key.chars().count() + 1 + label.chars().count();
        if used + cost > room {
            break;
        }
        used += cost;

        if separator > 0 {
            spans.push(Span::styled("  ·  ", Style::new().fg(theme.border)));
        }
        spans.push(Span::styled(
            (*key).to_string(),
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(label.clone(), theme.dim()));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---------------------------------------------------------------------------
// Overlays
// ---------------------------------------------------------------------------

fn draw_help(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;

    let bindings: [(&str, &str); 22] = [
        ("j / k · ↑ ↓", "tui.help.navigate"),
        ("h / l · ← →", "tui.help.panes"),
        ("Tab", "tui.help.cycle"),
        ("Enter", "tui.help.open"),
        ("1 … 4 · s", "tui.help.tabs"),
        ("e", "tui.help.edit"),
        ("Ctrl+S", "tui.help.save"),
        ("Esc", "tui.help.preview"),
        ("E", "tui.help.editor"),
        ("d", "tui.help.diff"),
        ("t", "tui.help.tasks"),
        ("m", "tui.help.document"),
        ("space", "tui.help.toggle"),
        ("v", "tui.help.validate"),
        ("a", "tui.help.archive"),
        ("g", "tui.help.dashboard"),
        ("H", "tui.help.activity"),
        ("/ · :", "tui.help.search_command"),
        ("r", "tui.help.reload"),
        ("< · >", "tui.help.sidebar"),
        (", · F2", "tui.help.settings"),
        ("L · q", "tui.help.language_quit"),
    ];

    let key_width = bindings
        .iter()
        .map(|(key, _)| key.chars().count())
        .max()
        .unwrap_or(8)
        + 2;

    let mut lines: Vec<Line<'static>> = bindings
        .iter()
        .map(|(key, label)| {
            Line::from(vec![
                Span::styled(
                    util::pad_right(key, key_width),
                    Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(t!(*label).to_string(), Style::new().fg(theme.text)),
            ])
        })
        .collect();

    // Two border rows and one padding row on each side.
    const CHROME: u16 = 4;
    let available = usize::from(area.height.saturating_sub(CHROME));

    // The catalogue is all-or-nothing: a section cut off after three entries
    // reads as a bug, so it is either listed whole or left out.
    if available >= lines.len() + 2 + command::CATALOGUE.len() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            t!("tui.help.commands").to_string(),
            theme.strong(),
        )));
        for entry in command::CATALOGUE {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!(":{entry}"), Style::new().fg(theme.info)),
            ]));
        }
    }

    // The overlay is sized to what it holds. A fixed share of the screen leaves
    // a bordered box half full of nothing whenever the catalogue is dropped.
    let widest = lines.iter().map(Line::width).max().unwrap_or(40);
    let width = u16::try_from(widest + 6)
        .unwrap_or(u16::MAX)
        .clamp(40.min(area.width), area.width.max(1));
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(CHROME)
        .min(area.height);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, popup);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.border_focus))
        .style(theme.panel())
        .padding(Padding::symmetric(2, 1))
        .title_top(Line::styled(
            format!(" {} ", t!("tui.help.title")),
            theme.title_style(true),
        ))
        .title_bottom(
            Line::styled(format!(" {} ", t!("tui.help.dismiss")), theme.dim()).right_aligned(),
        )
        // Validation and archiving are delegated, so whether the upstream CLI
        // is here decides whether two of these keys do anything at all.
        .title_bottom(Line::from(match &app.cli_version {
            Some(version) => Span::styled(
                format!(" openspec {version} "),
                Style::new().fg(theme.success),
            ),
            None => Span::styled(
                format!(" {} ", t!("error.cli_missing")),
                Style::new().fg(theme.warning),
            ),
        }));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let Some(confirm) = &app.confirm else {
        return;
    };

    let lines = vec![
        Line::from(Span::styled(confirm.question.clone(), theme.strong())),
        Line::default(),
        Line::from(Span::styled(confirm.detail.clone(), theme.dim())),
        Line::default(),
        Line::from(vec![
            Span::styled(
                format!(" {} ", confirm.accept),
                Style::new()
                    .fg(theme.background)
                    .bg(theme.danger)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled(
                format!(" {} ", t!("tui.confirm.reject")),
                Style::new().fg(theme.text).bg(theme.surface_alt),
            ),
        ]),
    ];

    // Sized to its contents: a fixed share of the screen leaves a bordered box
    // half full of nothing, which reads as a rendering fault mid-question.
    // A long explanation wraps rather than stretching the box across the
    // screen, which would read as a new screen instead of a question.
    let ceiling = area.width.clamp(1, 76);
    let width = u16::try_from(lines.iter().map(Line::width).max().unwrap_or(40) + 6)
        .unwrap_or(u16::MAX)
        .clamp(ceiling.min(30), ceiling);
    let wrapped = usize::from(width.saturating_sub(6)).max(1);
    let extra: usize = [&confirm.question, &confirm.detail]
        .iter()
        .map(|text| text.chars().count().saturating_sub(1) / wrapped)
        .sum();
    let height = u16::try_from(lines.len() + extra + 4)
        .unwrap_or(u16::MAX)
        .min(area.height);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.warning))
        .style(theme.panel())
        .padding(Padding::symmetric(2, 1))
        .title_top(Line::styled(
            format!(" {} ", t!("tui.confirm.title")),
            Style::new().fg(theme.warning).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

/// The preferences panel.
///
/// Two rows, each a label, the current value and the range it moves through.
/// There is no save button: a change applies and is written the moment it is
/// made, so the panel has no state of its own to get out of step with the file.
fn draw_settings(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;

    const LABEL_WIDTH: usize = 12;
    const VALUE_WIDTH: usize = 14;

    let rows: Vec<(Setting, String, String)> = Setting::ORDER
        .iter()
        .map(|setting| (*setting, setting.label(), app.setting_value(*setting)))
        .collect();

    // Wide enough for the longest hint in either catalogue: a sentence clipped
    // mid-word reads as a rendering fault, and these sentences are the only
    // thing telling a first-time reader what the row does.
    let width = area.width.clamp(1, 68);
    let height = u16::try_from(rows.len() * 2 + 6)
        .unwrap_or(u16::MAX)
        .min(area.height);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.accent))
        .style(theme.panel())
        .padding(Padding::symmetric(2, 1))
        .title_top(Line::styled(
            format!(" {SETTINGS_GLYPH} {} ", t!("tui.settings.title")),
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    for (index, (setting, label, value)) in rows.iter().enumerate() {
        // Two rows apiece: the control, and the sentence saying what it does.
        let y = inner.y + u16::try_from(index * 2).unwrap_or(0);
        if y >= inner.y + inner.height {
            break;
        }

        let selected = index == app.setting_cursor;
        let row = Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: 1,
        };
        app.regions.setting_rows[index] = row;

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    util::pad_right(label, LABEL_WIDTH),
                    if selected {
                        theme.strong()
                    } else {
                        Style::new().fg(theme.text)
                    },
                ),
                Span::styled(
                    format!(" {} ", util::pad_right(value, VALUE_WIDTH - 2)),
                    if selected {
                        Style::new()
                            .fg(theme.background)
                            .bg(theme.accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::new().fg(theme.text_strong).bg(theme.surface_alt)
                    },
                ),
                Span::styled(
                    if selected { "  ← →" } else { "" },
                    Style::new().fg(theme.accent),
                ),
            ])),
            row,
        );

        let hint = Rect {
            y: y + 1,
            height: 1,
            ..row
        };
        if hint.y < inner.y + inner.height {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(setting.hint(), theme.dim()))),
                hint,
            );
        }
    }

    let footer = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            t!("tui.settings.footer").to_string(),
            theme.dim(),
        ))),
        footer,
    );
}

// ---------------------------------------------------------------------------
// Shared widgets
// ---------------------------------------------------------------------------

/// A bordered pane. An empty title leaves the top border clean, for a pane that
/// names itself through its contents.
pub fn panel_block(theme: Theme, title: &str, focused: bool) -> Block<'static> {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.border_style(focused))
        .style(theme.panel())
        .padding(Padding::horizontal(1));

    if title.is_empty() {
        return block;
    }
    block.title_top(Line::styled(
        format!(" {title} "),
        theme.title_style(focused),
    ))
}

pub fn empty_paragraph(theme: Theme, message: &str) -> Paragraph<'static> {
    let mut lines = vec![Line::default()];
    lines.extend(
        message
            .lines()
            .map(|line| Line::from(Span::styled(line.to_string(), theme.dim())).centered()),
    );
    Paragraph::new(lines).wrap(Wrap { trim: true })
}

/// Width reserved for the labels of a key/value grid, including the gap that
/// separates them from their values.
pub fn label_column(labels: &[String], column: usize) -> usize {
    labels
        .iter()
        .map(|label| label.chars().count())
        .max()
        .unwrap_or(4)
        .saturating_add(2)
        .clamp(4, column.saturating_sub(4).max(4))
}

pub fn field(
    theme: Theme,
    label: &str,
    value: &str,
    color: ratatui::style::Color,
    column: usize,
    label_width: usize,
) -> Vec<Span<'static>> {
    let value_width = column.saturating_sub(label_width).max(4);

    vec![
        Span::styled(
            util::pad_right(
                &util::truncate(label, label_width.saturating_sub(1)),
                label_width,
            ),
            theme.dim(),
        ),
        Span::styled(
            util::pad_right(
                &util::truncate(value, value_width.saturating_sub(2).max(1)),
                value_width,
            ),
            Style::new().fg(color),
        ),
    ]
}

/// What the `position` handed to [`draw_scrollbar`] counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Track {
    /// The index of the highlighted entry, which ranges over every entry.
    Selection,
    /// The first visible row, which stops one viewport short of the end.
    Offset,
}

/// Draws a scrollbar, but only when the entries cannot all fit.
///
/// `rows_per_item` is how tall a single entry renders, which the change list
/// needs because each entry spans a title line and a progress line.
pub fn draw_scrollbar(
    frame: &mut Frame,
    theme: Theme,
    area: Rect,
    length: usize,
    position: usize,
    rows_per_item: usize,
    track_kind: Track,
) {
    let track = usize::from(area.height.saturating_sub(2));
    let visible = track / rows_per_item.max(1);
    if length <= visible || visible == 0 {
        return;
    }

    // ratatui counts scroll *positions*, not content rows, and pins the thumb
    // to the end of the track only when `position` reaches the last one. A
    // scroll offset stops a viewport short of the total, so handing it the row
    // count leaves the thumb floating above the bottom at the end of a
    // document — the scrollbar reporting there is more to read when there is not.
    let (content, position) = match track_kind {
        Track::Selection => (length, position),
        Track::Offset => {
            let last = length - visible;
            (last + 1, position.min(last))
        }
    };

    let mut state = ScrollbarState::new(content)
        .position(position)
        .viewport_content_length(visible);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(theme.border_focus))
            .track_style(Style::new().fg(theme.border)),
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        }),
        &mut state,
    );
}

/// Rectangle centered inside `area`, sized as a percentage of it.
pub fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

/// Whether a pane currently owns the keyboard.
pub fn focused(app: &App, pane: Pane) -> bool {
    app.view == View::Editor && app.mode == Mode::Normal && app.pane == pane
}

/// Badge shown on the right of the viewer title.
pub fn viewer_badge(app: &App) -> Option<(String, ratatui::style::Color)> {
    let theme = app.theme;
    match app.viewer {
        ViewerMode::Diff => Some((app.viewer.label(), theme.accent)),
        ViewerMode::Tasks => Some((app.viewer.label(), theme.success)),
        ViewerMode::Edit => Some((app.viewer.label(), theme.warning)),
        ViewerMode::Document => app
            .deltas
            .iter()
            .find(|capability| {
                app.open_document()
                    .is_some_and(|document| document.path == capability.path)
            })
            .and_then(|capability| capability.headline())
            .map(|kind| {
                let colour = match kind {
                    crate::domain::delta::DeltaKind::Added => theme.success,
                    crate::domain::delta::DeltaKind::Modified => theme.warning,
                    crate::domain::delta::DeltaKind::Removed => theme.danger,
                    crate::domain::delta::DeltaKind::Renamed => theme.info,
                };
                (kind.tag().to_string(), colour)
            }),
    }
}

#[cfg(test)]
mod tests;
