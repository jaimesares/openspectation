//! A compact Markdown renderer producing pre-wrapped Ratatui lines.
//!
//! Wrapping is done here instead of delegating to `Paragraph::wrap` so the
//! number of rendered lines is known exactly. That makes the scrollbar precise
//! and lets the preview pane render only the visible slice of a document.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::domain::spec::checkbox_state;
use crate::tui::theme::Theme;

/// Renders a Markdown body into wrapped lines of exactly `width` columns.
pub fn render(body: &str, theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width.max(8));
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut in_code_fence = false;
    let mut code_language = String::new();
    let mut previous_blank = true;

    for raw_line in body.lines() {
        let trimmed = raw_line.trim_end();
        let stripped = trimmed.trim_start();
        let indent = trimmed.len() - stripped.len();

        // --- fenced code blocks --------------------------------------------
        if stripped.starts_with("```") || stripped.starts_with("~~~") {
            if in_code_fence {
                out.push(code_footer(theme, width));
                in_code_fence = false;
                code_language.clear();
            } else {
                code_language = stripped.trim_start_matches(['`', '~']).trim().to_string();
                in_code_fence = true;
                out.push(code_header(theme, width, &code_language));
            }
            previous_blank = false;
            continue;
        }

        if in_code_fence {
            out.push(code_line(theme, width, trimmed));
            previous_blank = false;
            continue;
        }

        // --- blank lines ----------------------------------------------------
        if stripped.is_empty() {
            if !previous_blank {
                out.push(Line::default());
            }
            previous_blank = true;
            continue;
        }
        previous_blank = false;

        // --- horizontal rules ------------------------------------------------
        if is_rule(stripped) {
            out.push(Line::from(Span::styled("─".repeat(width), theme.rule())));
            continue;
        }

        // --- headings ---------------------------------------------------------
        if let Some(level) = heading_level(stripped) {
            let text = stripped[level as usize..].trim();
            let style = theme.heading(level);
            let marker = match level {
                1 => "▎ ",
                2 => "▏ ",
                _ => "  ",
            };

            if level <= 2 && !out.last().is_some_and(is_blank) {
                out.push(Line::default());
            }

            let prefix = vec![Span::styled(
                marker.to_string(),
                Style::new().fg(theme.accent),
            )];
            out.extend(wrap(
                &parse_inline(text, theme, style),
                width,
                &prefix,
                &prefix,
            ));

            if level <= 2 {
                let rule_width = width.saturating_sub(2);
                out.push(Line::from(vec![
                    Span::styled("  ".to_string(), theme.dim()),
                    Span::styled("─".repeat(rule_width), theme.rule()),
                ]));
            }
            continue;
        }

        // --- block quotes -------------------------------------------------------
        if let Some(rest) = stripped.strip_prefix('>') {
            let prefix = vec![Span::styled(
                "▎ ".to_string(),
                Style::new().fg(theme.primary),
            )];
            out.extend(wrap(
                &parse_inline(rest.trim_start(), theme, theme.quote()),
                width,
                &prefix,
                &prefix,
            ));
            continue;
        }

        // --- task list items -------------------------------------------------------
        if let Some(done) = checkbox_state(stripped) {
            let text = stripped
                .split_once(']')
                .map_or("", |(_, rest)| rest)
                .trim_start();
            let (glyph, color) = if done {
                ("✔ ", theme.success)
            } else {
                ("☐ ", theme.text_dim)
            };
            let pad = " ".repeat(indent.min(width / 2));
            let first = vec![
                Span::styled(pad.clone(), theme.dim()),
                Span::styled(glyph.to_string(), Style::new().fg(color)),
            ];
            let cont = vec![Span::styled(format!("{pad}  "), theme.dim())];
            let style = if done {
                theme.dim().add_modifier(Modifier::CROSSED_OUT)
            } else {
                Style::new().fg(theme.text)
            };
            out.extend(wrap(
                &parse_inline(text, theme, style),
                width,
                &first,
                &cont,
            ));
            continue;
        }

        // --- bullet and ordered lists -------------------------------------------
        if let Some((marker, rest)) = list_item(stripped) {
            let pad = " ".repeat(indent.min(width / 2));
            let first = vec![
                Span::styled(pad.clone(), theme.dim()),
                Span::styled(marker.clone(), theme.bullet()),
            ];
            let cont = vec![Span::styled(
                format!("{pad}{}", " ".repeat(marker.chars().count())),
                theme.dim(),
            )];
            out.extend(wrap(
                &parse_inline(rest, theme, Style::new().fg(theme.text)),
                width,
                &first,
                &cont,
            ));
            continue;
        }

        // --- tables -----------------------------------------------------------------
        if stripped.starts_with('|') && stripped.ends_with('|') {
            out.push(table_row(theme, stripped, width));
            continue;
        }

        // --- plain paragraph ---------------------------------------------------------
        let pad = " ".repeat(indent.min(width / 2));
        let prefix = vec![Span::styled(pad, theme.dim())];
        out.extend(wrap(
            &parse_inline(stripped, theme, Style::new().fg(theme.text)),
            width,
            &prefix,
            &prefix,
        ));
    }

    if in_code_fence {
        out.push(code_footer(theme, width));
    }

    if out.is_empty() {
        out.push(Line::default());
    }

    out
}

fn is_blank(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.trim().is_empty())
}

fn is_rule(text: &str) -> bool {
    let compact: String = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    compact.len() >= 3
        && (compact.chars().all(|character| character == '-')
            || compact.chars().all(|character| character == '*')
            || compact.chars().all(|character| character == '_'))
}

fn heading_level(text: &str) -> Option<u8> {
    let hashes = text
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    // ATX headings require a space after the hashes.
    text[hashes..]
        .starts_with(char::is_whitespace)
        .then_some(hashes as u8)
}

fn list_item(text: &str) -> Option<(String, &str)> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = text.strip_prefix(marker) {
            return Some(("• ".to_string(), rest));
        }
    }

    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 3 {
        let rest = &text[digits..];
        for separator in [". ", ") "] {
            if let Some(after) = rest.strip_prefix(separator) {
                return Some((format!("{} ", &text[..digits + 1]), after));
            }
        }
    }

    None
}

fn code_header(theme: &Theme, width: usize, language: &str) -> Line<'static> {
    let label = if language.is_empty() {
        String::new()
    } else {
        format!(" {language} ")
    };
    let filler = width.saturating_sub(label.chars().count() + 2);
    Line::from(vec![
        Span::styled("╭".to_string(), theme.rule()),
        Span::styled(label, Style::new().fg(theme.accent)),
        Span::styled("─".repeat(filler), theme.rule()),
        Span::styled("╮".to_string(), theme.rule()),
    ])
}

fn code_footer(theme: &Theme, width: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled("╰".to_string(), theme.rule()),
        Span::styled("─".repeat(width.saturating_sub(2)), theme.rule()),
        Span::styled("╯".to_string(), theme.rule()),
    ])
}

fn code_line(theme: &Theme, width: usize, text: &str) -> Line<'static> {
    let inner = width.saturating_sub(4);
    let content: String = text.chars().take(inner).collect();
    let padding = inner.saturating_sub(content.chars().count());
    Line::from(vec![
        Span::styled("│ ".to_string(), theme.rule()),
        Span::styled(content, theme.code()),
        Span::styled(" ".repeat(padding), theme.code()),
        Span::styled(" │".to_string(), theme.rule()),
    ])
}

fn table_row(theme: &Theme, text: &str, width: usize) -> Line<'static> {
    let cells: Vec<&str> = text.trim_matches('|').split('|').collect();
    let count = cells.len().max(1);
    // One separator before each cell plus one closing separator.
    let column = width.saturating_sub(count + 1) / count;

    let is_separator = cells
        .iter()
        .all(|cell| !cell.trim().is_empty() && cell.trim().chars().all(|c| c == '-' || c == ':'));

    if is_separator {
        let mut spans = vec![Span::styled("├".to_string(), theme.rule())];
        for index in 0..count {
            spans.push(Span::styled("─".repeat(column), theme.rule()));
            let joint = if index + 1 == count { "┤" } else { "┼" };
            spans.push(Span::styled(joint.to_string(), theme.rule()));
        }
        return Line::from(spans);
    }

    let mut spans = vec![Span::styled("│".to_string(), theme.rule())];

    for cell in cells {
        // A leading space keeps the text off the column rule.
        let content = format!(
            " {}",
            crate::domain::util::truncate(cell.trim(), column.saturating_sub(1))
        );
        let padding = column.saturating_sub(content.chars().count());
        spans.push(Span::styled(content, Style::new().fg(theme.text)));
        spans.push(Span::styled(
            " ".repeat(padding),
            Style::new().fg(theme.text),
        ));
        spans.push(Span::styled("│".to_string(), theme.rule()));
    }

    Line::from(spans)
}

// ---------------------------------------------------------------------------
// Inline parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Segment {
    text: String,
    style: Style,
}

/// Splits inline Markdown into styled segments.
///
/// Supports `**bold**`, `__bold__`, `*italic*`, `_italic_`, `` `code` ``,
/// `~~strikethrough~~`, `[label](url)` and backslash escapes.
fn parse_inline(text: &str, theme: &Theme, base: Style) -> Vec<Segment> {
    let characters: Vec<char> = text.chars().collect();
    let mut segments: Vec<Segment> = Vec::new();
    let mut buffer = String::new();
    let mut bold = false;
    let mut italic = false;
    let mut strike = false;
    let mut index = 0usize;

    let current = |bold: bool, italic: bool, strike: bool| {
        let mut style = base;
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if strike {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        style
    };

    while index < characters.len() {
        let character = characters[index];

        // Escapes.
        if character == '\\' && index + 1 < characters.len() {
            buffer.push(characters[index + 1]);
            index += 2;
            continue;
        }

        // Inline code spans are verbatim.
        if character == '`'
            && let Some(end) = find_char(&characters, index + 1, '`')
        {
            flush(&mut segments, &mut buffer, current(bold, italic, strike));
            let code: String = characters[index + 1..end].iter().collect();
            segments.push(Segment {
                text: format!(" {code} "),
                style: theme.code(),
            });
            index = end + 1;
            continue;
        }

        // Links: the label is kept, the target is dropped.
        if character == '['
            && let Some((label, next)) = parse_link(&characters, index)
        {
            flush(&mut segments, &mut buffer, current(bold, italic, strike));
            segments.push(Segment {
                text: label,
                style: theme.link(),
            });
            index = next;
            continue;
        }

        if starts_with(&characters, index, "**") || starts_with(&characters, index, "__") {
            flush(&mut segments, &mut buffer, current(bold, italic, strike));
            bold = !bold;
            index += 2;
            continue;
        }

        if starts_with(&characters, index, "~~") {
            flush(&mut segments, &mut buffer, current(bold, italic, strike));
            strike = !strike;
            index += 2;
            continue;
        }

        if (character == '*' || character == '_') && emphasis_boundary(&characters, index, italic) {
            flush(&mut segments, &mut buffer, current(bold, italic, strike));
            italic = !italic;
            index += 1;
            continue;
        }

        buffer.push(character);
        index += 1;
    }

    flush(&mut segments, &mut buffer, current(bold, italic, strike));

    if segments.is_empty() {
        segments.push(Segment {
            text: String::new(),
            style: base,
        });
    }

    segments
}

fn flush(segments: &mut Vec<Segment>, buffer: &mut String, style: Style) {
    if buffer.is_empty() {
        return;
    }
    segments.push(Segment {
        text: std::mem::take(buffer),
        style,
    });
}

fn starts_with(characters: &[char], index: usize, pattern: &str) -> bool {
    pattern
        .chars()
        .enumerate()
        .all(|(offset, expected)| characters.get(index + offset) == Some(&expected))
}

fn find_char(characters: &[char], from: usize, needle: char) -> Option<usize> {
    (from..characters.len()).find(|index| characters[*index] == needle)
}

/// Emphasis markers only toggle at a word boundary, so `snake_case` and
/// multiplications survive unchanged.
fn emphasis_boundary(characters: &[char], index: usize, open: bool) -> bool {
    let before = index
        .checked_sub(1)
        .and_then(|previous| characters.get(previous));
    let after = characters.get(index + 1);

    if open {
        // Closing marker: previous character must be content.
        before.is_some_and(|character| !character.is_whitespace())
    } else {
        // Opening marker: must be followed by content and preceded by a break.
        after.is_some_and(|character| !character.is_whitespace())
            && before.is_none_or(|character| !character.is_alphanumeric())
    }
}

fn parse_link(characters: &[char], index: usize) -> Option<(String, usize)> {
    let close = find_char(characters, index + 1, ']')?;
    if characters.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = find_char(characters, close + 2, ')')?;
    let label: String = characters[index + 1..close].iter().collect();
    Some((label, end + 1))
}

// ---------------------------------------------------------------------------
// Wrapping
// ---------------------------------------------------------------------------

struct WordPart {
    text: String,
    style: Style,
}

/// Greedy word wrap that keeps per-span styling and supports hanging indents.
fn wrap(
    segments: &[Segment],
    width: usize,
    first_prefix: &[Span<'static>],
    continuation_prefix: &[Span<'static>],
) -> Vec<Line<'static>> {
    let first_indent = span_width(first_prefix);
    let continuation_indent = span_width(continuation_prefix);
    let first_limit = width.saturating_sub(first_indent).max(1);
    let continuation_limit = width.saturating_sub(continuation_indent).max(1);

    let words = tokenize(segments);
    if words.is_empty() {
        return vec![Line::from(first_prefix.to_vec())];
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = first_prefix.to_vec();
    let mut used = 0usize;
    let mut has_content = false;

    for word in words {
        let limit = if lines.is_empty() {
            first_limit
        } else {
            continuation_limit
        };
        let length: usize = word.iter().map(|part| part.text.chars().count()).sum();

        if length > limit {
            // A single token longer than the line: hard break it.
            if has_content {
                lines.push(Line::from(std::mem::take(&mut spans)));
                spans = continuation_prefix.to_vec();
                used = 0;
                has_content = false;
            }
            for part in word {
                for chunk in chunks(&part.text, limit) {
                    let chunk_length = chunk.chars().count();
                    if used + chunk_length > limit && has_content {
                        lines.push(Line::from(std::mem::take(&mut spans)));
                        spans = continuation_prefix.to_vec();
                        used = 0;
                    }
                    spans.push(Span::styled(chunk, part.style));
                    used += chunk_length;
                    has_content = true;
                }
            }
            continue;
        }

        let separator = usize::from(has_content);
        if has_content && used + separator + length > limit {
            lines.push(Line::from(std::mem::take(&mut spans)));
            spans = continuation_prefix.to_vec();
            used = 0;
            has_content = false;
        }

        if has_content {
            spans.push(Span::raw(" "));
            used += 1;
        }
        for part in word {
            spans.push(Span::styled(part.text, part.style));
        }
        used += length;
        has_content = true;
    }

    lines.push(Line::from(spans));
    lines
}

fn tokenize(segments: &[Segment]) -> Vec<Vec<WordPart>> {
    let mut words: Vec<Vec<WordPart>> = Vec::new();
    let mut pending: Vec<WordPart> = Vec::new();
    let mut buffer = String::new();

    for segment in segments {
        for character in segment.text.chars() {
            if character.is_whitespace() {
                if !buffer.is_empty() {
                    pending.push(WordPart {
                        text: std::mem::take(&mut buffer),
                        style: segment.style,
                    });
                }
                if !pending.is_empty() {
                    words.push(std::mem::take(&mut pending));
                }
            } else {
                buffer.push(character);
            }
        }
        if !buffer.is_empty() {
            pending.push(WordPart {
                text: std::mem::take(&mut buffer),
                style: segment.style,
            });
        }
    }

    if !pending.is_empty() {
        words.push(pending);
    }

    words
}

fn chunks(text: &str, size: usize) -> Vec<String> {
    let size = size.max(1);
    let characters: Vec<char> = text.chars().collect();
    characters
        .chunks(size)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn span_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::DARK
    }

    #[test]
    fn wraps_long_paragraphs_to_the_requested_width() {
        let body = "word ".repeat(40);
        let lines = render(&body, &theme(), 20);
        assert!(lines.len() > 1);
        for line in &lines {
            let width: usize = line
                .spans
                .iter()
                .map(|span| span.content.chars().count())
                .sum();
            assert!(width <= 20, "line too wide: {width}");
        }
    }

    #[test]
    fn detects_atx_headings_only_with_a_space() {
        assert_eq!(heading_level("# Title"), Some(1));
        assert_eq!(heading_level("###### Deep"), Some(6));
        assert_eq!(heading_level("#NoSpace"), None);
        assert_eq!(heading_level("####### TooDeep"), None);
    }

    #[test]
    fn keeps_snake_case_intact() {
        let segments = parse_inline("call some_function_name now", &theme(), Style::new());
        let joined: String = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(joined, "call some_function_name now");
    }

    #[test]
    fn parses_bold_and_code() {
        let segments = parse_inline("a **b** `c`", &theme(), Style::new());
        let joined: String = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(joined, "a b  c ");
    }

    #[test]
    fn renders_links_without_the_target() {
        let segments = parse_inline("see [docs](https://example.com)", &theme(), Style::new());
        let joined: String = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(joined, "see docs");
    }

    fn text_of(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn rendered(body: &str, width: u16) -> Vec<String> {
        render(body, &theme(), width).iter().map(text_of).collect()
    }

    fn inline(text: &str) -> String {
        parse_inline(text, &theme(), Style::new())
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }

    /// The property the scrollbar depends on: the caller asked for `width`
    /// columns and no produced line may exceed them, whatever the block type.
    #[test]
    fn no_rendered_line_ever_exceeds_the_requested_width() {
        let body = "\
# A heading long enough to need wrapping across several columns

Paragraph with **bold**, `code`, a [link](https://example.com) and a
supercalifragilisticexpialidocious token.

- a bullet item that also has to wrap somewhere
1. an ordered item

> a quoted line that runs past the edge

| column one | column two |
| --- | --- |
| value | other |

```rust
let x = 1; // a comment long enough to be truncated by the frame
```

---
";
        for width in [8u16, 12, 20, 37, 80] {
            for line in render(body, &theme(), width) {
                let measured = span_width(&line.spans);
                assert!(
                    measured <= usize::from(width),
                    "line {:?} measured {measured} at width {width}",
                    text_of(&line)
                );
            }
        }
    }

    #[test]
    fn a_width_below_the_floor_is_raised_to_eight_columns() {
        // `render` clamps rather than dividing by zero further down.
        for line in render("hello world", &theme(), 1) {
            assert!(span_width(&line.spans) <= 8);
        }
    }

    #[test]
    fn an_empty_body_still_yields_one_line() {
        // The viewer indexes into the returned slice, so it must never be empty.
        assert_eq!(render("", &theme(), 40).len(), 1);
        assert_eq!(rendered("", 40), vec![""]);
    }

    #[test]
    fn consecutive_blank_lines_collapse_into_one() {
        assert_eq!(rendered("a\n\n\n\nb", 40), vec!["a", "", "b"]);
        assert_eq!(rendered("\n\n\na", 40), vec!["a"]);
    }

    #[test]
    fn a_top_level_heading_is_framed_by_a_blank_line_and_a_rule() {
        let lines = rendered("# Title", 20);
        assert_eq!(lines[0], "");
        assert!(lines[1].contains("Title"));
        assert!(lines[2].trim_start().starts_with('─'));
    }

    #[test]
    fn deep_headings_get_neither_the_blank_line_nor_the_rule() {
        let lines = rendered("### Small", 20);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Small"));
    }

    #[test]
    fn an_unterminated_code_fence_is_closed_at_the_end_of_the_document() {
        let lines = rendered("```rust\nlet x = 1;", 30);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with('╭') && lines[0].contains("rust"));
        assert!(lines[1].contains("let x = 1;"));
        assert!(lines[2].starts_with('╰') && lines[2].ends_with('╯'));
    }

    #[test]
    fn tildes_open_a_fence_just_like_backticks() {
        let lines = rendered("~~~\nplain\n~~~", 30);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with('╭'));
        assert!(lines[2].starts_with('╰'));
    }

    #[test]
    fn markdown_inside_a_fence_is_left_verbatim() {
        // Requirement counting elsewhere also ignores fenced content; the
        // renderer must not disagree with it.
        let lines = rendered("```\n# not a heading\n- not a bullet\n```", 40);
        assert!(lines[1].contains("# not a heading"));
        assert!(lines[2].contains("- not a bullet"));
    }

    #[test]
    fn a_code_line_is_truncated_to_the_frame_and_padded_back_out() {
        let line = code_line(&theme(), 12, "0123456789abcdef");
        assert_eq!(text_of(&line), "│ 01234567 │");

        let padded = code_line(&theme(), 12, "ab");
        assert_eq!(text_of(&padded), "│ ab       │");
    }

    #[test]
    fn a_fence_without_a_language_gets_an_unlabelled_header() {
        assert_eq!(text_of(&code_header(&theme(), 10, "")), "╭────────╮");
        assert_eq!(text_of(&code_header(&theme(), 10, "rs")), "╭ rs ────╮");
    }

    #[test]
    fn rules_need_three_repeats_of_a_single_character() {
        assert!(is_rule("---"));
        assert!(is_rule("***"));
        assert!(is_rule("___"));
        assert!(is_rule("- - -"));
        assert!(!is_rule("--"));
        assert!(!is_rule("-*-"));
        assert!(!is_rule("text"));
    }

    #[test]
    fn bullet_markers_are_normalized_and_ordered_markers_are_kept() {
        assert_eq!(list_item("- one"), Some(("• ".to_string(), "one")));
        assert_eq!(list_item("* two"), Some(("• ".to_string(), "two")));
        assert_eq!(list_item("+ three"), Some(("• ".to_string(), "three")));
        assert_eq!(list_item("1. four"), Some(("1. ".to_string(), "four")));
        assert_eq!(list_item("12) five"), Some(("12) ".to_string(), "five")));
        assert_eq!(list_item("1234. six"), None);
        assert_eq!(list_item("1.no space"), None);
        assert_eq!(list_item("plain"), None);
    }

    #[test]
    fn a_wrapped_list_item_hangs_under_its_own_text() {
        let lines = rendered("- alpha beta gamma delta epsilon", 16);
        assert!(lines[0].starts_with("• "));
        for continuation in &lines[1..] {
            assert!(
                continuation.starts_with("  "),
                "continuation not indented: {continuation:?}"
            );
        }
    }

    #[test]
    fn a_task_item_shows_its_checkbox_state() {
        let lines = rendered("- [x] done\n- [ ] pending", 40);
        assert!(lines[0].contains('✔') && lines[0].contains("done"));
        assert!(lines[1].contains('☐') && lines[1].contains("pending"));
    }

    #[test]
    fn a_block_quote_drops_its_marker_for_a_styled_bar() {
        let lines = rendered("> quoted text", 40);
        assert!(lines[0].starts_with("▎ "));
        assert!(lines[0].contains("quoted text"));
        assert!(!lines[0].contains('>'));
    }

    #[test]
    fn a_table_separator_row_becomes_box_drawing_joints() {
        let lines = rendered("| a | b |\n| --- | --- |", 24);
        assert!(lines[0].starts_with('│') && lines[0].contains('a'));
        assert!(lines[1].starts_with('├') && lines[1].contains('┼') && lines[1].ends_with('┤'));
    }

    #[test]
    fn an_alignment_row_counts_as_a_separator() {
        let line = table_row(&theme(), "| :--- | ---: |", 24);
        assert!(text_of(&line).starts_with('├'));
    }

    #[test]
    fn a_token_longer_than_the_line_is_hard_broken() {
        let lines = rendered(&"x".repeat(30), 10);
        assert_eq!(lines.len(), 3);
        for line in &lines {
            assert_eq!(line.chars().count(), 10);
        }
        assert_eq!(lines.concat(), "x".repeat(30));
    }

    #[test]
    fn wrapping_preserves_every_word_in_order() {
        let body = "alpha beta gamma delta epsilon zeta eta theta";
        let joined = rendered(body, 12).join(" ");
        let words: Vec<&str> = joined.split_whitespace().collect();
        assert_eq!(words, body.split(' ').collect::<Vec<_>>());
    }

    #[test]
    fn escapes_keep_the_following_character_literal() {
        assert_eq!(inline(r"\*not italic\*"), "*not italic*");
        assert_eq!(inline(r"a \\ b"), r"a \ b");
    }

    #[test]
    fn emphasis_only_toggles_at_word_boundaries() {
        assert_eq!(inline("a *b* c"), "a b c");
        assert_eq!(inline("2 * 3 * 4"), "2 * 3 * 4");
        assert_eq!(inline("_lead_"), "lead");
    }

    #[test]
    fn strikethrough_and_underscore_bold_are_recognized() {
        assert_eq!(inline("~~gone~~"), "gone");
        assert_eq!(inline("__loud__"), "loud");
    }

    #[test]
    fn an_unclosed_marker_is_left_as_text() {
        assert_eq!(inline("`unclosed"), "`unclosed");
        assert_eq!(inline("[label] no target"), "[label] no target");
        assert_eq!(inline("[label](unterminated"), "[label](unterminated");
    }

    #[test]
    fn inline_styles_reach_the_produced_spans() {
        let segments = parse_inline("plain **bold** ~~gone~~", &theme(), Style::new());
        let bold = segments
            .iter()
            .find(|segment| segment.text.contains("bold"))
            .unwrap();
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));

        let struck = segments
            .iter()
            .find(|segment| segment.text.contains("gone"))
            .unwrap();
        assert!(struck.style.add_modifier.contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn an_empty_inline_run_still_produces_one_segment() {
        let segments = parse_inline("", &theme(), Style::new());
        assert_eq!(segments.len(), 1);
        assert!(segments[0].text.is_empty());
    }

    #[test]
    fn chunks_splits_on_character_counts_not_bytes() {
        assert_eq!(chunks("abcdef", 2), vec!["ab", "cd", "ef"]);
        assert_eq!(chunks("áéíóú", 2), vec!["áé", "íó", "ú"]);
        // A zero size would loop forever, so it is floored at one.
        assert_eq!(chunks("ab", 0), vec!["a", "b"]);
    }

    #[test]
    fn accented_text_is_measured_in_characters() {
        let lines = rendered("áéíóú ñçü", 8);
        for line in &lines {
            assert!(line.chars().count() <= 8);
        }
    }
}
