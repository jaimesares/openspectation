//! Collapsible artifact tree for the documents of a change proposal.
//!
//! The tree is rebuilt from the workspace on every reload, so collapsed state
//! is keyed by path rather than by row index: a directory the user folded stays
//! folded when a sibling appears or disappears.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;

use crate::domain::spec::SpecDocument;

/// What a row stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    Document,
}

/// One visible row of the tree.
#[derive(Debug, Clone)]
pub struct Row {
    pub label: String,
    pub depth: usize,
    pub kind: NodeKind,
    /// Slash separated path inside the change, used as the collapse key.
    pub key: String,
    /// Index into the change's document list, for document rows.
    pub document: Option<usize>,
    pub expanded: bool,
    /// Documents contained in this row, directly or below it.
    pub documents: usize,
    /// Completed and total task counts of everything under the row.
    pub tasks_done: usize,
    pub tasks_total: usize,
}

/// Flattened, navigable view of a change's files.
#[derive(Debug, Default)]
pub struct ArtifactTree {
    rows: Vec<Row>,
    collapsed: HashSet<String>,
    selected: usize,
}

impl ArtifactTree {
    /// Rebuilds the rows from a document set, preserving collapsed directories.
    ///
    /// Paths are shown relative to `root`, and `Row::document` indexes into
    /// `documents`, so the caller must resolve rows against the same slice it
    /// built the tree from.
    pub fn rebuild(&mut self, root: Option<&Path>, documents: &[&SpecDocument]) {
        let previous = self.selected_key();

        self.rows = match root {
            Some(root) => self.flatten(root, documents),
            None => Vec::new(),
        };

        self.selected = previous
            .and_then(|key| self.rows.iter().position(|row| row.key == key))
            .unwrap_or(0)
            .min(self.rows.len().saturating_sub(1));
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn selected(&self) -> Option<usize> {
        (!self.rows.is_empty()).then_some(self.selected)
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    /// Index into the change's documents for the selected row, if it is a file.
    pub fn selected_document(&self) -> Option<usize> {
        self.selected_row().and_then(|row| row.document)
    }

    fn selected_key(&self) -> Option<String> {
        self.rows.get(self.selected).map(|row| row.key.clone())
    }

    pub fn select(&mut self, index: usize) {
        if index < self.rows.len() {
            self.selected = index;
        }
    }

    /// Moves the cursor, wrapping at both ends.
    pub fn step(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let length = self.rows.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(length) as usize;
    }

    pub fn first(&mut self) {
        self.selected = 0;
    }

    pub fn last(&mut self) {
        self.selected = self.rows.len().saturating_sub(1);
    }

    /// Folds or unfolds the selected directory; returns `true` when it acted.
    pub fn toggle_selected(&mut self, root: Option<&Path>, documents: &[&SpecDocument]) -> bool {
        let Some(row) = self.rows.get(self.selected) else {
            return false;
        };
        if row.kind != NodeKind::Directory {
            return false;
        }

        let key = row.key.clone();
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
        self.rebuild(root, documents);
        true
    }

    /// Collapses the selected directory, or jumps to the parent of a file.
    pub fn collapse_or_parent(&mut self, root: Option<&Path>, documents: &[&SpecDocument]) {
        let Some(row) = self.rows.get(self.selected).cloned() else {
            return;
        };

        if row.kind == NodeKind::Directory && !self.collapsed.contains(&row.key) {
            self.collapsed.insert(row.key);
            self.rebuild(root, documents);
            return;
        }

        if row.depth > 0
            && let Some(parent) = self.rows[..self.selected]
                .iter()
                .rposition(|candidate| candidate.depth < row.depth)
        {
            self.selected = parent;
        }
    }

    /// Expands the selected directory, or steps into its first child.
    pub fn expand_or_child(&mut self, root: Option<&Path>, documents: &[&SpecDocument]) {
        let Some(row) = self.rows.get(self.selected).cloned() else {
            return;
        };
        if row.kind != NodeKind::Directory {
            return;
        }

        if self.collapsed.remove(&row.key) {
            self.rebuild(root, documents);
            return;
        }
        if self
            .rows
            .get(self.selected + 1)
            .is_some_and(|next| next.depth > row.depth)
        {
            self.selected += 1;
        }
    }

    fn flatten(&self, base: &Path, documents: &[&SpecDocument]) -> Vec<Row> {
        let mut root = Node::default();

        for (index, document) in documents.iter().enumerate() {
            let relative = document.path.strip_prefix(base).unwrap_or(&document.path);

            let parts: Vec<String> = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect();

            root.insert(
                &parts,
                index,
                document.tasks_done,
                document.tasks_total,
                String::new(),
            );
        }

        let mut rows = Vec::new();
        root.emit(0, &self.collapsed, &mut rows);
        rows
    }
}

/// Intermediate tree used only while flattening.
#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    key: String,
    document: Option<usize>,
    documents: usize,
    tasks_done: usize,
    tasks_total: usize,
}

impl Node {
    fn insert(
        &mut self,
        parts: &[String],
        document: usize,
        tasks_done: usize,
        tasks_total: usize,
        prefix: String,
    ) {
        self.documents += 1;
        self.tasks_done += tasks_done;
        self.tasks_total += tasks_total;

        let Some((head, rest)) = parts.split_first() else {
            self.document = Some(document);
            return;
        };

        let key = if prefix.is_empty() {
            head.clone()
        } else {
            format!("{prefix}/{head}")
        };

        let child = self.children.entry(head.clone()).or_default();
        child.key = key.clone();
        child.insert(rest, document, tasks_done, tasks_total, key);
    }

    fn emit(&self, depth: usize, collapsed: &HashSet<String>, out: &mut Vec<Row>) {
        // Files sort before directories at the same level, matching how the
        // change's own documents read: the proposal first, its deltas after.
        let mut entries: Vec<(&String, &Node)> = self.children.iter().collect();
        entries.sort_by_key(|(name, node)| {
            (
                if node.children.is_empty() { 0 } else { 1 },
                (*name).clone(),
            )
        });

        for (name, child) in entries {
            let directory = !child.children.is_empty();
            let expanded = directory && !collapsed.contains(&child.key);

            out.push(Row {
                label: name.clone(),
                depth,
                kind: if directory {
                    NodeKind::Directory
                } else {
                    NodeKind::Document
                },
                key: child.key.clone(),
                document: child.document,
                expanded,
                documents: child.documents,
                tasks_done: child.tasks_done,
                tasks_total: child.tasks_total,
            });

            if expanded {
                child.emit(depth + 1, collapsed, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    const ROOT: &str = "/w/changes/add-history";

    fn document(path: &str) -> SpecDocument {
        SpecDocument::from_memory(
            &PathBuf::from(path),
            Path::new("/w/changes"),
            "Doc".to_string(),
            "- [x] one\n- [ ] two\n".to_string(),
            None,
        )
    }

    fn documents() -> Vec<SpecDocument> {
        vec![
            document("/w/changes/add-history/proposal.md"),
            document("/w/changes/add-history/tasks.md"),
            document("/w/changes/add-history/specs/edit/spec.md"),
            document("/w/changes/add-history/specs/undo/spec.md"),
        ]
    }

    fn build(owned: &[SpecDocument]) -> (ArtifactTree, Vec<&SpecDocument>) {
        let borrowed: Vec<&SpecDocument> = owned.iter().collect();
        let mut tree = ArtifactTree::default();
        tree.rebuild(Some(Path::new(ROOT)), &borrowed);
        (tree, borrowed)
    }

    #[test]
    fn files_come_before_directories() {
        let owned = documents();
        let (tree, _) = build(&owned);

        let labels: Vec<&str> = tree.rows().iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "proposal.md",
                "tasks.md",
                "specs",
                "edit",
                "spec.md",
                "undo",
                "spec.md"
            ]
        );
    }

    #[test]
    fn collapsing_a_directory_hides_its_subtree() {
        let owned = documents();
        let (mut tree, borrowed) = build(&owned);

        tree.select(2);
        assert!(tree.toggle_selected(Some(Path::new(ROOT)), &borrowed));

        let labels: Vec<&str> = tree.rows().iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["proposal.md", "tasks.md", "specs"]);
    }

    #[test]
    fn a_directory_aggregates_the_tasks_below_it() {
        let owned = documents();
        let (tree, _) = build(&owned);

        let specs = tree.rows().iter().find(|row| row.label == "specs").unwrap();
        assert_eq!((specs.tasks_done, specs.tasks_total), (2, 4));
        assert_eq!(specs.documents, 2);
    }

    #[test]
    fn the_selected_path_survives_a_rebuild() {
        let owned = documents();
        let (mut tree, borrowed) = build(&owned);

        tree.select(4);
        let key = tree.selected_row().unwrap().key.clone();
        tree.rebuild(Some(Path::new(ROOT)), &borrowed);

        assert_eq!(tree.selected_row().unwrap().key, key);
    }

    fn labels(tree: &ArtifactTree) -> Vec<&str> {
        tree.rows().iter().map(|row| row.label.as_str()).collect()
    }

    #[test]
    fn keys_are_the_slash_separated_path_inside_the_change() {
        let owned = documents();
        let (tree, _) = build(&owned);

        let keys: Vec<&str> = tree.rows().iter().map(|row| row.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "proposal.md",
                "tasks.md",
                "specs",
                "specs/edit",
                "specs/edit/spec.md",
                "specs/undo",
                "specs/undo/spec.md"
            ]
        );
    }

    #[test]
    fn only_directory_rows_are_marked_expandable() {
        let owned = documents();
        let (tree, _) = build(&owned);

        for row in tree.rows() {
            match row.kind {
                NodeKind::Directory => {
                    assert!(row.expanded, "{} not expanded", row.label);
                    assert!(row.document.is_none());
                }
                NodeKind::Document => {
                    assert!(!row.expanded, "{} expanded", row.label);
                    assert!(row.document.is_some());
                }
            }
        }
    }

    #[test]
    fn a_document_row_points_back_into_the_change_document_list() {
        let owned = documents();
        let (mut tree, _) = build(&owned);

        tree.select(0);
        assert_eq!(tree.selected_document(), Some(0));
        tree.select(4);
        assert_eq!(tree.selected_document(), Some(2));
        // A directory resolves to no document at all.
        tree.select(2);
        assert_eq!(tree.selected_document(), None);
    }

    #[test]
    fn a_change_without_a_root_produces_no_rows() {
        let owned = documents();
        let borrowed: Vec<&SpecDocument> = owned.iter().collect();
        let mut tree = ArtifactTree::default();
        tree.rebuild(None, &borrowed);

        assert!(tree.is_empty());
        assert_eq!(tree.selected(), None);
        assert!(tree.selected_row().is_none());
        assert_eq!(tree.selected_document(), None);
    }

    #[test]
    fn every_movement_is_inert_on_an_empty_tree() {
        let mut tree = ArtifactTree::default();
        tree.step(1);
        tree.first();
        tree.last();
        tree.select(3);
        assert!(!tree.toggle_selected(Some(Path::new(ROOT)), &[]));
        tree.collapse_or_parent(Some(Path::new(ROOT)), &[]);
        tree.expand_or_child(Some(Path::new(ROOT)), &[]);
        assert_eq!(tree.selected(), None);
    }

    #[test]
    fn the_cursor_wraps_at_both_ends() {
        let owned = documents();
        let (mut tree, _) = build(&owned);

        tree.step(-1);
        assert_eq!(tree.selected(), Some(6));
        tree.step(1);
        assert_eq!(tree.selected(), Some(0));
        tree.last();
        assert_eq!(tree.selected(), Some(6));
        tree.first();
        assert_eq!(tree.selected(), Some(0));
    }

    #[test]
    fn selecting_past_the_last_row_is_ignored() {
        let owned = documents();
        let (mut tree, _) = build(&owned);

        tree.select(2);
        tree.select(99);
        assert_eq!(tree.selected(), Some(2));
    }

    #[test]
    fn toggling_a_document_row_does_nothing() {
        let owned = documents();
        let (mut tree, borrowed) = build(&owned);

        tree.select(0);
        assert!(!tree.toggle_selected(Some(Path::new(ROOT)), &borrowed));
        assert_eq!(labels(&tree).len(), 7);
    }

    #[test]
    fn toggling_twice_returns_the_subtree() {
        let owned = documents();
        let (mut tree, borrowed) = build(&owned);

        tree.select(2);
        tree.toggle_selected(Some(Path::new(ROOT)), &borrowed);
        assert_eq!(labels(&tree).len(), 3);
        tree.toggle_selected(Some(Path::new(ROOT)), &borrowed);
        assert_eq!(labels(&tree).len(), 7);
    }

    #[test]
    fn collapse_folds_a_directory_and_then_walks_to_the_parent() {
        let owned = documents();
        let (mut tree, borrowed) = build(&owned);

        tree.select(3);
        tree.collapse_or_parent(Some(Path::new(ROOT)), &borrowed);
        assert_eq!(
            labels(&tree),
            [
                "proposal.md",
                "tasks.md",
                "specs",
                "edit",
                "undo",
                "spec.md"
            ]
        );
        assert_eq!(tree.selected_row().unwrap().key, "specs/edit");

        // Already folded, so the second press climbs instead.
        tree.collapse_or_parent(Some(Path::new(ROOT)), &borrowed);
        assert_eq!(tree.selected_row().unwrap().key, "specs");
    }

    #[test]
    fn collapse_on_a_top_level_document_has_no_parent_to_reach() {
        let owned = documents();
        let (mut tree, borrowed) = build(&owned);

        tree.select(1);
        tree.collapse_or_parent(Some(Path::new(ROOT)), &borrowed);
        assert_eq!(tree.selected(), Some(1));
    }

    #[test]
    fn expand_unfolds_a_directory_and_then_steps_into_it() {
        let owned = documents();
        let (mut tree, borrowed) = build(&owned);

        tree.select(2);
        tree.toggle_selected(Some(Path::new(ROOT)), &borrowed);
        assert_eq!(labels(&tree).len(), 3);

        tree.expand_or_child(Some(Path::new(ROOT)), &borrowed);
        assert_eq!(labels(&tree).len(), 7);
        assert_eq!(tree.selected_row().unwrap().key, "specs");

        // Already open, so the second press descends.
        tree.expand_or_child(Some(Path::new(ROOT)), &borrowed);
        assert_eq!(tree.selected_row().unwrap().key, "specs/edit");
    }

    #[test]
    fn expand_on_a_document_row_does_nothing() {
        let owned = documents();
        let (mut tree, borrowed) = build(&owned);

        tree.select(4);
        tree.expand_or_child(Some(Path::new(ROOT)), &borrowed);
        assert_eq!(tree.selected(), Some(4));
    }

    #[test]
    fn a_selection_whose_file_disappeared_falls_back_to_the_first_row() {
        let owned = documents();
        let (mut tree, _) = build(&owned);
        tree.select(4);

        let shrunk: Vec<&SpecDocument> = owned.iter().take(2).collect();
        tree.rebuild(Some(Path::new(ROOT)), &shrunk);

        assert_eq!(labels(&tree), ["proposal.md", "tasks.md"]);
        assert_eq!(tree.selected(), Some(0));
    }

    #[test]
    fn a_folded_directory_stays_folded_when_a_sibling_appears() {
        // The reason collapsed state is keyed by path and not by row index.
        let owned = documents();
        let (mut tree, borrowed) = build(&owned);

        tree.select(3);
        tree.toggle_selected(Some(Path::new(ROOT)), &borrowed);
        assert_eq!(
            labels(&tree),
            [
                "proposal.md",
                "tasks.md",
                "specs",
                "edit",
                "undo",
                "spec.md"
            ]
        );

        let mut grown = documents();
        grown.insert(0, document("/w/changes/add-history/design.md"));
        let grown_refs: Vec<&SpecDocument> = grown.iter().collect();
        tree.rebuild(Some(Path::new(ROOT)), &grown_refs);

        let edit = tree
            .rows()
            .iter()
            .find(|row| row.key == "specs/edit")
            .unwrap();
        assert!(!edit.expanded);
    }
}
