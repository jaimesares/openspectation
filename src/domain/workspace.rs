//! Workspace discovery, loading and aggregate statistics.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rust_i18n::t;

use crate::domain::change::ChangeProposal;
use crate::domain::config::{self, WorkspaceConfig};
use crate::domain::scanner;
use crate::domain::spec::{SpecDocument, SpecStatus};
use crate::error::{Error, Result};

/// Which OpenSpec markers were found in a directory.
#[derive(Debug, Clone, Copy, Default)]
pub struct Markers {
    pub specs: bool,
    pub changes: bool,
    pub design: bool,
    pub config: bool,
}

impl Markers {
    /// A directory counts as a workspace as soon as one marker is present.
    pub const fn any(self) -> bool {
        self.specs || self.changes || self.design || self.config
    }

    /// All four markers present.
    pub const fn complete(self) -> bool {
        self.specs && self.changes && self.design && self.config
    }
}

/// Fully loaded workspace: configuration plus every parsed document.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub config: WorkspaceConfig,
    /// `true` when `openspec.yaml` was actually present on disk.
    pub has_config_file: bool,
    pub markers: Markers,
    pub specs: Vec<SpecDocument>,
    pub changes: Vec<ChangeProposal>,
    pub design: Option<SpecDocument>,
    /// The configuration file read as a document, so it can be read and edited
    /// like anything else in the workspace.
    pub config_document: Option<SpecDocument>,
}

impl Workspace {
    /// Validates a `--path` value and returns its canonical form.
    pub fn resolve_root(path: &Path) -> Result<PathBuf> {
        if !path.exists() {
            return Err(Error::PathNotFound(path.to_path_buf()));
        }
        if !path.is_dir() {
            return Err(Error::NotADirectory(path.to_path_buf()));
        }
        // Canonicalisation is a convenience, not a requirement: a path that
        // cannot be canonicalised is still perfectly usable.
        Ok(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
    }

    /// Finds the actual workspace root for a `--path` value.
    ///
    /// The upstream OpenSpec CLI keeps its whole layout inside an `openspec/`
    /// directory, so pointing at a repository root must still work: when the
    /// given directory carries no marker, the nested one is tried before
    /// giving up. The original path is returned unchanged when neither
    /// qualifies, so the resulting error still names what the user typed.
    pub fn discover(path: &Path) -> Result<PathBuf> {
        let root = Self::resolve_root(path)?;
        if Self::detect_markers(&root).any() {
            return Ok(root);
        }

        let nested = root.join(config::WORKSPACE_DIR);
        if nested.is_dir() && Self::detect_markers(&nested).any() {
            return Ok(nested);
        }

        Ok(root)
    }

    /// Detects which OpenSpec markers exist under `root`.
    pub fn detect_markers(root: &Path) -> Markers {
        let config = WorkspaceConfig::load(root)
            .ok()
            .flatten()
            .unwrap_or_default();
        Markers {
            specs: config.specs_dir(root).is_dir(),
            changes: config.changes_dir(root).is_dir(),
            design: config.design_file(root).is_file(),
            config: WorkspaceConfig::locate(root).is_some(),
        }
    }

    /// Loads a workspace, failing when no OpenSpec marker is present.
    pub fn open(path: &Path) -> Result<Self> {
        let root = Self::discover(path)?;
        let workspace = Self::load(&root)?;

        if !workspace.markers.any() {
            return Err(Error::NotAWorkspace(root));
        }

        Ok(workspace)
    }

    /// Loads a workspace without requiring any marker to be present.
    pub fn load(root: &Path) -> Result<Self> {
        let loaded = WorkspaceConfig::load(root)?;
        let has_config_file = loaded.is_some();
        let config = loaded.unwrap_or_default();

        let specs_dir = config.specs_dir(root);
        let changes_dir = config.changes_dir(root);
        let design_path = config.design_file(root);

        let markers = Markers {
            specs: specs_dir.is_dir(),
            changes: changes_dir.is_dir(),
            design: design_path.is_file(),
            config: has_config_file,
        };

        let specs = scanner::scan_specs(&specs_dir, &config)?;
        let changes = scanner::scan_changes(&changes_dir, &config)?;
        let design = if markers.design {
            Some(SpecDocument::load(&design_path, root)?)
        } else {
            context_document(root, &config)
        };

        let config_document = config::WorkspaceConfig::locate(root)
            .map(|path| SpecDocument::load(&path, root))
            .transpose()?;

        Ok(Self {
            root: root.to_path_buf(),
            config,
            has_config_file,
            markers,
            specs,
            changes,
            design,
            config_document,
        })
    }

    /// Documents describing the project itself rather than a capability.
    ///
    /// Real files only. When there is no `design.md` the design view is
    /// synthesised from the `context:` block of the configuration, and showing
    /// that next to the file it was extracted from would put two different
    /// texts behind one path — which an editor cannot honour.
    pub fn project_documents(&self) -> Vec<&SpecDocument> {
        let config = self.config_document.as_ref().map(|doc| doc.path.as_path());
        self.design
            .iter()
            .filter(|doc| Some(doc.path.as_path()) != config)
            .chain(self.config_document.iter())
            .collect()
    }

    /// Re-reads everything from disk.
    pub fn reload(&mut self) -> Result<()> {
        *self = Self::load(&self.root.clone())?;
        Ok(())
    }

    pub fn specs_dir(&self) -> PathBuf {
        self.config.specs_dir(&self.root)
    }

    pub fn changes_dir(&self) -> PathBuf {
        self.config.changes_dir(&self.root)
    }

    pub fn design_path(&self) -> PathBuf {
        self.config.design_file(&self.root)
    }

    /// Project name from the configuration, falling back to the directory name.
    pub fn project_name(&self) -> String {
        let configured = self.config.project.name.trim();
        if !configured.is_empty() {
            return configured.to_string();
        }

        let name = self.root.file_name().and_then(|value| value.to_str());
        // A nested layout is named after the tool rather than the project, so
        // the directory holding it is what carries the real name.
        if name == Some(config::WORKSPACE_DIR)
            && let Some(parent) = self
                .root
                .parent()
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
        {
            return parent.to_string();
        }

        name.unwrap_or(config::WORKSPACE_DIR).to_string()
    }

    /// Position of a change proposal addressed by identifier.
    ///
    /// An exact match wins; otherwise a unique case-insensitive prefix is
    /// accepted, so `ospc --change add-native` reaches `add-native-history`.
    pub fn change_index(&self, id: &str) -> Option<usize> {
        let needle = id.trim();
        if needle.is_empty() {
            return None;
        }

        if let Some(exact) = self.changes.iter().position(|change| change.id == needle) {
            return Some(exact);
        }

        let lowered = needle.to_lowercase();
        let mut matches = self
            .changes
            .iter()
            .enumerate()
            .filter(|(_, change)| change.id.to_lowercase().starts_with(&lowered));

        let first = matches.next()?;
        matches.next().is_none().then_some(first.0)
    }

    /// Change proposal addressed by identifier.
    pub fn find_change(&self, id: &str) -> Option<&ChangeProposal> {
        self.changes.get(self.change_index(id)?)
    }

    /// Number of change proposals that are still active.
    pub fn active_changes(&self) -> usize {
        self.changes
            .iter()
            .filter(|change| !change.archived)
            .count()
    }

    /// Number of change proposals that have been archived.
    pub fn archived_changes(&self) -> usize {
        self.changes.iter().filter(|change| change.archived).count()
    }

    /// Aggregate metrics used by `list` and by the TUI dashboard.
    pub fn stats(&self) -> Stats {
        let mut by_status: BTreeMap<SpecStatus, usize> = BTreeMap::new();
        for document in &self.specs {
            *by_status.entry(document.status).or_default() += 1;
        }

        let spec_bytes: u64 = self.specs.iter().map(|document| document.size).sum();
        let change_bytes: u64 = self.changes.iter().map(|change| change.size).sum();
        let design_bytes = self.design.as_ref().map_or(0, |document| document.size);

        Stats {
            spec_count: self.specs.len(),
            change_count: self.changes.len(),
            total_bytes: spec_bytes + change_bytes + design_bytes,
            words: self.specs.iter().map(|document| document.words).sum(),
            requirements: self
                .specs
                .iter()
                .map(|document| document.requirements)
                .sum(),
            tasks_total: self.changes.iter().map(|change| change.tasks_total).sum(),
            tasks_done: self.changes.iter().map(|change| change.tasks_done).sum(),
            by_status,
        }
    }

    /// Documents sorted by modification time, most recent first.
    pub fn recently_updated(&self, limit: usize) -> Vec<&SpecDocument> {
        let mut documents: Vec<&SpecDocument> = self
            .specs
            .iter()
            .chain(self.design.iter())
            .chain(
                self.changes
                    .iter()
                    .flat_map(|change| change.documents.iter()),
            )
            .collect();

        documents.sort_by_key(|document| std::cmp::Reverse(document.modified));
        documents.truncate(limit);
        documents
    }

    /// Path of `root` relative to the current directory when that is shorter.
    pub fn display_root(&self) -> String {
        let absolute = self.root.display().to_string();
        let Ok(current) = std::env::current_dir() else {
            return absolute;
        };
        if self.root == current {
            return ".".to_string();
        }
        match self.root.strip_prefix(&current) {
            Ok(relative) if !relative.as_os_str().is_empty() => {
                let relative = relative.display().to_string();
                if relative.len() < absolute.len() {
                    relative
                } else {
                    absolute
                }
            }
            _ => absolute,
        }
    }
}

/// Turns the configuration `context` block into a design document.
///
/// A nested OpenSpec workspace records its architecture there instead of in a
/// separate file, so this keeps the design view meaningful for those projects.
fn context_document(root: &Path, config: &WorkspaceConfig) -> Option<SpecDocument> {
    let context = config.context.trim();
    if context.is_empty() {
        return None;
    }

    let path = WorkspaceConfig::locate(root)?;
    let modified = std::fs::metadata(&path)
        .ok()
        .and_then(|metadata| metadata.modified().ok());

    Some(SpecDocument::from_memory(
        &path,
        root,
        t!("list.context").to_string(),
        context.to_string(),
        modified,
    ))
}

/// Aggregate metrics for a workspace.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub spec_count: usize,
    pub change_count: usize,
    pub total_bytes: u64,
    pub words: usize,
    pub requirements: usize,
    pub tasks_total: usize,
    pub tasks_done: usize,
    pub by_status: BTreeMap<SpecStatus, usize>,
}

impl Stats {
    /// Overall completion in the `0.0..=1.0` range.
    ///
    /// Task checkboxes are authoritative; without them the mix of document
    /// statuses is used as a proxy.
    pub fn completion(&self) -> f64 {
        if self.tasks_total > 0 {
            return self.tasks_done as f64 / self.tasks_total as f64;
        }
        if self.spec_count == 0 {
            return 0.0;
        }

        let weighted: f64 = self
            .by_status
            .iter()
            .map(|(status, count)| status_weight(*status) * *count as f64)
            .sum();
        (weighted / self.spec_count as f64).clamp(0.0, 1.0)
    }

    /// Number of documents in a given status.
    pub fn count(&self, status: SpecStatus) -> usize {
        self.by_status.get(&status).copied().unwrap_or_default()
    }
}

const fn status_weight(status: SpecStatus) -> f64 {
    match status {
        SpecStatus::Implemented => 1.0,
        SpecStatus::Approved => 0.75,
        SpecStatus::Review => 0.45,
        SpecStatus::Draft => 0.15,
        SpecStatus::Deprecated | SpecStatus::Unknown => 0.0,
    }
}

/// Directory names created by `init`, exposed for the scaffolding command.
pub const SCAFFOLD_DIRECTORIES: [&str; 2] = [config::SPECS_DIR, config::CHANGES_DIR];

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// A repository laid out the way the upstream OpenSpec CLI writes it.
    struct Nested {
        root: PathBuf,
    }

    impl Nested {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "openspectation-nested-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);

            let inner = root.join("openspec");
            std::fs::create_dir_all(inner.join("specs/content-format")).unwrap();
            std::fs::create_dir_all(inner.join("changes/add-history")).unwrap();
            std::fs::create_dir_all(inner.join("changes/archive/2026-08-06-secure-html")).unwrap();

            std::fs::write(
                inner.join("config.yaml"),
                "schema: spec-driven\ncontext: |\n  A minimal editor.\nrules:\n  proposal:\n    - Keep it short.\n",
            )
            .unwrap();
            std::fs::write(
                inner.join("specs/content-format/spec.md"),
                "The editor MUST speak one format.\n",
            )
            .unwrap();
            std::fs::write(
                inner.join("changes/add-history/proposal.md"),
                "# Add history\n",
            )
            .unwrap();
            std::fs::write(
                inner.join("changes/add-history/tasks.md"),
                "- [x] one\n- [ ] two\n",
            )
            .unwrap();
            std::fs::write(
                inner.join("changes/archive/2026-08-06-secure-html/proposal.md"),
                "# Secure HTML\n",
            )
            .unwrap();

            Self { root }
        }
    }

    impl Drop for Nested {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn descends_into_a_nested_openspec_directory() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        assert!(workspace.root.ends_with("openspec"));
        assert!(workspace.has_config_file);
        assert_eq!(workspace.specs.len(), 1);
    }

    #[test]
    fn names_the_project_after_the_repository_not_the_layout_directory() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        let expected = fixture.root.file_name().unwrap().to_str().unwrap();
        assert_eq!(workspace.project_name(), expected);
    }

    #[test]
    fn titles_a_generic_spec_file_after_its_capability_directory() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        assert_eq!(workspace.specs[0].title, "Content Format");
    }

    #[test]
    fn keeps_archived_proposals_separate_and_last() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        let ids: Vec<&str> = workspace
            .changes
            .iter()
            .map(|change| change.id.as_str())
            .collect();
        assert_eq!(ids, ["add-history", "2026-08-06-secure-html"]);

        assert!(!workspace.changes[0].archived);
        assert!(workspace.changes[1].archived);
    }

    #[test]
    fn derives_a_status_for_proposals_that_declare_none() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        assert_eq!(workspace.changes[0].status, SpecStatus::Review);
        assert_eq!(workspace.changes[1].status, SpecStatus::Implemented);
    }

    #[test]
    fn uses_the_configuration_context_as_the_design_document() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        let design = workspace
            .design
            .expect("context should stand in for design");
        assert_eq!(design.body.trim(), "A minimal editor.");
    }

    #[test]
    fn a_directory_without_markers_is_not_a_workspace() {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "openspectation-bare-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();

        let outcome = Workspace::open(&root);
        let _ = std::fs::remove_dir_all(&root);

        assert!(matches!(outcome, Err(Error::NotAWorkspace(_))));
    }

    #[test]
    fn one_marker_is_enough_but_completeness_needs_all_four() {
        let none = Markers::default();
        assert!(!none.any());
        assert!(!none.complete());

        let one = Markers {
            specs: true,
            ..Markers::default()
        };
        assert!(one.any());
        assert!(!one.complete());

        let all = Markers {
            specs: true,
            changes: true,
            design: true,
            config: true,
        };
        assert!(all.any() && all.complete());
    }

    #[test]
    fn a_path_that_does_not_exist_is_told_apart_from_one_that_is_a_file() {
        let missing = std::env::temp_dir().join("openspectation-no-such-root-xyz");
        assert!(matches!(
            Workspace::resolve_root(&missing),
            Err(Error::PathNotFound(_))
        ));

        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let file = std::env::temp_dir().join(format!(
            "openspectation-file-{}-{unique}",
            std::process::id()
        ));
        std::fs::write(&file, "not a directory").unwrap();
        let outcome = Workspace::resolve_root(&file);
        let _ = std::fs::remove_file(&file);

        assert!(matches!(outcome, Err(Error::NotADirectory(_))));
    }

    #[test]
    fn discovery_returns_the_given_path_when_neither_layout_qualifies() {
        // The error must name what the user typed, not a directory they never
        // mentioned.
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "openspectation-plain-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();

        let discovered = Workspace::discover(&root);
        let _ = std::fs::remove_dir_all(&root);

        assert!(!discovered.unwrap().ends_with(config::WORKSPACE_DIR));
    }

    #[test]
    fn the_directory_accessors_follow_the_discovered_root() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        assert!(workspace.specs_dir().ends_with("specs"));
        assert!(workspace.changes_dir().ends_with("changes"));
        assert!(workspace.specs_dir().starts_with(&workspace.root));
        assert!(workspace.design_path().starts_with(&workspace.root));
    }

    #[test]
    fn markers_report_what_the_nested_layout_actually_has() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        assert!(workspace.markers.specs);
        assert!(workspace.markers.changes);
        assert!(workspace.markers.config);
        // There is no design.md; the context block stands in for it.
        assert!(!workspace.markers.design);
        assert!(!workspace.markers.complete());
    }

    #[test]
    fn a_change_is_addressable_by_a_unique_prefix() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        assert_eq!(workspace.change_index("add-history"), Some(0));
        assert_eq!(workspace.change_index("add"), Some(0));
        assert_eq!(workspace.change_index("ADD-HIST"), Some(0));
        assert_eq!(
            workspace.find_change("add").map(|c| c.id.as_str()),
            Some("add-history")
        );

        assert_eq!(workspace.change_index("nonexistent"), None);
        assert_eq!(workspace.change_index("  "), None);
        assert!(workspace.find_change("nonexistent").is_none());
    }

    #[test]
    fn an_ambiguous_prefix_resolves_to_nothing_rather_than_to_a_guess() {
        let fixture = Nested::new();
        let inner = fixture.root.join("openspec");
        std::fs::create_dir_all(inner.join("changes/add-undo")).unwrap();
        std::fs::write(inner.join("changes/add-undo/proposal.md"), "# Add undo\n").unwrap();

        let workspace = Workspace::open(&fixture.root).unwrap();
        assert_eq!(workspace.change_index("add"), None);
        // The full identifier still resolves.
        assert!(workspace.change_index("add-undo").is_some());
    }

    #[test]
    fn an_exact_identifier_wins_over_a_prefix_of_another() {
        let fixture = Nested::new();
        let inner = fixture.root.join("openspec");
        std::fs::create_dir_all(inner.join("changes/add")).unwrap();
        std::fs::write(inner.join("changes/add/proposal.md"), "# Add\n").unwrap();

        let workspace = Workspace::open(&fixture.root).unwrap();
        let index = workspace.change_index("add").unwrap();
        assert_eq!(workspace.changes[index].id, "add");
    }

    #[test]
    fn active_and_archived_proposals_are_counted_apart() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        assert_eq!(workspace.active_changes(), 1);
        assert_eq!(workspace.archived_changes(), 1);
        assert_eq!(
            workspace.active_changes() + workspace.archived_changes(),
            workspace.changes.len()
        );
    }

    #[test]
    fn the_statistics_aggregate_every_document_in_the_workspace() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();
        let stats = workspace.stats();

        assert_eq!(stats.spec_count, 1);
        assert_eq!(stats.change_count, 2);
        assert_eq!(stats.requirements, 1);
        assert_eq!((stats.tasks_done, stats.tasks_total), (1, 2));
        assert!(stats.total_bytes > 0);
        assert_eq!(stats.count(SpecStatus::Unknown), 1);
        assert_eq!(stats.count(SpecStatus::Deprecated), 0);
    }

    #[test]
    fn checkboxes_decide_the_completion_when_there_are_any() {
        let stats = Stats {
            tasks_total: 4,
            tasks_done: 3,
            spec_count: 10,
            ..Stats::default()
        };
        assert!((stats.completion() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn without_checkboxes_the_completion_falls_back_to_the_status_mix() {
        let mut by_status = BTreeMap::new();
        by_status.insert(SpecStatus::Implemented, 1);
        by_status.insert(SpecStatus::Draft, 1);
        let stats = Stats {
            spec_count: 2,
            by_status,
            ..Stats::default()
        };
        // (1.0 + 0.15) / 2
        assert!((stats.completion() - 0.575).abs() < 1e-9);

        // A deprecated document weighs nothing rather than counting as done.
        let mut only_deprecated = BTreeMap::new();
        only_deprecated.insert(SpecStatus::Deprecated, 3);
        let stats = Stats {
            spec_count: 3,
            by_status: only_deprecated,
            ..Stats::default()
        };
        assert_eq!(stats.completion(), 0.0);
    }

    #[test]
    fn an_empty_workspace_completes_at_zero_rather_than_dividing_by_zero() {
        assert_eq!(Stats::default().completion(), 0.0);
    }

    #[test]
    fn the_recent_list_is_newest_first_and_honours_its_limit() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        let recent = workspace.recently_updated(3);
        assert_eq!(recent.len(), 3);
        for pair in recent.windows(2) {
            assert!(pair[0].modified >= pair[1].modified);
        }
        assert!(workspace.recently_updated(0).is_empty());
    }

    #[test]
    fn the_project_view_never_shows_one_path_behind_two_texts() {
        // The synthesised design document borrows the configuration's path, and
        // an editor cannot open one path onto two different bodies.
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        let documents = workspace.project_documents();
        let mut paths: Vec<&Path> = documents.iter().map(|doc| doc.path.as_path()).collect();
        let before = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), before, "two documents share one path");
    }

    #[test]
    fn a_reload_picks_up_a_file_added_after_the_first_read() {
        let fixture = Nested::new();
        let mut workspace = Workspace::open(&fixture.root).unwrap();
        assert_eq!(workspace.specs.len(), 1);

        let added = fixture.root.join("openspec/specs/billing");
        std::fs::create_dir_all(&added).unwrap();
        std::fs::write(added.join("spec.md"), "Invoices MUST balance.\n").unwrap();

        workspace.reload().unwrap();
        assert_eq!(workspace.specs.len(), 2);
    }

    #[test]
    fn the_displayed_root_is_absolute_when_it_is_not_below_the_current_directory() {
        let fixture = Nested::new();
        let workspace = Workspace::open(&fixture.root).unwrap();

        // The fixture lives in the temp directory, never inside the crate.
        let displayed = workspace.display_root();
        assert_eq!(displayed, workspace.root.display().to_string());
    }
}
