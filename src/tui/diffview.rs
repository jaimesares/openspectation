//! Renders the requirement deltas of a change as pre-wrapped lines.
//!
//! This is not a textual diff. An OpenSpec change declares what it does to a
//! capability, so the view reports the declared operations rather than trying
//! to reconstruct them from two versions of a file.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use rust_i18n::t;

use crate::domain::delta::{CapabilityDelta, DeltaKind, RequirementDelta};
use crate::domain::util;
use crate::tui::theme::Theme;

/// Builds the delta view for a change, wrapped to `width` columns.
pub fn render(deltas: &[CapabilityDelta], theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width.max(16));
    let mut out: Vec<Line<'static>> = Vec::new();

    if deltas.is_empty() {
        return out;
    }

    out.push(Line::from(Span::styled(
        t!("tui.diff.title").to_string(),
        theme.heading(1),
    )));
    out.push(rule(theme, width));
    out.push(Line::default());

    for capability in deltas {
        out.extend(header(capability, theme, width));

        for requirement in &capability.requirements {
            out.extend(entry(requirement, theme, width));
        }

        out.push(Line::default());
    }

    out.extend(totals(deltas, theme, width));
    out
}

fn header(capability: &CapabilityDelta, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let status = if capability.existing {
        t!("tui.diff.existing", count = capability.current_requirements)
    } else {
        t!("tui.diff.new_capability")
    }
    .to_string();

    let name = util::truncate(&capability.capability, width.saturating_sub(4));

    vec![
        Line::from(vec![
            Span::styled("▎ ", Style::new().fg(theme.accent)),
            Span::styled(
                name,
                Style::new()
                    .fg(theme.text_strong)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                util::truncate(&status, width.saturating_sub(2)),
                theme.dim(),
            ),
        ]),
        Line::default(),
    ]
}

fn entry(requirement: &RequirementDelta, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let kind = requirement.kind;
    let color = colour(kind, theme);
    let marker = format!("  {} ", kind.glyph());
    let tag = format!("{} ", kind.tag());
    let indent = marker.chars().count() + tag.chars().count();

    let mut lines = Vec::new();
    let mut first = true;

    for chunk in wrap(&requirement.name, width.saturating_sub(indent).max(8)) {
        let mut spans = Vec::new();
        if first {
            spans.push(Span::styled(marker.clone(), Style::new().fg(color)));
            spans.push(Span::styled(
                tag.clone(),
                Style::new().fg(color).add_modifier(Modifier::BOLD),
            ));
            first = false;
        } else {
            spans.push(Span::raw(" ".repeat(indent)));
        }
        spans.push(Span::styled(chunk, Style::new().fg(theme.text)));
        lines.push(Line::from(spans));
    }

    // The line number is what makes the entry actionable: it is where the
    // requirement is written, not where it will end up.
    let mut trailer = format!("L{}", requirement.line);
    if requirement.scenarios > 0 {
        trailer.push_str(&format!(
            " · {}",
            t!("tui.diff.scenarios", count = requirement.scenarios)
        ));
    }
    lines.push(Line::from(vec![
        Span::raw(" ".repeat(indent)),
        Span::styled(trailer, theme.dim()),
    ]));

    lines
}

fn totals(deltas: &[CapabilityDelta], theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut spans = vec![Span::styled("  ", theme.dim())];
    let mut first = true;

    for kind in [
        DeltaKind::Added,
        DeltaKind::Modified,
        DeltaKind::Removed,
        DeltaKind::Renamed,
    ] {
        let count: usize = deltas.iter().map(|capability| capability.count(kind)).sum();
        if count == 0 {
            continue;
        }
        if !first {
            spans.push(Span::styled("   ", theme.dim()));
        }
        first = false;
        spans.push(Span::styled(
            format!("{} {count} {}", kind.glyph(), kind.label()),
            Style::new().fg(colour(kind, theme)),
        ));
    }

    if first {
        return Vec::new();
    }

    vec![rule(theme, width), spans.into()]
}

const fn colour(kind: DeltaKind, theme: &Theme) -> ratatui::style::Color {
    match kind {
        DeltaKind::Added => theme.success,
        DeltaKind::Modified => theme.warning,
        DeltaKind::Removed => theme.danger,
        DeltaKind::Renamed => theme.info,
    }
}

fn rule(theme: &Theme, width: usize) -> Line<'static> {
    Line::from(Span::styled("─".repeat(width), theme.rule()))
}

/// Greedy word wrap; the delta view has no inline Markdown to preserve.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let length = word.chars().count();
        if current.is_empty() {
            current = word.to_string();
        } else if current.chars().count() + 1 + length <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(std::mem::take(&mut current));
            current = word.to_string();
        }

        // A single word longer than the line is cut rather than overflowing.
        while current.chars().count() > width {
            let head: String = current.chars().take(width).collect();
            current = current.chars().skip(width).collect();
            out.push(head);
        }
    }

    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    fn capability() -> CapabilityDelta {
        CapabilityDelta {
            capability: "edit-history".to_string(),
            path: PathBuf::from("/w/changes/add/specs/edit-history/spec.md"),
            existing: false,
            current_requirements: 0,
            requirements: vec![
                RequirementDelta {
                    kind: DeltaKind::Added,
                    name: "Undo SHALL restore the document".to_string(),
                    scenarios: 3,
                    line: 7,
                },
                RequirementDelta {
                    kind: DeltaKind::Removed,
                    name: "The stack SHALL clear on load".to_string(),
                    scenarios: 0,
                    line: 21,
                },
            ],
        }
    }

    fn text(lines: &[Line<'_>]) -> String {
        lines
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

    #[test]
    fn lists_every_operation_with_its_tag() {
        let rendered = render(&[capability()], &Theme::DARK, 60);
        let body = text(&rendered);

        assert!(body.contains("+ ADDED Undo SHALL restore the document"));
        assert!(body.contains("- REMOVED The stack SHALL clear on load"));
    }

    #[test]
    fn summarises_the_operations_at_the_end() {
        let rendered = render(&[capability()], &Theme::DARK, 60);
        let body = text(&rendered);

        assert!(body.contains("+ 1"));
        assert!(body.contains("- 1"));
    }

    #[test]
    fn nothing_to_show_renders_nothing() {
        assert!(render(&[], &Theme::DARK, 60).is_empty());
    }

    #[test]
    fn wraps_without_exceeding_the_width() {
        let long = "word ".repeat(40);
        for line in wrap(&long, 20) {
            assert!(line.chars().count() <= 20);
        }
    }
}
