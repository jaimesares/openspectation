//! Recursive discovery of specification documents and change proposals.

use std::path::Path;

use walkdir::{DirEntry, WalkDir};

use crate::domain::change::ChangeProposal;
use crate::domain::config::WorkspaceConfig;
use crate::domain::spec::SpecDocument;
use crate::error::Result;

/// Hard cap on recursion depth, protecting against pathological trees.
const MAX_DEPTH: usize = 12;

/// Directories never descended into.
const IGNORED_DIRECTORIES: [&str; 6] = ["node_modules", "target", "dist", "build", ".git", ".venv"];

/// Files at the root of `changes/` that document the convention rather than
/// describing an actual proposal.
const CHANGES_NON_PROPOSALS: [&str; 2] = ["README.md", "index.md"];

/// Directory holding proposals that have already been deployed.
const ARCHIVE_DIR: &str = "archive";

/// Collects every specification document under `dir`.
///
/// A missing directory yields an empty result: an uninitialised layout is a
/// state to display, not a failure.
pub fn scan_specs(dir: &Path, config: &WorkspaceConfig) -> Result<Vec<SpecDocument>> {
    scan_specs_from(dir, dir, config)
}

/// Same as [`scan_specs`], but reports document paths relative to `base`.
///
/// The documents of a change proposal are scanned per proposal directory while
/// still being addressed from `changes/`, so `tasks.md` reads as
/// `add-oauth/tasks.md` wherever it is shown.
pub fn scan_specs_from(
    dir: &Path,
    base: &Path,
    config: &WorkspaceConfig,
) -> Result<Vec<SpecDocument>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut documents = Vec::new();

    for entry in walker(dir) {
        let entry = match entry {
            Ok(entry) => entry,
            // An unreadable entry must not abort the whole scan.
            Err(_) => continue,
        };

        if !entry.file_type().is_file() || !is_spec_file(&entry, config) {
            continue;
        }

        documents.push(SpecDocument::load(entry.path(), base)?);
    }

    sort_documents(&mut documents);
    Ok(documents)
}

/// Collects every change proposal under `dir`, active ones first.
pub fn scan_changes(dir: &Path, config: &WorkspaceConfig) -> Result<Vec<ChangeProposal>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut proposals = collect_proposals(dir, config, false)?;

    let archive = dir.join(ARCHIVE_DIR);
    if archive.is_dir() {
        proposals.extend(collect_proposals(&archive, config, true)?);
    }

    proposals.sort_by(|left, right| {
        left.archived
            .cmp(&right.archived)
            .then_with(|| left.id.to_lowercase().cmp(&right.id.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(proposals)
}

/// Reads the direct children of a changes directory as individual proposals.
fn collect_proposals(
    dir: &Path,
    config: &WorkspaceConfig,
    archived: bool,
) -> Result<Vec<ChangeProposal>> {
    let mut proposals = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|source| crate::error::Error::io(dir, source))?
        .filter_map(std::result::Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            // The archive is scanned separately so its contents stay one
            // proposal each instead of collapsing into a single "archive".
            if is_ignored_name(&path) || (!archived && is_archive(&path)) {
                continue;
            }
            let documents = scan_specs_from(&path, dir, config)?;
            if let Some(proposal) = ChangeProposal::assemble(&path, documents, false, archived) {
                proposals.push(proposal);
            }
        } else if file_type.is_file()
            && has_spec_extension(&path, config)
            && !is_convention_document(&path)
        {
            let document = SpecDocument::load(&path, dir)?;
            if let Some(proposal) = ChangeProposal::assemble(&path, vec![document], true, archived)
            {
                proposals.push(proposal);
            }
        }
    }

    Ok(proposals)
}

fn is_archive(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(ARCHIVE_DIR))
}

/// Sorts documents by directory first, then by file name, case insensitively.
pub fn sort_documents(documents: &mut [SpecDocument]) {
    documents.sort_by(|left, right| {
        let left_group = left.group().unwrap_or_default().to_lowercase();
        let right_group = right.group().unwrap_or_default().to_lowercase();
        left_group
            .cmp(&right_group)
            .then_with(|| {
                left.file_name
                    .to_lowercase()
                    .cmp(&right.file_name.to_lowercase())
            })
            .then_with(|| left.relative.cmp(&right.relative))
    });
}

fn walker(dir: &Path) -> impl Iterator<Item = walkdir::Result<DirEntry>> {
    WalkDir::new(dir)
        .max_depth(MAX_DEPTH)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| !is_hidden(entry) && !is_ignored(entry))
}

fn is_hidden(entry: &DirEntry) -> bool {
    // Depth 0 is the scan root itself, which may legitimately be hidden.
    entry.depth() > 0
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with('.'))
}

fn is_ignored(entry: &DirEntry) -> bool {
    entry.file_type().is_dir() && is_ignored_name(entry.path())
}

fn is_convention_document(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| {
            CHANGES_NON_PROPOSALS
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(name))
        })
}

fn is_ignored_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| IGNORED_DIRECTORIES.contains(&name))
}

fn is_spec_file(entry: &DirEntry, config: &WorkspaceConfig) -> bool {
    has_spec_extension(entry.path(), config)
}

fn has_spec_extension(path: &Path, config: &WorkspaceConfig) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| config.is_spec_extension(extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "openspectation-scanner-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn config() -> WorkspaceConfig {
        WorkspaceConfig::default()
    }

    fn names(documents: &[SpecDocument]) -> Vec<String> {
        documents.iter().map(SpecDocument::display_path).collect()
    }

    #[test]
    fn an_uninitialised_layout_scans_as_empty_rather_than_failing() {
        // A workspace without specs/ is a state the dashboard shows, not an error.
        let missing = Path::new("/tmp/openspectation-no-such-dir-xyz");
        assert!(scan_specs(missing, &config()).unwrap().is_empty());
        assert!(scan_changes(missing, &config()).unwrap().is_empty());
    }

    #[test]
    fn only_files_with_a_configured_extension_are_collected() {
        let temp = TempDir::new();
        temp.write("a.md", "# A\n");
        temp.write("b.markdown", "# B\n");
        temp.write("c.yaml", "title: C\n");
        temp.write("notes.txt", "ignored");
        temp.write("script.rs", "fn main() {}");
        temp.write("no-extension", "ignored");

        assert_eq!(names(&scan_specs(&temp.0, &config()).unwrap()).len(), 3);
    }

    #[test]
    fn hidden_files_and_directories_are_skipped() {
        let temp = TempDir::new();
        temp.write("visible.md", "# Visible\n");
        temp.write(".hidden.md", "# Hidden\n");
        temp.write(".config/nested.md", "# Nested\n");

        assert_eq!(
            names(&scan_specs(&temp.0, &config()).unwrap()),
            ["visible.md"]
        );
    }

    #[test]
    fn a_hidden_scan_root_is_still_descended_into() {
        // Depth 0 is the root the user pointed at; refusing it would make a
        // workspace under a dotted directory unreadable.
        let temp = TempDir::new();
        temp.write(".openspec/spec.md", "# Spec\n");

        let root = temp.0.join(".openspec");
        assert_eq!(names(&scan_specs(&root, &config()).unwrap()), ["spec.md"]);
    }

    #[test]
    fn build_and_dependency_directories_are_never_descended_into() {
        let temp = TempDir::new();
        temp.write("kept.md", "# Kept\n");
        for ignored in IGNORED_DIRECTORIES {
            temp.write(&format!("{ignored}/buried.md"), "# Buried\n");
        }

        assert_eq!(names(&scan_specs(&temp.0, &config()).unwrap()), ["kept.md"]);
    }

    #[test]
    fn documents_are_ordered_by_directory_then_by_name() {
        let temp = TempDir::new();
        temp.write("zebra.md", "# Z\n");
        temp.write("alpha.md", "# A\n");
        temp.write("billing/Invoices.md", "# I\n");
        temp.write("billing/accounts.md", "# A\n");
        temp.write("auth/spec.md", "# S\n");

        assert_eq!(
            names(&scan_specs(&temp.0, &config()).unwrap()),
            [
                "alpha.md",
                "zebra.md",
                "auth/spec.md",
                "billing/accounts.md",
                "billing/Invoices.md",
            ]
        );
    }

    #[test]
    fn a_scan_can_report_paths_relative_to_a_directory_above_it() {
        // How a proposal's own documents stay addressed from changes/.
        let temp = TempDir::new();
        temp.write("add-oauth/tasks.md", "- [ ] one\n");

        let documents = scan_specs_from(&temp.0.join("add-oauth"), &temp.0, &config()).unwrap();
        assert_eq!(names(&documents), ["add-oauth/tasks.md"]);
    }

    #[test]
    fn a_directory_of_documents_becomes_one_proposal() {
        let temp = TempDir::new();
        temp.write("add-oauth/proposal.md", "# Add OAuth\n");
        temp.write("add-oauth/tasks.md", "- [x] one\n- [ ] two\n");

        let changes = scan_changes(&temp.0, &config()).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].id, "add-oauth");
        assert!(!changes[0].single_file);
        assert_eq!(changes[0].documents.len(), 2);
        assert_eq!((changes[0].tasks_done, changes[0].tasks_total), (1, 2));
    }

    #[test]
    fn a_lone_markdown_file_is_also_a_proposal() {
        let temp = TempDir::new();
        temp.write("rename-fields.md", "# Rename fields\n");

        let changes = scan_changes(&temp.0, &config()).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].id, "rename-fields");
        assert!(changes[0].single_file);
    }

    #[test]
    fn an_empty_proposal_directory_is_not_a_proposal() {
        let temp = TempDir::new();
        std::fs::create_dir_all(temp.0.join("empty-idea")).unwrap();
        temp.write("real/proposal.md", "# Real\n");

        let changes = scan_changes(&temp.0, &config()).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].id, "real");
    }

    #[test]
    fn documentation_of_the_convention_is_not_read_as_a_proposal() {
        let temp = TempDir::new();
        temp.write("README.md", "# How to write a change\n");
        temp.write("index.md", "# Index\n");
        temp.write("add-oauth/proposal.md", "# Add OAuth\n");

        let changes = scan_changes(&temp.0, &config()).unwrap();
        let ids: Vec<&str> = changes.iter().map(|change| change.id.as_str()).collect();
        assert_eq!(ids, ["add-oauth"]);
    }

    #[test]
    fn the_archive_yields_one_proposal_each_and_sorts_last() {
        let temp = TempDir::new();
        temp.write("zeta/proposal.md", "# Zeta\n");
        temp.write("alpha/proposal.md", "# Alpha\n");
        temp.write("archive/2026-01-01-old/proposal.md", "# Old\n");
        temp.write("archive/2026-02-01-older/proposal.md", "# Older\n");

        let changes = scan_changes(&temp.0, &config()).unwrap();
        let ids: Vec<&str> = changes.iter().map(|change| change.id.as_str()).collect();
        assert_eq!(ids, ["alpha", "zeta", "2026-01-01-old", "2026-02-01-older"]);
        assert_eq!(
            changes.iter().filter(|change| change.archived).count(),
            2,
            "the archive must not collapse into one proposal"
        );
    }

    #[test]
    fn the_archive_directory_is_recognized_whatever_its_case() {
        assert!(is_archive(Path::new("/w/changes/archive")));
        assert!(is_archive(Path::new("/w/changes/Archive")));
        assert!(!is_archive(Path::new("/w/changes/archived")));
    }

    #[test]
    fn a_proposal_directory_named_like_a_build_output_is_skipped() {
        let temp = TempDir::new();
        temp.write("target/proposal.md", "# Not a change\n");
        temp.write("add-oauth/proposal.md", "# Add OAuth\n");

        let changes = scan_changes(&temp.0, &config()).unwrap();
        let ids: Vec<&str> = changes.iter().map(|change| change.id.as_str()).collect();
        assert_eq!(ids, ["add-oauth"]);
    }
}
