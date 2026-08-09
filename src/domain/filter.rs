//! Filter expressions shared by the search box and the `:filter` command.
//!
//! Plain text stays plain text — most filtering is "show me the thing whose
//! name I half remember". The structured forms exist because "what is not
//! finished" and "what is still a draft" are the two questions a spec workspace
//! is actually asked.

use rust_i18n::t;

use crate::domain::change::ChangeProposal;
use crate::domain::spec::{SpecDocument, SpecStatus};

/// Comparison used by a `tasks` predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Less,
    LessOrEqual,
    Equal,
    GreaterOrEqual,
    Greater,
}

impl Comparison {
    fn parse(raw: &str) -> Option<(Self, &str)> {
        for (token, comparison) in [
            ("<=", Self::LessOrEqual),
            (">=", Self::GreaterOrEqual),
            ("==", Self::Equal),
            ("<", Self::Less),
            (">", Self::Greater),
            ("=", Self::Equal),
        ] {
            if let Some(rest) = raw.strip_prefix(token) {
                return Some((comparison, rest));
            }
        }
        None
    }

    fn holds(self, left: f64, right: f64) -> bool {
        // Percentages come from integer ratios, so an epsilon keeps `=100%`
        // from missing a change whose division lands a hair below one.
        const EPSILON: f64 = 1e-9;
        match self {
            Self::Less => left < right - EPSILON,
            Self::LessOrEqual => left <= right + EPSILON,
            Self::Equal => (left - right).abs() <= EPSILON,
            Self::GreaterOrEqual => left >= right - EPSILON,
            Self::Greater => left > right + EPSILON,
        }
    }
}

/// A parsed filter expression.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Filter {
    #[default]
    All,
    Text(String),
    Status(SpecStatus),
    /// Completion ratio in the `0.0..=1.0` range.
    Tasks(Comparison, f64),
    Archived(bool),
}

impl Filter {
    /// Reads an expression, falling back to a plain substring match.
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Self::All;
        }

        let lowered = trimmed.to_lowercase();

        if let Some(rest) = lowered.strip_prefix("status:")
            && let status = SpecStatus::parse(rest)
            && status != SpecStatus::Unknown
        {
            return Self::Status(status);
        }

        if let Some(rest) = lowered.strip_prefix("tasks")
            && let Some((comparison, value)) = Comparison::parse(rest)
            && let Some(ratio) = ratio(value)
        {
            return Self::Tasks(comparison, ratio);
        }

        match lowered.as_str() {
            "archived" => Self::Archived(true),
            "active" => Self::Archived(false),
            _ => Self::Text(lowered),
        }
    }

    pub const fn is_active(&self) -> bool {
        !matches!(self, Self::All)
    }

    /// Localized description shown next to the match counter.
    pub fn describe(&self) -> String {
        match self {
            Self::All => t!("filter.all").to_string(),
            Self::Text(needle) => t!("filter.text", value = needle).to_string(),
            Self::Status(status) => t!("filter.status", value = status.label()).to_string(),
            Self::Tasks(comparison, ratio) => t!(
                "filter.tasks",
                comparison = symbol(*comparison),
                value = format!("{:.0}%", ratio * 100.0)
            )
            .to_string(),
            Self::Archived(true) => t!("filter.archived").to_string(),
            Self::Archived(false) => t!("filter.active").to_string(),
        }
    }

    pub fn matches_change(&self, change: &ChangeProposal) -> bool {
        match self {
            Self::All => true,
            Self::Text(needle) => change.matches(needle),
            Self::Status(status) => change.status == *status,
            Self::Tasks(comparison, ratio) => comparison.holds(change.progress(), *ratio),
            Self::Archived(archived) => change.archived == *archived,
        }
    }

    pub fn matches_spec(&self, document: &SpecDocument) -> bool {
        match self {
            Self::All => true,
            Self::Text(needle) => document.matches(needle),
            Self::Status(status) => document.status == *status,
            Self::Tasks(comparison, ratio) => {
                let progress = if document.tasks_total == 0 {
                    0.0
                } else {
                    document.tasks_done as f64 / document.tasks_total as f64
                };
                comparison.holds(progress, *ratio)
            }
            // A specification is never archived, so an archive filter hides
            // the whole list rather than pretending every entry qualifies.
            Self::Archived(archived) => !*archived,
        }
    }
}

/// Reads `100%`, `0.5` or `50` as a ratio in `0.0..=1.0`.
fn ratio(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if let Some(percent) = trimmed.strip_suffix('%') {
        return percent
            .trim()
            .parse::<f64>()
            .ok()
            .map(|value| value / 100.0);
    }

    let value = trimmed.parse::<f64>().ok()?;
    Some(if value > 1.0 { value / 100.0 } else { value })
}

const fn symbol(comparison: Comparison) -> &'static str {
    match comparison {
        Comparison::Less => "<",
        Comparison::LessOrEqual => "≤",
        Comparison::Equal => "=",
        Comparison::GreaterOrEqual => "≥",
        Comparison::Greater => ">",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_stays_plain_text() {
        assert_eq!(Filter::parse("bill"), Filter::Text("bill".to_string()));
        assert_eq!(Filter::parse("   "), Filter::All);
    }

    #[test]
    fn reads_the_task_predicate_from_the_landing() {
        assert_eq!(
            Filter::parse("tasks<100%"),
            Filter::Tasks(Comparison::Less, 1.0)
        );
        assert_eq!(
            Filter::parse("tasks>=50"),
            Filter::Tasks(Comparison::GreaterOrEqual, 0.5)
        );
    }

    #[test]
    fn reads_status_and_archive_predicates() {
        assert_eq!(
            Filter::parse("status:draft"),
            Filter::Status(SpecStatus::Draft)
        );
        assert_eq!(Filter::parse("archived"), Filter::Archived(true));
        assert_eq!(Filter::parse("active"), Filter::Archived(false));
    }

    #[test]
    fn an_unknown_status_is_treated_as_text() {
        assert_eq!(
            Filter::parse("status:nonsense"),
            Filter::Text("status:nonsense".to_string())
        );
    }

    #[test]
    fn a_complete_ratio_compares_equal() {
        assert!(Comparison::Equal.holds(39.0 / 39.0, 1.0));
        assert!(Comparison::Less.holds(37.0 / 39.0, 1.0));
    }

    use std::path::{Path, PathBuf};

    fn document(name: &str, tasks: &str) -> SpecDocument {
        SpecDocument::from_memory(
            &PathBuf::from(format!("/w/changes/add-oauth/{name}")),
            Path::new("/w/changes"),
            "Add OAuth".to_string(),
            tasks.to_string(),
            None,
        )
    }

    fn change(tasks: &str, archived: bool) -> ChangeProposal {
        ChangeProposal::assemble(
            Path::new("/w/changes/add-oauth"),
            vec![document("tasks.md", tasks)],
            false,
            archived,
        )
        .unwrap()
    }

    #[test]
    fn every_comparison_token_is_recognized() {
        for (expression, expected) in [
            ("tasks<50", Comparison::Less),
            ("tasks<=50", Comparison::LessOrEqual),
            ("tasks=50", Comparison::Equal),
            ("tasks==50", Comparison::Equal),
            ("tasks>=50", Comparison::GreaterOrEqual),
            ("tasks>50", Comparison::Greater),
        ] {
            assert_eq!(
                Filter::parse(expression),
                Filter::Tasks(expected, 0.5),
                "failed on {expression}"
            );
        }
    }

    #[test]
    fn each_comparison_holds_only_where_it_should() {
        assert!(Comparison::Less.holds(0.4, 0.5));
        assert!(!Comparison::Less.holds(0.5, 0.5));
        assert!(Comparison::LessOrEqual.holds(0.5, 0.5));
        assert!(Comparison::GreaterOrEqual.holds(0.5, 0.5));
        assert!(Comparison::Greater.holds(0.6, 0.5));
        assert!(!Comparison::Greater.holds(0.5, 0.5));
        assert!(!Comparison::Equal.holds(0.4, 0.5));
    }

    #[test]
    fn a_ratio_is_read_as_a_percentage_a_fraction_or_a_bare_number() {
        assert_eq!(ratio("100%"), Some(1.0));
        assert_eq!(ratio(" 50 % "), Some(0.5));
        assert_eq!(ratio("0.25"), Some(0.25));
        assert_eq!(ratio("25"), Some(0.25));
        assert_eq!(ratio("1"), Some(1.0));
        assert_eq!(ratio("nonsense"), None);
        assert_eq!(ratio("%"), None);
    }

    #[test]
    fn a_malformed_predicate_degrades_to_a_text_search() {
        assert_eq!(
            Filter::parse("tasks<nonsense"),
            Filter::Text("tasks<nonsense".to_string())
        );
        assert_eq!(
            Filter::parse("tasks50"),
            Filter::Text("tasks50".to_string())
        );
    }

    #[test]
    fn an_expression_is_matched_case_insensitively() {
        assert_eq!(Filter::parse("ARCHIVED"), Filter::Archived(true));
        assert_eq!(
            Filter::parse("STATUS:Draft"),
            Filter::Status(SpecStatus::Draft)
        );
        assert_eq!(Filter::parse("BiLl"), Filter::Text("bill".to_string()));
    }

    #[test]
    fn only_the_empty_filter_is_inactive() {
        assert!(!Filter::default().is_active());
        assert!(!Filter::All.is_active());
        assert!(Filter::Text("x".into()).is_active());
        assert!(Filter::Archived(false).is_active());
        assert!(Filter::Tasks(Comparison::Less, 1.0).is_active());
    }

    #[test]
    fn every_variant_describes_itself_without_leaking_a_missing_key() {
        for filter in [
            Filter::All,
            Filter::Text("bill".into()),
            Filter::Status(SpecStatus::Draft),
            Filter::Tasks(Comparison::GreaterOrEqual, 0.5),
            Filter::Archived(true),
            Filter::Archived(false),
        ] {
            let described = filter.describe();
            assert!(!described.trim().is_empty(), "empty for {filter:?}");
            assert!(!described.contains("filter."), "raw key in {described:?}");
        }
        assert!(Filter::Text("bill".into()).describe().contains("bill"));
        assert!(
            Filter::Tasks(Comparison::GreaterOrEqual, 0.5)
                .describe()
                .contains("50%")
        );
    }

    #[test]
    fn each_comparison_has_its_own_symbol() {
        let symbols: Vec<&str> = [
            Comparison::Less,
            Comparison::LessOrEqual,
            Comparison::Equal,
            Comparison::GreaterOrEqual,
            Comparison::Greater,
        ]
        .into_iter()
        .map(symbol)
        .collect();
        assert_eq!(symbols, ["<", "≤", "=", "≥", ">"]);
    }

    #[test]
    fn the_empty_filter_matches_everything() {
        let change = change("- [ ] one\n", false);
        assert!(Filter::All.matches_change(&change));
        assert!(Filter::All.matches_spec(&change.documents[0]));
    }

    #[test]
    fn a_task_predicate_reads_the_completion_of_a_change() {
        let half = change("- [x] one\n- [ ] two\n", false);
        assert!(Filter::Tasks(Comparison::Equal, 0.5).matches_change(&half));
        assert!(Filter::Tasks(Comparison::Less, 1.0).matches_change(&half));
        assert!(!Filter::Tasks(Comparison::Greater, 0.5).matches_change(&half));
    }

    #[test]
    fn a_document_without_tasks_counts_as_zero_rather_than_dividing_by_zero() {
        let change = change("Plain prose, no checkboxes.\n", false);
        let document = &change.documents[0];
        assert!(Filter::Tasks(Comparison::Equal, 0.0).matches_spec(document));
        assert!(!Filter::Tasks(Comparison::Greater, 0.0).matches_spec(document));
    }

    #[test]
    fn the_archive_predicate_separates_shipped_from_active_changes() {
        let active = change("- [ ] one\n", false);
        let shipped = change("- [ ] one\n", true);
        assert!(Filter::Archived(false).matches_change(&active));
        assert!(!Filter::Archived(true).matches_change(&active));
        assert!(Filter::Archived(true).matches_change(&shipped));
    }

    #[test]
    fn an_archive_filter_empties_the_specification_list() {
        // Specifications are never archived, so the honest answer is "none",
        // not "all of them".
        let change = change("- [ ] one\n", false);
        let document = &change.documents[0];
        assert!(!Filter::Archived(true).matches_spec(document));
        assert!(Filter::Archived(false).matches_spec(document));
    }

    #[test]
    fn a_text_filter_reaches_the_identifier_and_the_title() {
        let change = change("- [ ] one\n", false);
        assert!(Filter::parse("oauth").matches_change(&change));
        assert!(Filter::parse("add").matches_change(&change));
        assert!(!Filter::parse("billing").matches_change(&change));
    }

    #[test]
    fn a_status_filter_keeps_only_its_own_status() {
        let change = change("- [ ] one\n", false);
        assert!(Filter::Status(change.status).matches_change(&change));
        assert!(Filter::Status(change.documents[0].status).matches_spec(&change.documents[0]));

        let other = SpecStatus::ALL
            .iter()
            .find(|status| **status != change.status)
            .unwrap();
        assert!(!Filter::Status(*other).matches_change(&change));
    }
}
