//! Change proposals stored under `changes/`.
//!
//! Two shapes are supported:
//!
//! * a directory per change (`changes/add-oauth/{proposal,tasks,design}.md`),
//! * a single file per change (`changes/add-oauth.md`).
//!
//! Proposals under `changes/archive/` have already been deployed and are kept
//! apart from the active ones.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::domain::spec::{SpecDocument, SpecStatus};
use crate::domain::util;

/// Document names promoted to the summary view, in priority order.
const PRIMARY_DOCUMENTS: [&str; 4] = ["proposal.md", "README.md", "design.md", "spec.md"];

#[derive(Debug, Clone)]
pub struct ChangeProposal {
    /// Directory or file stem identifying the change.
    pub id: String,
    pub path: PathBuf,
    pub title: String,
    pub status: SpecStatus,
    pub documents: Vec<SpecDocument>,
    pub tasks_total: usize,
    pub tasks_done: usize,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// `true` when the proposal is a single Markdown file.
    pub single_file: bool,
    /// `true` when the proposal lives under `changes/archive/`.
    pub archived: bool,
}

impl ChangeProposal {
    /// Builds a proposal from the documents discovered inside `path`.
    pub fn assemble(
        path: &Path,
        documents: Vec<SpecDocument>,
        single_file: bool,
        archived: bool,
    ) -> Option<Self> {
        if documents.is_empty() {
            return None;
        }

        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();

        let primary = pick_primary(&documents);

        let title = primary
            .and_then(|document| {
                document
                    .frontmatter
                    .title
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .or_else(|| top_heading(&document.body))
            })
            .unwrap_or_else(|| humanize_id(&id));

        let tasks_total = documents.iter().map(|document| document.tasks_total).sum();
        let tasks_done = documents.iter().map(|document| document.tasks_done).sum();

        let status = primary
            .map(|document| document.status)
            .filter(|status| *status != SpecStatus::Unknown)
            .or_else(|| {
                documents
                    .iter()
                    .map(|document| document.status)
                    .find(|status| *status != SpecStatus::Unknown)
            })
            .unwrap_or_else(|| derive_status(archived, tasks_total, tasks_done));

        let size = documents.iter().map(|document| document.size).sum();
        let modified = documents
            .iter()
            .filter_map(|document| document.modified)
            .max();

        Some(Self {
            id,
            path: path.to_path_buf(),
            title,
            status,
            documents,
            tasks_total,
            tasks_done,
            size,
            modified,
            single_file,
            archived,
        })
    }

    /// Completion ratio in the `0.0..=1.0` range.
    ///
    /// Archived proposals are not forced to 100%: their task list is what it
    /// was when they shipped, and reporting anything else would be a fiction.
    pub fn progress(&self) -> f64 {
        if self.tasks_total == 0 {
            return match self.status {
                SpecStatus::Implemented => 1.0,
                SpecStatus::Approved => 0.75,
                SpecStatus::Review => 0.5,
                SpecStatus::Draft => 0.15,
                _ => 0.0,
            };
        }
        self.tasks_done as f64 / self.tasks_total as f64
    }

    /// Case insensitive match used by the TUI filter.
    pub fn matches(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_lowercase();
        self.id.to_lowercase().contains(&needle)
            || self.title.to_lowercase().contains(&needle)
            || self.status.key().contains(needle.as_str())
            || (self.archived && "archived".contains(needle.as_str()))
    }
}

/// Human readable form of a change identifier.
///
/// Archived proposals are prefixed with the date they shipped. That prefix is
/// kept verbatim rather than title-cased, so the list still reads as a
/// timeline instead of as four stray numbers.
fn humanize_id(id: &str) -> String {
    match split_date_prefix(id) {
        Some((date, rest)) => format!("{date} · {}", util::humanize(rest)),
        None => util::humanize(id),
    }
}

/// Splits a leading `YYYY-MM-DD-` prefix from a change identifier.
fn split_date_prefix(id: &str) -> Option<(&str, &str)> {
    let (date, rest) = id.split_at_checked(10)?;
    let rest = rest.strip_prefix('-')?;
    if rest.is_empty() {
        return None;
    }

    let mut parts = date.split('-');
    let shaped = matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(year), Some(month), Some(day), None)
            if year.len() == 4
                && month.len() == 2
                && day.len() == 2
                && [year, month, day]
                    .iter()
                    .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
    );

    shaped.then_some((date, rest))
}

/// First level-one heading of a document.
///
/// A deeper heading names a section, not the document: change proposals
/// routinely open with `## Why`, and taking that as the proposal title makes
/// every change in a workspace look identical.
fn top_heading(body: &str) -> Option<String> {
    let mut in_code_fence = false;

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("# ")
            && !heading.trim().is_empty()
        {
            return Some(heading.trim().to_string());
        }
    }

    None
}

/// Status of a proposal that declares none, inferred from its task list.
fn derive_status(archived: bool, tasks_total: usize, tasks_done: usize) -> SpecStatus {
    if archived {
        return SpecStatus::Implemented;
    }
    match (tasks_total, tasks_done) {
        (0, _) => SpecStatus::Unknown,
        (total, done) if done >= total => SpecStatus::Implemented,
        (_, 0) => SpecStatus::Draft,
        _ => SpecStatus::Review,
    }
}

fn pick_primary(documents: &[SpecDocument]) -> Option<&SpecDocument> {
    for wanted in PRIMARY_DOCUMENTS {
        if let Some(found) = documents
            .iter()
            .find(|document| document.file_name.eq_ignore_ascii_case(wanted))
        {
            return Some(found);
        }
    }
    documents.first()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_level_one_heading_names_a_proposal() {
        assert_eq!(top_heading("## Why\n\nBecause.\n"), None);
        assert_eq!(
            top_heading("## Why\n\n# Add OAuth\n").as_deref(),
            Some("Add OAuth")
        );
    }

    #[test]
    fn ignores_headings_inside_code_fences() {
        assert_eq!(top_heading("```sh\n# not a heading\n```\n"), None);
    }

    #[test]
    fn keeps_the_date_prefix_of_an_archived_identifier() {
        assert_eq!(
            humanize_id("2026-08-06-secure-set-html"),
            "2026-08-06 · Secure Set Html"
        );
        assert_eq!(humanize_id("add-oauth-login"), "Add Oauth Login");
        assert_eq!(humanize_id("2026-08-06"), "2026 08 06");
    }

    #[test]
    fn a_date_prefix_is_only_split_off_when_it_is_shaped_like_one() {
        assert_eq!(
            split_date_prefix("2026-08-06-thing"),
            Some(("2026-08-06", "thing"))
        );
        assert_eq!(split_date_prefix("2026-8-6-thing"), None);
        assert_eq!(split_date_prefix("20xx-08-06-thing"), None);
        assert_eq!(split_date_prefix("2026-08-06"), None);
        assert_eq!(split_date_prefix("short"), None);
        assert_eq!(split_date_prefix("2026-08-06-"), None);
    }

    #[test]
    fn a_tilde_fence_hides_a_heading_too() {
        assert_eq!(top_heading("~~~\n# not a heading\n~~~\n"), None);
        assert_eq!(top_heading("#\n\n# Real\n").as_deref(), Some("Real"));
        assert_eq!(top_heading("no headings here\n"), None);
    }

    #[test]
    fn an_archived_proposal_is_implemented_whatever_its_task_list_says() {
        assert_eq!(derive_status(true, 0, 0), SpecStatus::Implemented);
        assert_eq!(derive_status(true, 4, 1), SpecStatus::Implemented);
    }

    #[test]
    fn the_task_list_infers_the_status_of_an_active_proposal() {
        assert_eq!(derive_status(false, 0, 0), SpecStatus::Unknown);
        assert_eq!(derive_status(false, 3, 0), SpecStatus::Draft);
        assert_eq!(derive_status(false, 3, 1), SpecStatus::Review);
        assert_eq!(derive_status(false, 3, 3), SpecStatus::Implemented);
    }

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempChange(PathBuf);

    impl TempChange {
        fn new(id: &str) -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "openspectation-change-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join(id)).unwrap();
            Self(root.join(id))
        }

        fn write(&self, name: &str, contents: &str) {
            std::fs::write(self.0.join(name), contents).unwrap();
        }

        fn assemble(&self, archived: bool) -> ChangeProposal {
            let base = self.0.parent().unwrap();
            let documents =
                crate::domain::scanner::scan_specs_from(&self.0, base, &Default::default())
                    .unwrap();
            ChangeProposal::assemble(&self.0, documents, false, archived).unwrap()
        }
    }

    impl Drop for TempChange {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
        }
    }

    #[test]
    fn a_proposal_without_documents_is_not_a_proposal() {
        assert!(
            ChangeProposal::assemble(Path::new("/w/changes/empty"), Vec::new(), false, false)
                .is_none()
        );
    }

    #[test]
    fn the_proposal_document_names_the_change_and_the_others_only_add_up() {
        let temp = TempChange::new("add-oauth");
        temp.write("proposal.md", "# Add OAuth Login\n");
        temp.write("tasks.md", "- [x] one\n- [ ] two\n- [ ] three\n");

        let change = temp.assemble(false);
        assert_eq!(change.id, "add-oauth");
        assert_eq!(change.title, "Add OAuth Login");
        assert_eq!((change.tasks_done, change.tasks_total), (1, 3));
        assert_eq!(change.documents.len(), 2);
        assert!(change.size > 0);
        assert!(change.modified.is_some());
    }

    #[test]
    fn a_proposal_with_no_heading_falls_back_to_its_identifier() {
        let temp = TempChange::new("2026-08-06-secure-html");
        temp.write("proposal.md", "Prose without a heading.\n");

        assert_eq!(temp.assemble(true).title, "2026-08-06 · Secure Html");
    }

    #[test]
    fn declared_frontmatter_outranks_a_derived_status() {
        let temp = TempChange::new("add-oauth");
        temp.write("proposal.md", "---\nstatus: approved\n---\n# Add OAuth\n");
        temp.write("tasks.md", "- [ ] one\n");

        // Without the declaration the empty task list would derive `Draft`.
        assert_eq!(temp.assemble(false).status, SpecStatus::Approved);
    }

    #[test]
    fn an_archived_proposal_reports_the_progress_it_shipped_with() {
        // Forcing it to 100% would rewrite history the task list contradicts.
        let temp = TempChange::new("2026-08-06-secure-html");
        temp.write("proposal.md", "# Secure HTML\n");
        temp.write("tasks.md", "- [x] one\n- [ ] two\n");

        let change = temp.assemble(true);
        assert!(change.archived);
        assert!((change.progress() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_proposal_without_tasks_reads_its_progress_off_its_status() {
        let mut change = ChangeProposal::assemble(
            Path::new("/w/changes/add-oauth"),
            vec![SpecDocument::from_memory(
                Path::new("/w/changes/add-oauth/proposal.md"),
                Path::new("/w/changes"),
                "Add OAuth".to_string(),
                "Prose only.\n".to_string(),
                None,
            )],
            true,
            false,
        )
        .unwrap();

        for (status, expected) in [
            (SpecStatus::Implemented, 1.0),
            (SpecStatus::Approved, 0.75),
            (SpecStatus::Review, 0.5),
            (SpecStatus::Draft, 0.15),
            (SpecStatus::Deprecated, 0.0),
            (SpecStatus::Unknown, 0.0),
        ] {
            change.status = status;
            assert!(
                (change.progress() - expected).abs() < 1e-9,
                "{status:?} reported {}",
                change.progress()
            );
        }
    }

    #[test]
    fn a_search_reaches_the_identifier_the_title_and_the_archive_flag() {
        let temp = TempChange::new("add-oauth");
        temp.write("proposal.md", "# Add OAuth Login\n");
        temp.write("tasks.md", "- [ ] one\n");

        let change = temp.assemble(true);
        for needle in ["oauth", "ADD", "login", "archived", ""] {
            assert!(change.matches(needle), "missed {needle:?}");
        }
        assert!(!change.matches("billing"));
    }

    #[test]
    fn the_primary_document_is_chosen_by_name_not_by_position() {
        let base = Path::new("/w/changes");
        let document = |name: &str, body: &str| {
            SpecDocument::from_memory(
                &PathBuf::from(format!("/w/changes/add-oauth/{name}")),
                base,
                name.to_string(),
                body.to_string(),
                None,
            )
        };

        let documents = vec![
            document("design.md", "# Design notes\n"),
            document("proposal.md", "# Add OAuth\n"),
        ];
        assert_eq!(pick_primary(&documents).unwrap().file_name, "proposal.md");

        // With none of the known names, the first document stands in.
        let others = vec![document("notes.md", "# Notes\n")];
        assert_eq!(pick_primary(&others).unwrap().file_name, "notes.md");
        assert!(pick_primary(&[]).is_none());
    }
}
