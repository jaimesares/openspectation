//! `openspectation list` — colorized inventory of the specs directory.
//!
//! The whole listing is built as text and printed once. Laying it out is where
//! everything that can go wrong lives — column widths, grouping, the totals of
//! a filtered set — and text that is returned can be read back by a test.

use std::cmp::Reverse;
use std::path::Path;

use crossterm::style::Color;
use rust_i18n::t;

use crate::cli::{ListArgs, SortKey};
use crate::domain::spec::{SpecDocument, SpecStatus};
use crate::domain::util;
use crate::domain::workspace::Workspace;
use crate::error::Result;
use crate::paint::{Painter, palette};

/// Width assumed when the terminal size cannot be queried.
const FALLBACK_WIDTH: u16 = 100;

/// Narrowest terminal the table is laid out for. Below this the columns would
/// collide, so the layout pretends the screen is this wide and lets the
/// terminal wrap.
const MIN_WIDTH: u16 = 60;

const MIN_TITLE_WIDTH: usize = 12;
const MAX_TITLE_WIDTH: usize = 42;
const MAX_DOCUMENT_WIDTH: usize = 36;
const SIZE_WIDTH: usize = 9;
const UPDATED_WIDTH: usize = 13;
const INDENT: &str = "    ";

pub fn run(path: &Path, args: &ListArgs, painter: Painter) -> Result<()> {
    let workspace = Workspace::open(path)?;
    super::emit(&render(&workspace, args, painter, terminal_width()));
    Ok(())
}

fn render(workspace: &Workspace, args: &ListArgs, painter: Painter, width: u16) -> Vec<String> {
    let filter: Option<SpecStatus> = args.status.map(Into::into);
    let mut documents: Vec<&SpecDocument> = workspace
        .specs
        .iter()
        .filter(|document| filter.is_none_or(|status| document.status == status))
        .collect();
    sort_documents(&mut documents, args.sort);

    let mut out = render_header(workspace, painter);

    if workspace.specs.is_empty() {
        out.extend(render_empty(workspace, painter));
    } else if documents.is_empty() {
        let status = filter.map(SpecStatus::label).unwrap_or_default();
        out.push(format!(
            "{INDENT}{}",
            painter.warning(t!("list.filtered_empty", status = status))
        ));
        out.push(String::new());
    } else {
        out.extend(render_table(
            &documents,
            painter,
            width,
            args.sort == SortKey::Path,
        ));
        out.extend(render_summary(&documents, painter));
    }

    if args.all {
        out.extend(render_changes(workspace, painter, width));
    }

    out
}

fn render_header(workspace: &Workspace, painter: Painter) -> Vec<String> {
    let workspace_label = t!("list.workspace").to_string();
    let project_label = t!("list.project").to_string();
    let label_width = workspace_label
        .chars()
        .count()
        .max(project_label.chars().count());

    vec![
        String::new(),
        format!(
            "  {} {}",
            painter.accent_bold("◆"),
            painter.bold(t!("list.title"))
        ),
        format!(
            "{INDENT}{} {}",
            painter.dim(util::pad_right(&workspace_label, label_width)),
            painter.primary(workspace.display_root())
        ),
        format!(
            "{INDENT}{} {}",
            painter.dim(util::pad_right(&project_label, label_width)),
            painter.text(workspace.project_name())
        ),
        String::new(),
    ]
}

fn render_empty(workspace: &Workspace, painter: Painter) -> Vec<String> {
    let dir = workspace
        .specs_dir()
        .strip_prefix(&workspace.root)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| workspace.specs_dir().display().to_string());

    vec![
        format!("{INDENT}{}", painter.warning(t!("list.empty", dir = dir))),
        format!("{INDENT}{}", painter.dim(t!("list.empty_hint"))),
        String::new(),
    ]
}

/// Renders the table. `grouped` inserts a heading per directory, which only
/// makes sense while the documents are ordered by path.
fn render_table(
    documents: &[&SpecDocument],
    painter: Painter,
    width: u16,
    grouped: bool,
) -> Vec<String> {
    let status_width = SpecStatus::ALL
        .iter()
        .map(|status| status.label().chars().count())
        .max()
        .unwrap_or(10)
        + 2;

    let document_width = documents
        .iter()
        .map(|document| leaf_name(document, grouped).chars().count())
        .max()
        .unwrap_or(8)
        .clamp(8, MAX_DOCUMENT_WIDTH);

    let fixed =
        INDENT.len() + status_width + 2 + document_width + 2 + SIZE_WIDTH + 2 + UPDATED_WIDTH;
    let title_width = (width as usize)
        .saturating_sub(fixed + 2)
        .clamp(MIN_TITLE_WIDTH, MAX_TITLE_WIDTH);

    let mut out = vec![
        format!(
            "{INDENT}{}  {}  {}  {}  {}",
            painter.dim(util::pad_right(&t!("list.col_status"), status_width)),
            painter.dim(util::pad_right(&t!("list.col_document"), document_width)),
            painter.dim(util::pad_right(&t!("list.col_title"), title_width)),
            painter.dim(pad_left(&t!("list.col_size"), SIZE_WIDTH)),
            painter.dim(t!("list.col_updated"))
        ),
        format!(
            "{INDENT}{}",
            painter.dim("─".repeat(
                status_width + document_width + title_width + SIZE_WIDTH + UPDATED_WIDTH + 8
            ))
        ),
    ];

    // `None` means "no group printed yet"; the inner option is the group itself,
    // which is `None` for documents sitting directly in specs/.
    let mut current_group: Option<Option<String>> = None;

    for document in documents {
        let group = document.group();
        if grouped && current_group.as_ref() != Some(&group) {
            let label = group
                .clone()
                .unwrap_or_else(|| t!("list.root_group").to_string());
            out.push(format!("{INDENT}{}", painter.accent(format!("▸ {label}"))));
            current_group = Some(group);
        }

        out.push(format!(
            "{INDENT}{}  {}  {}  {}  {}",
            painter.colored(
                util::pad_right(
                    &format!("{} {}", document.status.glyph(), document.status.label()),
                    status_width
                ),
                status_color(document.status)
            ),
            painter.text(util::pad_right(
                &util::truncate(&leaf_name(document, grouped), document_width),
                document_width
            )),
            painter.dim(util::pad_right(
                &util::truncate(&document.title, title_width),
                title_width
            )),
            painter.info(pad_left(&util::format_size(document.size), SIZE_WIDTH)),
            painter.dim(util::format_relative(document.modified))
        ));
    }

    out.push(String::new());
    out
}

/// Totals for the documents actually printed, so a `--status` filter narrows
/// the summary as well as the table.
fn render_summary(documents: &[&SpecDocument], painter: Painter) -> Vec<String> {
    let bytes: u64 = documents.iter().map(|document| document.size).sum();
    let words: usize = documents.iter().map(|document| document.words).sum();
    let requirements: usize = documents.iter().map(|document| document.requirements).sum();

    let mut out = vec![format!(
        "{INDENT}{}",
        painter.bold(t!(
            "list.summary",
            files = documents.len(),
            size = util::format_size(bytes),
            words = words,
            requirements = requirements
        ))
    )];

    let badges: Vec<String> = SpecStatus::ALL
        .iter()
        .filter_map(|status| {
            let count = documents
                .iter()
                .filter(|document| document.status == *status)
                .count();
            (count > 0).then(|| {
                painter.colored(
                    format!("{} {} {count}", status.glyph(), status.label()),
                    status_color(*status),
                )
            })
        })
        .collect();

    if !badges.is_empty() {
        out.push(format!(
            "{INDENT}{} {}",
            painter.dim(format!("{}:", t!("list.by_status"))),
            badges.join(painter.dim("  ·  ").as_str())
        ));
    }

    out.push(String::new());
    out
}

fn render_changes(workspace: &Workspace, painter: Painter, width: u16) -> Vec<String> {
    let mut out = vec![
        String::new(),
        format!(
            "  {} {}",
            painter.accent_bold("◆"),
            painter.bold(t!("list.changes_title"))
        ),
        String::new(),
    ];

    if workspace.changes.is_empty() {
        let dir = workspace
            .changes_dir()
            .strip_prefix(&workspace.root)
            .map(|relative| relative.display().to_string())
            .unwrap_or_else(|_| workspace.changes_dir().display().to_string());
        out.push(format!(
            "{INDENT}{}",
            painter.dim(t!("list.changes_empty", dir = dir))
        ));
        out.push(String::new());
        return out;
    }

    let id_width = workspace
        .changes
        .iter()
        .map(|change| change.id.chars().count())
        .max()
        .unwrap_or(8)
        .clamp(8, MAX_DOCUMENT_WIDTH);

    let bar_width = ((width as usize).saturating_sub(INDENT.len() + id_width + 40)).clamp(8, 24);

    for change in &workspace.changes {
        let progress = change.progress();
        let bar = util::bar(progress, bar_width);

        let mut tasks = if change.tasks_total > 0 {
            t!(
                "list.changes_progress",
                done = change.tasks_done,
                total = change.tasks_total
            )
            .to_string()
        } else {
            t!("list.changes_documents", count = change.documents.len()).to_string()
        };
        if change.archived {
            tasks.push_str(&format!(" · {}", t!("list.archived")));
        }

        out.push(format!(
            "{INDENT}{}  {}  {}{}  {}  {}",
            painter.colored(change.status.glyph(), status_color(change.status)),
            painter.text(util::pad_right(
                &util::truncate(&change.id, id_width),
                id_width
            )),
            painter.colored(bar.filled, progress_color(progress)),
            painter.dim(bar.track),
            painter.dim(pad_left(&format!("{:.0}%", progress * 100.0), 4)),
            painter.dim(tasks)
        ));
    }

    out.push(String::new());
    out
}

fn sort_documents(documents: &mut [&SpecDocument], key: SortKey) {
    match key {
        // Keeps every directory contiguous so the grouped view reads as a tree.
        SortKey::Path => documents.sort_by(|left, right| {
            left.group()
                .unwrap_or_default()
                .to_lowercase()
                .cmp(&right.group().unwrap_or_default().to_lowercase())
                .then_with(|| {
                    left.file_name
                        .to_lowercase()
                        .cmp(&right.file_name.to_lowercase())
                })
        }),
        SortKey::Name => documents.sort_by(|left, right| {
            left.file_name
                .to_lowercase()
                .cmp(&right.file_name.to_lowercase())
        }),
        SortKey::Title => documents.sort_by_key(|document| document.title.to_lowercase()),
        SortKey::Size => documents.sort_by_key(|document| Reverse(document.size)),
        SortKey::Status => documents.sort_by(|left, right| {
            left.status
                .cmp(&right.status)
                .then_with(|| left.relative.cmp(&right.relative))
        }),
        SortKey::Updated => documents.sort_by_key(|document| Reverse(document.modified)),
    }
}

/// File name inside its group heading, or the full relative path when the
/// listing is flat.
fn leaf_name(document: &SpecDocument, grouped: bool) -> String {
    if grouped {
        document.file_name.clone()
    } else {
        document.display_path()
    }
}

pub fn status_color(status: SpecStatus) -> Color {
    match status {
        SpecStatus::Draft => palette::INFO,
        SpecStatus::Review => palette::WARNING,
        SpecStatus::Approved => palette::PRIMARY,
        SpecStatus::Implemented => palette::SUCCESS,
        SpecStatus::Deprecated => palette::DANGER,
        SpecStatus::Unknown => palette::MUTED,
    }
}

fn progress_color(progress: f64) -> Color {
    if progress >= 0.999 {
        palette::SUCCESS
    } else if progress >= 0.5 {
        palette::PRIMARY
    } else if progress > 0.0 {
        palette::WARNING
    } else {
        palette::MUTED
    }
}

fn pad_left(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.to_string();
    }
    let mut out = String::with_capacity(width);
    out.extend(std::iter::repeat_n(' ', width - len));
    out.push_str(text);
    out
}

fn terminal_width() -> u16 {
    crossterm::terminal::size()
        .map(|(columns, _)| columns)
        .unwrap_or(FALLBACK_WIDTH)
        .max(MIN_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cli::StatusFilter;
    use crate::commands::tests::{Fixture, plain};

    fn args(all: bool, status: Option<StatusFilter>, sort: SortKey) -> ListArgs {
        ListArgs { all, status, sort }
    }

    fn default_args() -> ListArgs {
        args(false, None, SortKey::Path)
    }

    /// The row a document occupies, found by the leaf the table shows for it.
    fn row<'a>(lines: &'a [String], needle: &str) -> &'a str {
        lines
            .iter()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("no row for {needle} in {lines:#?}"))
    }

    #[test]
    fn the_header_names_the_workspace_and_the_project() {
        let fixture = Fixture::new();
        let lines = render_header(&fixture.workspace(), plain());

        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "");
        assert!(lines[1].contains("Specifications"));
        assert!(lines[2].contains("Workspace"));
        assert!(lines[2].contains("openspectation-cmd"));
        assert!(lines[3].contains("Demo Project"));
        assert_eq!(lines[4], "");
    }

    #[test]
    fn an_empty_specs_directory_explains_itself_instead_of_printing_a_table() {
        let fixture = Fixture::bare();
        let lines = render(&fixture.workspace(), &default_args(), plain(), 100);

        assert!(lines.iter().any(|line| line.contains("No specification")));
        assert!(lines.iter().any(|line| line.contains("specs/")));
        assert!(
            !lines.iter().any(|line| line.contains("DOCUMENT")),
            "an empty workspace must not print column headers"
        );
    }

    #[test]
    fn a_filter_that_matches_nothing_names_the_status_it_filtered_on() {
        let fixture = Fixture::new();
        let lines = render(
            &fixture.workspace(),
            &args(false, Some(StatusFilter::Deprecated), SortKey::Path),
            plain(),
            100,
        );

        let message = row(&lines, "No document matches");
        assert!(message.contains("deprecated"));
        assert!(!lines.iter().any(|line| line.contains("Billing")));
    }

    #[test]
    fn the_table_groups_documents_by_directory_only_when_sorted_by_path() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        let grouped = render(&workspace, &default_args(), plain(), 100);
        assert!(grouped.iter().any(|line| line.contains("▸ auth")));
        assert!(grouped.iter().any(|line| line.contains("▸ specs")));
        // Inside a group the row carries the file name alone.
        assert!(row(&grouped, "tokens.md").contains("tokens.md  "));

        let flat = render(&workspace, &args(false, None, SortKey::Name), plain(), 100);
        assert!(!flat.iter().any(|line| line.contains("▸ ")));
        assert!(row(&flat, "tokens.md").contains("auth/tokens.md"));
    }

    #[test]
    fn the_summary_totals_only_the_documents_on_screen() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        let everything = render(&workspace, &default_args(), plain(), 100);
        assert!(row(&everything, "documents ·").starts_with("    4 documents ·"));
        assert!(row(&everything, "By status").contains("approved 1"));

        let approved = render(
            &workspace,
            &args(false, Some(StatusFilter::Approved), SortKey::Path),
            plain(),
            100,
        );
        assert!(row(&approved, "documents ·").starts_with("    1 documents ·"));
        assert!(!approved.iter().any(|line| line.contains("draft 1")));
    }

    #[test]
    fn the_title_column_grows_with_the_terminal_and_stops_at_its_bounds() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        // The title column runs from its own heading to the fixed pair that
        // closes the row: two spaces, the size column, two more spaces.
        let title_width = |width: u16| {
            let lines = render(&workspace, &default_args(), plain(), width);
            let header = row(&lines, "TITLE").to_string();
            let column = |needle: &str| {
                let byte = header.find(needle).expect(needle);
                header[..byte].chars().count()
            };
            column("UPDATED") - column("TITLE") - (2 + SIZE_WIDTH + 2)
        };

        assert_eq!(title_width(MIN_WIDTH), MIN_TITLE_WIDTH);
        assert_eq!(title_width(400), MAX_TITLE_WIDTH);

        let between = title_width(90);
        assert!(between > MIN_TITLE_WIDTH && between < MAX_TITLE_WIDTH);
    }

    #[test]
    fn the_changes_section_is_printed_only_with_the_all_flag() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        let without = render(&workspace, &default_args(), plain(), 100);
        assert!(!without.iter().any(|line| line.contains("Change proposals")));

        let with = render(&workspace, &args(true, None, SortKey::Path), plain(), 100);
        assert!(with.iter().any(|line| line.contains("Change proposals")));
        assert!(row(&with, "add-oauth").contains("1/2 tasks"));
        assert!(row(&with, "add-oauth").contains("50%"));
        // An archived proposal says so and counts its documents instead.
        let old = row(&with, "old-thing");
        assert!(old.contains("archived"));
        assert!(old.contains("1 documents"));
    }

    #[test]
    fn a_workspace_without_changes_says_so() {
        let fixture = Fixture::bare();
        let lines = render_changes(&fixture.workspace(), plain(), 100);

        assert!(
            lines
                .iter()
                .any(|line| line.contains("No change proposals"))
        );
        assert!(lines.iter().any(|line| line.contains("changes")));
    }

    #[test]
    fn every_sort_key_orders_the_documents_it_names() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        let sorted = |key: SortKey| {
            let mut documents: Vec<&SpecDocument> = workspace.specs.iter().collect();
            sort_documents(&mut documents, key);
            documents
        };
        let names = |key: SortKey| {
            sorted(key)
                .iter()
                .map(|document| document.display_path())
                .collect::<Vec<_>>()
        };

        // Path keeps every directory contiguous, which is what makes the
        // grouped table read as a tree; name ignores directories entirely.
        assert_eq!(
            names(SortKey::Path),
            [
                "billing.md",
                "workflow.md",
                "auth/spec.md",
                "auth/tokens.md"
            ]
        );
        assert_eq!(
            names(SortKey::Name),
            [
                "billing.md",
                "auth/spec.md",
                "auth/tokens.md",
                "workflow.md"
            ]
        );
        assert_eq!(
            names(SortKey::Title),
            [
                "auth/spec.md",
                "billing.md",
                "auth/tokens.md",
                "workflow.md"
            ]
        );
        // Draft, in review, approved, implemented: the lifecycle order.
        assert_eq!(
            names(SortKey::Status),
            [
                "auth/spec.md",
                "workflow.md",
                "billing.md",
                "auth/tokens.md"
            ]
        );

        assert!(
            sorted(SortKey::Size)
                .windows(2)
                .all(|pair| pair[0].size >= pair[1].size)
        );
        assert!(
            sorted(SortKey::Updated)
                .windows(2)
                .all(|pair| pair[0].modified >= pair[1].modified)
        );
    }

    #[test]
    fn a_status_keeps_a_colour_of_its_own() {
        let colors: Vec<Color> = SpecStatus::ALL.iter().copied().map(status_color).collect();
        for (index, color) in colors.iter().enumerate() {
            assert!(
                !colors[index + 1..].contains(color),
                "{:?} shares its colour",
                SpecStatus::ALL[index]
            );
        }
    }

    #[test]
    fn the_progress_colour_moves_through_its_four_bands() {
        assert_eq!(progress_color(0.0), palette::MUTED);
        assert_eq!(progress_color(0.01), palette::WARNING);
        assert_eq!(progress_color(0.49), palette::WARNING);
        assert_eq!(progress_color(0.5), palette::PRIMARY);
        assert_eq!(progress_color(0.99), palette::PRIMARY);
        assert_eq!(progress_color(1.0), palette::SUCCESS);
    }

    #[test]
    fn padding_left_aligns_to_the_right_and_never_truncates() {
        assert_eq!(pad_left("9 B", 6), "   9 B");
        assert_eq!(pad_left("exact", 5), "exact");
        assert_eq!(pad_left("far too long", 4), "far too long");
        // Width is counted in characters, not bytes.
        assert_eq!(pad_left("ñ", 3).chars().count(), 3);
    }

    #[test]
    fn a_leaf_name_drops_its_directory_only_in_a_grouped_listing() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();
        let nested = workspace
            .specs
            .iter()
            .find(|document| document.display_path() == "auth/tokens.md")
            .unwrap();

        assert_eq!(leaf_name(nested, true), "tokens.md");
        assert_eq!(leaf_name(nested, false), "auth/tokens.md");
    }

    /// The layout collides below this, whether the width came from the
    /// terminal or from the fallback used when it cannot be queried.
    #[test]
    fn the_reported_width_never_drops_below_the_table_minimum() {
        const { assert!(FALLBACK_WIDTH >= MIN_WIDTH) };
        assert!(terminal_width() >= MIN_WIDTH);
    }

    #[test]
    fn the_command_refuses_a_directory_that_is_not_a_workspace() {
        let fixture = Fixture::scratch();
        let outcome = run(&fixture.root, &default_args(), plain());

        assert!(matches!(
            outcome,
            Err(crate::error::Error::NotAWorkspace(_))
        ));
        assert!(run(&fixture.root.join("nowhere"), &default_args(), plain()).is_err());
    }

    #[test]
    fn the_command_prints_the_listing_it_renders() {
        let fixture = Fixture::new();
        assert!(run(&fixture.root, &args(true, None, SortKey::Size), plain()).is_ok());
    }
}
