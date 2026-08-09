//! The workspace seen as numbers: totals, distribution and recent activity.
//!
//! Reached with `g` or `:dashboard`, and left with the same key or `Esc`. It
//! answers "where does this workspace stand", which the working surface
//! deliberately does not try to.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Gauge, Padding, Paragraph};
use rust_i18n::t;

use crate::domain::spec::SpecStatus;
use crate::domain::util;
use crate::domain::workspace::Stats;
use crate::tui::app::App;
use crate::tui::theme::Theme;
use crate::tui::ui::{SHORT_HEIGHT, empty_paragraph, panel_block};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let stats = app.workspace.stats();

    let compact = area.height < SHORT_HEIGHT;
    let rows = if compact {
        Layout::vertical([Constraint::Length(6), Constraint::Min(0)]).split(area)
    } else {
        Layout::vertical([
            Constraint::Length(6),
            Constraint::Length(9),
            Constraint::Min(3),
        ])
        .split(area)
    };

    let cards = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ])
    .split(rows[0]);

    let tasks_caption = if stats.tasks_total > 0 {
        format!("{} / {}", stats.tasks_done, stats.tasks_total)
    } else {
        t!("common.none").to_string()
    };

    draw_card(
        frame,
        theme,
        cards[0],
        &t!("tui.stat.specs"),
        &stats.spec_count.to_string(),
        &t!("tui.field.requirements").to_lowercase().to_string(),
        &stats.requirements.to_string(),
        theme.primary,
    );
    draw_card(
        frame,
        theme,
        cards[1],
        &t!("tui.stat.changes"),
        &stats.change_count.to_string(),
        &t!("tui.field.documents").to_lowercase().to_string(),
        &app.workspace
            .changes
            .iter()
            .map(|change| change.documents.len())
            .sum::<usize>()
            .to_string(),
        theme.accent,
    );
    draw_card(
        frame,
        theme,
        cards[2],
        &t!("tui.stat.tasks"),
        &tasks_caption,
        &t!("tui.field.words").to_lowercase().to_string(),
        &stats.words.to_string(),
        theme.success,
    );
    draw_card(
        frame,
        theme,
        cards[3],
        &t!("tui.stat.size"),
        &util::format_size(stats.total_bytes),
        &t!("tui.field.updated").to_lowercase().to_string(),
        &util::format_relative(
            app.workspace
                .recently_updated(1)
                .first()
                .and_then(|document| document.modified),
        ),
        theme.info,
    );

    if compact {
        draw_distribution(frame, app, &stats, rows[1]);
        return;
    }

    let middle =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);
    draw_distribution(frame, app, &stats, middle[0]);
    draw_progress(frame, app, &stats, middle[1]);
    draw_recent(frame, app, rows[2]);
}

#[allow(clippy::too_many_arguments)]
fn draw_card(
    frame: &mut Frame,
    theme: Theme,
    area: Rect,
    label: &str,
    value: &str,
    caption_label: &str,
    caption_value: &str,
    color: ratatui::style::Color,
) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.border))
        .style(theme.panel())
        .padding(Padding::horizontal(2))
        .title_top(Line::styled(
            format!(" {label} "),
            Style::new().fg(theme.text_dim).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content = vec![
        Line::default(),
        Line::from(Span::styled(
            value.to_string(),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(format!("{caption_label} "), theme.dim()),
            Span::styled(caption_value.to_string(), Style::new().fg(theme.text)),
        ]),
    ];

    frame.render_widget(Paragraph::new(content), inner);
}

fn draw_distribution(frame: &mut Frame, app: &App, stats: &Stats, area: Rect) {
    let theme = app.theme;

    let block = panel_block(theme, &t!("tui.panel.distribution"), false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if stats.spec_count == 0 {
        frame.render_widget(empty_paragraph(theme, &t!("tui.empty.specs")), inner);
        return;
    }

    let label_width = SpecStatus::ALL
        .iter()
        .map(|status| status.label().chars().count())
        .max()
        .unwrap_or(10);
    let bar_width = usize::from(inner.width)
        .saturating_sub(label_width + 10)
        .clamp(4, 40);
    // Scaling against the total makes each bar readable as a share of the
    // corpus; scaling against the peak would fill every bar on a flat spread.
    let total = stats.spec_count.max(1);

    let lines: Vec<Line<'static>> = SpecStatus::ALL
        .iter()
        .filter(|status| stats.count(**status) > 0)
        .map(|status| {
            let count = stats.count(*status);
            let share = util::bar(count as f64 / total as f64, bar_width);
            Line::from(vec![
                Span::styled(
                    format!("{} ", status.glyph()),
                    Style::new().fg(theme.status(*status)),
                ),
                Span::styled(
                    util::pad_right(&status.label(), label_width + 1),
                    Style::new().fg(theme.text),
                ),
                Span::styled(share.filled, Style::new().fg(theme.status(*status))),
                Span::styled(share.track, Style::new().fg(theme.border)),
                Span::styled(format!(" {count}"), theme.dim()),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_progress(frame: &mut Frame, app: &App, stats: &Stats, area: Rect) {
    let theme = app.theme;

    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let ratio = stats.completion();
    let gauge_block = panel_block(theme, &t!("tui.stat.completion"), false);
    frame.render_widget(
        Gauge::default()
            .block(gauge_block)
            .ratio(ratio.clamp(0.0, 1.0))
            .gauge_style(Style::new().fg(theme.progress(ratio)).bg(theme.surface_alt))
            .label(Span::styled(
                format!("{:.0}%", ratio * 100.0),
                Style::new()
                    .fg(theme.text_strong)
                    .add_modifier(Modifier::BOLD),
            )),
        rows[0],
    );

    let block = panel_block(theme, &t!("tui.panel.progress"), false);
    let inner = block.inner(rows[1]);
    frame.render_widget(block, rows[1]);

    if app.workspace.changes.is_empty() {
        frame.render_widget(empty_paragraph(theme, &t!("tui.empty.changes")), inner);
        return;
    }

    let name_width = usize::from(inner.width).saturating_sub(18).clamp(6, 30);
    let bar_width = usize::from(inner.width)
        .saturating_sub(name_width + 8)
        .clamp(4, 24);

    let lines: Vec<Line<'static>> = app
        .workspace
        .changes
        .iter()
        .take(usize::from(inner.height))
        .map(|change| {
            let progress = change.progress();
            let bar = util::bar(progress, bar_width);
            Line::from(vec![
                Span::styled(
                    util::pad_right(&util::truncate(&change.id, name_width), name_width + 1),
                    Style::new().fg(theme.text),
                ),
                Span::styled(bar.filled, Style::new().fg(theme.progress(progress))),
                Span::styled(bar.track, Style::new().fg(theme.border)),
                Span::styled(format!(" {:>3.0}%", progress * 100.0), theme.dim()),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_recent(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;

    let block = panel_block(theme, &t!("tui.panel.recent"), false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let documents = app.workspace.recently_updated(usize::from(inner.height));
    if documents.is_empty() {
        frame.render_widget(empty_paragraph(theme, &t!("tui.empty.specs")), inner);
        return;
    }

    let title_width = usize::from(inner.width).saturating_sub(34).clamp(10, 48);

    // Section headings such as "Purpose" repeat across documents, so the path
    // is what actually identifies a row once there is room for it.
    let path_width = usize::from(inner.width).saturating_sub(title_width + 28);

    let lines: Vec<Line<'static>> = documents
        .iter()
        .map(|document| {
            let mut spans = vec![
                Span::styled(
                    format!("{} ", document.status.glyph()),
                    Style::new().fg(theme.status(document.status)),
                ),
                Span::styled(
                    util::pad_right(
                        &util::truncate(&document.title, title_width),
                        title_width + 2,
                    ),
                    Style::new().fg(theme.text),
                ),
                Span::styled(
                    util::pad_right(&util::format_relative(document.modified), 14),
                    theme.dim(),
                ),
                Span::styled(
                    util::pad_right(&util::format_size(document.size), 10),
                    Style::new().fg(theme.info),
                ),
            ];

            if path_width >= 12 {
                spans.push(Span::styled(
                    util::truncate(&document.display_path(), path_width),
                    theme.dim(),
                ));
            }

            Line::from(spans)
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}
