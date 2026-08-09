//! The timeline: what happened in this workspace, and what it implies.
//!
//! Reached with `H` or `:activity`, and left with the same key or `Esc`. Two
//! halves: the completions themselves, grouped by day and newest first, and the
//! per-change figures derived from them.
//!
//! Every duration on this screen is elapsed time — from a task appearing to it
//! being completed — and the screen says so, because measuring effort would
//! need an explicit start and stop and the discipline to use them. A task that
//! sat untouched for a week reads as a week, and correcting that would be a
//! guess dressed up as a measurement.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use rust_i18n::t;

use crate::domain::history::{ChangeSummary, Completion};
use crate::domain::util;
use crate::tui::app::App;
use crate::tui::ui::{NARROW_WIDTH, Track, draw_scrollbar, empty_paragraph, panel_block};

/// Widest the per-change summary is allowed to grow.
const SUMMARY_MAX: u16 = 46;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    // Narrow terminals get the timeline alone: the summary is the derived half,
    // and half a table of figures is worse than none.
    if area.width < NARROW_WIDTH {
        draw_timeline(frame, app, area);
        return;
    }

    let summary_width = (area.width / 3).clamp(30, SUMMARY_MAX);
    let columns =
        Layout::horizontal([Constraint::Min(34), Constraint::Length(summary_width)]).split(area);

    draw_timeline(frame, app, columns[0]);
    draw_summary(frame, app, columns[1]);
}

fn draw_timeline(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;

    let mut block = panel_block(theme, &t!("tui.activity.title"), true).title_bottom(
        Line::styled(format!(" {} ", t!("tui.activity.elapsed")), theme.dim()).right_aligned(),
    );

    // An acknowledged gap beats a partial history presented as the whole
    // record, so the marker sits on the frame rather than in the log.
    if app.timeline().truncated {
        block = block.title_top(
            Line::styled(
                format!(" {} ", t!("tui.activity.incomplete")),
                Style::new().fg(theme.warning),
            )
            .right_aligned(),
        );
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    app.activity_height = inner.height.max(1);
    let lines = timeline_lines(app, usize::from(inner.width));
    app.activity_total = lines.len();

    if lines.is_empty() {
        frame.render_widget(empty_paragraph(theme, &t!("tui.empty.activity")), inner);
        return;
    }

    let offset = usize::from(app.activity_scroll);
    frame.render_widget(
        Paragraph::new(
            lines
                .iter()
                .skip(offset)
                .take(usize::from(inner.height))
                .cloned()
                .collect::<Vec<_>>(),
        ),
        inner,
    );

    draw_scrollbar(frame, theme, area, lines.len(), offset, 1, Track::Offset);
}

/// The completions, grouped under the day they happened on.
fn timeline_lines(app: &mut App, width: usize) -> Vec<Line<'static>> {
    let theme = app.theme;
    let completions: Vec<Completion> = app.timeline().completions.clone();

    let change_width = width.saturating_sub(34).clamp(8, 24);
    let label_width = width.saturating_sub(change_width + 22).clamp(10, 72);

    let mut lines = Vec::new();
    let mut day = String::new();

    for completion in &completions {
        let today = util::format_date(Some(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(completion.at),
        ));
        if today != day {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.push(Line::from(Span::styled(today.clone(), theme.strong())));
            day = today;
        }

        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{}  ",
                    util::format_clock(Some(
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(completion.at)
                    ))
                ),
                theme.dim(),
            ),
            Span::styled(
                util::pad_right(
                    &util::truncate(&completion.change, change_width),
                    change_width + 1,
                ),
                Style::new().fg(theme.accent),
            ),
            Span::styled(
                util::pad_right(
                    &util::truncate(&completion.label, label_width),
                    label_width + 1,
                ),
                Style::new().fg(theme.text),
            ),
            Span::styled(
                match completion.elapsed {
                    Some(elapsed) => util::format_duration(elapsed),
                    None => String::new(),
                },
                Style::new().fg(theme.success),
            ),
        ]));
    }

    lines
}

fn draw_summary(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;

    let block = panel_block(theme, &t!("tui.activity.per_change"), false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let suggest = app.timeline().suggest_filters;
    let summaries: Vec<ChangeSummary> = app.timeline().changes.clone();

    if summaries.is_empty() {
        frame.render_widget(empty_paragraph(theme, &t!("tui.empty.activity")), inner);
        return;
    }

    let width = usize::from(inner.width);
    let name_width = width.saturating_sub(14).clamp(8, 28);
    let mut lines: Vec<Line<'static>> = Vec::new();

    for summary in summaries.iter().take(usize::from(inner.height) / 3) {
        let ratio = if summary.total > 0 {
            summary.done as f64 / summary.total as f64
        } else {
            0.0
        };
        let bar = util::bar(ratio, 8);

        lines.push(Line::from(vec![
            Span::styled(
                util::pad_right(&util::truncate(&summary.change, name_width), name_width + 1),
                Style::new()
                    .fg(theme.text_strong)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(bar.filled, Style::new().fg(theme.progress(ratio))),
            Span::styled(bar.track, Style::new().fg(theme.border)),
        ]));
        lines.push(Line::from(Span::styled(
            format!(
                "  {}",
                t!(
                    "tui.activity.progress",
                    done = summary.done,
                    total = summary.total
                )
            ),
            theme.dim(),
        )));

        // Nothing derived is said at all where there is nothing to derive it
        // from: a change with no completed task gets silence, not a zero.
        let mut figures: Vec<String> = Vec::new();
        if let Some(elapsed) = summary.elapsed {
            figures.push(
                t!(
                    "tui.activity.running",
                    duration = util::format_duration(elapsed)
                )
                .to_string(),
            );
        }
        if let Some(median) = summary.median {
            figures.push(
                t!(
                    "tui.activity.median",
                    duration = util::format_duration(median)
                )
                .to_string(),
            );
        }
        if let Some(estimate) = summary.estimate {
            figures.push(
                t!(
                    "tui.activity.estimate",
                    duration = util::format_duration(estimate)
                )
                .to_string(),
            );
        }

        if figures.is_empty() {
            lines.push(Line::default());
        } else {
            lines.push(Line::from(Span::styled(
                format!("  {}", figures.join(" · ")),
                Style::new().fg(theme.info),
            )));
        }
    }

    // The filters cost time and space inside a repository this tool does not
    // own, so what is offered is the command, not the act.
    if suggest && lines.len() + 2 <= usize::from(inner.height) {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            t!("tui.activity.suggest_filters").to_string(),
            Style::new().fg(theme.warning),
        )));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}
