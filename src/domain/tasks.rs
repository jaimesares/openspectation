//! Task checkboxes, addressed by file and line so they can be written back.
//!
//! Toggling re-reads the file immediately before rewriting it and refuses to
//! act when the addressed line is no longer the checkbox it was collected from.
//! An agent editing `tasks.md` between two frames therefore loses nothing.

use std::path::PathBuf;

use crate::domain::change::ChangeProposal;
use crate::domain::spec::checkbox_state;
use crate::domain::util::write_atomic;
use crate::error::{Error, Result};

/// A single `- [ ]` / `- [x]` item inside a change proposal.
#[derive(Debug, Clone)]
pub struct TaskLine {
    pub path: PathBuf,
    /// One-based line number inside `path`.
    pub line: usize,
    pub done: bool,
    pub label: String,
    /// Nearest heading above the task, used to group the list.
    pub section: Option<String>,
}

impl TaskLine {
    /// File name the task lives in, for display.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string()
    }
}

/// Collects every task of a change, in document then line order.
pub fn collect(change: &ChangeProposal) -> Vec<TaskLine> {
    let mut out = Vec::new();

    for document in &change.documents {
        if document.tasks_total == 0 {
            continue;
        }

        let mut section: Option<String> = None;
        let mut in_code_fence = false;

        for (index, raw) in document.body.lines().enumerate() {
            let trimmed = raw.trim();

            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_code_fence = !in_code_fence;
                continue;
            }
            if in_code_fence {
                continue;
            }

            if let Some(heading) = trimmed.strip_prefix('#') {
                let text = heading.trim_start_matches('#').trim();
                if !text.is_empty() {
                    section = Some(text.to_string());
                }
                continue;
            }

            if let Some(done) = checkbox_state(trimmed) {
                out.push(TaskLine {
                    path: document.path.clone(),
                    // Line numbers address the file, and the body starts after
                    // any frontmatter block that was stripped from it.
                    line: index + 1 + document.frontmatter_lines,
                    done,
                    label: label_of(trimmed),
                    section: section.clone(),
                });
            }
        }
    }

    out
}

/// Flips a task on disk and reports its new state.
///
/// Fails when the file changed underneath: the caller is expected to reload.
pub fn toggle(task: &TaskLine) -> Result<bool> {
    let raw =
        std::fs::read_to_string(&task.path).map_err(|source| Error::io(&task.path, source))?;

    let mut lines: Vec<&str> = raw.lines().collect();
    let index = task.line.saturating_sub(1);

    let current = lines
        .get(index)
        .and_then(|line| checkbox_state(line.trim()))
        .ok_or_else(|| Error::TaskMoved(task.path.clone(), task.line))?;

    let rewritten = flip(lines[index], current);
    lines[index] = &rewritten;

    let mut body = lines.join("\n");
    // `lines()` drops the final terminator; keep the file's own ending.
    if raw.ends_with('\n') {
        body.push('\n');
    }

    write_atomic(&task.path, &body)?;
    Ok(!current)
}

/// Replaces the checkbox marker of a line, leaving everything else alone.
fn flip(line: &str, current: bool) -> String {
    let (from, to) = if current {
        ("[x]", "[ ]")
    } else {
        ("[ ]", "[x]")
    };

    // Only the first marker is the checkbox; a `[x]` later in the text is prose.
    match line.find(from) {
        Some(at) => format!("{}{to}{}", &line[..at], &line[at + from.len()..]),
        None => match line.find(if current { "[X]" } else { "[ ]" }) {
            Some(at) => format!("{}{to}{}", &line[..at], &line[at + 3..]),
            None => line.to_string(),
        },
    }
}

/// Task text with the list marker and the checkbox removed.
fn label_of(trimmed: &str) -> String {
    let rest = trimmed
        .trim_start_matches(['-', '*', '+'])
        .trim_start()
        .trim_start_matches(['[', ']', 'x', 'X', ' ', '✓'])
        .trim();
    rest.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flips_only_the_checkbox_marker() {
        assert_eq!(
            flip("- [ ] write [x] in prose", false),
            "- [x] write [x] in prose"
        );
        assert_eq!(flip("- [x] done", true), "- [ ] done");
        assert_eq!(flip("  - [X] done", true), "  - [ ] done");
    }

    #[test]
    fn strips_the_marker_from_the_label() {
        assert_eq!(
            label_of("- [x] 1.1 Create the plugin"),
            "1.1 Create the plugin"
        );
        assert_eq!(label_of("* [ ] review"), "review");
    }

    #[test]
    fn toggling_rewrites_a_single_line() {
        let path = std::env::temp_dir().join(format!(
            "openspectation-tasks-{}-{}.md",
            std::process::id(),
            line!()
        ));
        std::fs::write(&path, "## One\n\n- [ ] first\n- [x] second\n").unwrap();

        let task = TaskLine {
            path: path.clone(),
            line: 3,
            done: false,
            label: "first".to_string(),
            section: None,
        };
        assert!(toggle(&task).unwrap());

        let raw = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(raw, "## One\n\n- [x] first\n- [x] second\n");
    }

    #[test]
    fn refuses_to_write_when_the_line_is_no_longer_a_task() {
        let path = std::env::temp_dir().join(format!(
            "openspectation-tasks-{}-{}.md",
            std::process::id(),
            line!()
        ));
        std::fs::write(&path, "# Heading\n").unwrap();

        let task = TaskLine {
            path: path.clone(),
            line: 1,
            done: false,
            label: "gone".to_string(),
            section: None,
        };
        let outcome = toggle(&task);
        let _ = std::fs::remove_file(&path);

        assert!(matches!(outcome, Err(Error::TaskMoved(_, 1))));
    }

    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempChange(PathBuf);

    impl TempChange {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "openspectation-tasklist-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(path.join("add-oauth")).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, contents: &str) {
            std::fs::write(self.0.join("add-oauth").join(name), contents).unwrap();
        }

        fn assemble(&self) -> ChangeProposal {
            let directory = self.0.join("add-oauth");
            let documents =
                crate::domain::scanner::scan_specs_from(&directory, &self.0, &Default::default())
                    .unwrap();
            ChangeProposal::assemble(&directory, documents, false, false).unwrap()
        }
    }

    impl Drop for TempChange {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn tasks_are_grouped_under_the_nearest_heading_above_them() {
        let temp = TempChange::new();
        temp.write(
            "tasks.md",
            "# Plan\n\n## Backend\n\n- [x] schema\n- [ ] endpoints\n\n## Frontend\n\n- [ ] form\n",
        );

        let tasks = collect(&temp.assemble());
        let sections: Vec<Option<&str>> =
            tasks.iter().map(|task| task.section.as_deref()).collect();
        assert_eq!(
            sections,
            [Some("Backend"), Some("Backend"), Some("Frontend")]
        );
        assert_eq!(
            tasks
                .iter()
                .map(|task| task.label.as_str())
                .collect::<Vec<_>>(),
            ["schema", "endpoints", "form"]
        );
        assert!(tasks[0].done);
        assert!(!tasks[1].done);
    }

    #[test]
    fn a_checkbox_inside_a_code_fence_is_not_a_task() {
        let temp = TempChange::new();
        temp.write(
            "tasks.md",
            "```markdown\n- [ ] an example\n```\n\n- [ ] a real one\n",
        );

        let tasks = collect(&temp.assemble());
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].label, "a real one");
    }

    #[test]
    fn a_line_number_addresses_the_file_not_the_stripped_body() {
        // Toggling writes to the file, so an offset here corrupts the wrong line.
        let temp = TempChange::new();
        temp.write("tasks.md", "---\nstatus: draft\n---\n\n- [ ] first\n");

        let tasks = collect(&temp.assemble());
        assert_eq!(tasks.len(), 1);

        let raw = std::fs::read_to_string(&tasks[0].path).unwrap();
        let addressed = raw.lines().nth(tasks[0].line - 1).unwrap();
        assert_eq!(addressed, "- [ ] first");
    }

    #[test]
    fn documents_without_checkboxes_are_skipped_entirely() {
        let temp = TempChange::new();
        temp.write("proposal.md", "# Add OAuth\n\nProse only.\n");
        temp.write("tasks.md", "- [ ] only one\n");

        let tasks = collect(&temp.assemble());
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].file_name(), "tasks.md");
    }

    #[test]
    fn a_change_with_no_tasks_at_all_collects_nothing() {
        let temp = TempChange::new();
        temp.write("proposal.md", "# Add OAuth\n\nProse only.\n");
        assert!(collect(&temp.assemble()).is_empty());
    }

    #[test]
    fn toggling_back_and_forth_returns_the_file_to_its_original_bytes() {
        let temp = TempChange::new();
        temp.write("tasks.md", "- [ ] first\n- [x] second\n");
        let original = std::fs::read_to_string(temp.0.join("add-oauth/tasks.md")).unwrap();

        let tasks = collect(&temp.assemble());
        assert!(toggle(&tasks[0]).unwrap());
        assert!(!toggle(&tasks[0]).unwrap());

        assert_eq!(
            std::fs::read_to_string(temp.0.join("add-oauth/tasks.md")).unwrap(),
            original
        );
    }

    #[test]
    fn a_file_without_a_trailing_newline_does_not_gain_one() {
        let temp = TempChange::new();
        temp.write("tasks.md", "- [ ] first");

        let tasks = collect(&temp.assemble());
        toggle(&tasks[0]).unwrap();

        assert_eq!(
            std::fs::read_to_string(temp.0.join("add-oauth/tasks.md")).unwrap(),
            "- [x] first"
        );
    }

    #[test]
    fn a_line_past_the_end_of_the_file_reports_the_task_as_moved() {
        let temp = TempChange::new();
        temp.write("tasks.md", "- [ ] first\n");

        let task = TaskLine {
            path: temp.0.join("add-oauth/tasks.md"),
            line: 99,
            done: false,
            label: "gone".to_string(),
            section: None,
        };
        assert!(matches!(toggle(&task), Err(Error::TaskMoved(_, 99))));
    }

    #[test]
    fn toggling_a_file_that_disappeared_reports_the_path() {
        let task = TaskLine {
            path: Path::new("/tmp/openspectation-no-such-tasks-xyz.md").to_path_buf(),
            line: 1,
            done: false,
            label: "gone".to_string(),
            section: None,
        };
        assert!(matches!(toggle(&task), Err(Error::Io { .. })));
    }

    #[test]
    fn a_task_before_any_heading_belongs_to_no_section() {
        let temp = TempChange::new();
        temp.write("tasks.md", "- [ ] loose\n\n## Later\n\n- [ ] grouped\n");

        let tasks = collect(&temp.assemble());
        assert_eq!(tasks[0].section, None);
        assert_eq!(tasks[1].section.as_deref(), Some("Later"));
    }
}
