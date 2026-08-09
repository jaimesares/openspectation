//! `openspectation init <NAME>` — scaffolds a standard OpenSpec structure.
//!
//! The heading is printed before anything is written and the report once the
//! scaffolding is done, so a failure half way through still names the workspace
//! it was writing into.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rust_i18n::t;

use crate::cli::InitArgs;
use crate::domain::config::{self, WorkspaceConfig};
use crate::domain::util;
use crate::domain::workspace::Workspace;
use crate::domain::{template, workspace};
use crate::error::{Error, Result};
use crate::i18n;
use crate::paint::Painter;

/// Outcome of writing a single scaffold entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Created,
    Overwritten,
    Kept,
}

impl Outcome {
    fn label(self) -> String {
        match self {
            Self::Created => t!("init.created"),
            Self::Overwritten => t!("init.overwritten"),
            Self::Kept => t!("init.exists"),
        }
        .to_string()
    }

    fn render(self, painter: Painter) -> String {
        match self {
            Self::Created => painter.success(format!("✔ {}", self.label())),
            Self::Overwritten => painter.warning(format!("↻ {}", self.label())),
            Self::Kept => painter.dim(format!("• {}", self.label())),
        }
    }
}

/// What the scaffolding did, in the order it did it.
#[derive(Debug)]
struct Report {
    entries: Vec<(String, Outcome)>,
    /// Whether a document was left untouched. Only files can be kept in a
    /// meaningful way: a pre-existing directory is simply reused and never
    /// warrants the `--force` hint.
    kept_files: bool,
}

pub fn run(path: &Path, args: &InitArgs, painter: Painter) -> Result<()> {
    let name = project_name(&args.name)?;

    let root = Workspace::resolve_root(path)?;
    let markers = Workspace::detect_markers(&root);
    if markers.complete() && !args.force {
        return Err(Error::AlreadyInitialized(root));
    }

    super::emit(&render_header(name, &root, painter));

    let report = scaffold(&root, name, &args.description, args.force)?;
    super::emit(&render_report(&report, painter));
    super::emit(&render_footer(&report, &root, painter));

    Ok(())
}

/// Accepts a name that can head a document and name a file, and nothing else.
fn project_name(raw: &str) -> Result<&str> {
    let name = raw.trim();
    if name.is_empty()
        || name
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '\0'))
    {
        return Err(Error::InvalidProjectName(raw.to_string()));
    }
    Ok(name)
}

/// Writes the directories and documents of a fresh workspace.
fn scaffold(root: &Path, name: &str, description: &str, force: bool) -> Result<Report> {
    let today = util::format_date(Some(SystemTime::now()));
    let config = WorkspaceConfig::scaffold(name, description, &i18n::current());

    let mut entries: Vec<(String, Outcome)> = Vec::new();

    for directory in workspace::SCAFFOLD_DIRECTORIES {
        let outcome = create_directory(&root.join(directory))?;
        entries.push((format!("{directory}/"), outcome));
    }

    let files: Vec<(PathBuf, String)> = vec![
        (
            root.join(config::DESIGN_FILE),
            template::design(name, description, &today),
        ),
        (root.join(config::CONFIG_FILE), config.to_yaml()?),
        (
            root.join(config::SPECS_DIR).join("README.md"),
            template::specs_readme(name),
        ),
        (
            root.join(config::SPECS_DIR)
                .join(format!("{}.md", template::slugify(name))),
            template::example_spec(name, &today),
        ),
        (
            root.join(config::CHANGES_DIR).join("README.md"),
            template::changes_readme(name),
        ),
        (
            root.join(config::CHANGES_DIR)
                .join(format!("bootstrap-{}.md", template::slugify(name))),
            template::example_change(name, &today),
        ),
    ];

    let mut kept_files = false;

    for (target, contents) in files {
        let outcome = write_file(&target, &contents, force)?;
        kept_files |= outcome == Outcome::Kept;

        let label = target
            .strip_prefix(root)
            .unwrap_or(&target)
            .display()
            .to_string();
        entries.push((label, outcome));
    }

    Ok(Report {
        entries,
        kept_files,
    })
}

fn render_header(name: &str, root: &Path, painter: Painter) -> Vec<String> {
    vec![
        String::new(),
        format!(
            "  {} {}",
            painter.accent_bold("◆"),
            painter.bold(t!("init.header", name = name))
        ),
        format!(
            "  {} {}",
            painter.dim(format!("{}:", t!("init.target"))),
            painter.primary(root.display())
        ),
        String::new(),
    ]
}

fn render_report(report: &Report, painter: Painter) -> Vec<String> {
    let width = report
        .entries
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);

    report
        .entries
        .iter()
        .map(|(label, outcome)| {
            format!(
                "    {} {}  {}",
                painter.dim("│"),
                painter.text(util::pad_right(label, width)),
                outcome.render(painter)
            )
        })
        .collect()
}

fn render_footer(report: &Report, root: &Path, painter: Painter) -> Vec<String> {
    let mut out = vec![
        String::new(),
        format!(
            "  {} {}",
            painter.success("✔"),
            painter.bold(t!("init.done"))
        ),
    ];

    if report.kept_files {
        out.push(format!(
            "  {} {}",
            painter.warning("!"),
            painter.dim(t!("init.partial"))
        ));
    }

    let display_root = display_for_hint(root);
    out.push(String::new());
    out.push(format!("  {}", painter.dim(t!("init.next_steps"))));
    out.push(format!(
        "    {} {}",
        painter.dim("›"),
        painter.primary(t!("init.next_list", path = display_root))
    ));
    out.push(format!(
        "    {} {}",
        painter.dim("›"),
        painter.primary(t!("init.next_tui", path = display_root))
    ));
    out.push(String::new());

    out
}

fn create_directory(target: &Path) -> Result<Outcome> {
    if target.is_dir() {
        return Ok(Outcome::Kept);
    }
    std::fs::create_dir_all(target).map_err(|source| Error::io(target, source))?;
    Ok(Outcome::Created)
}

fn write_file(target: &Path, contents: &str, force: bool) -> Result<Outcome> {
    let exists = target.exists();
    if exists && !force {
        return Ok(Outcome::Kept);
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::io(parent, source))?;
    }
    std::fs::write(target, contents).map_err(|source| Error::io(target, source))?;

    Ok(if exists {
        Outcome::Overwritten
    } else {
        Outcome::Created
    })
}

/// Shortest sensible representation of the workspace root for the hints.
fn display_for_hint(root: &Path) -> String {
    let Ok(current) = std::env::current_dir() else {
        return root.display().to_string();
    };
    if root == current {
        return ".".to_string();
    }
    match root.strip_prefix(&current) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.display().to_string(),
        _ => root.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::commands::tests::{Fixture, plain};

    fn args(name: &str, force: bool) -> InitArgs {
        InitArgs {
            name: name.to_string(),
            description: "A workspace to write into.".to_string(),
            force,
        }
    }

    fn outcome(report: &Report, label: &str) -> Outcome {
        report
            .entries
            .iter()
            .find(|(entry, _)| entry == label)
            .unwrap_or_else(|| panic!("no entry for {label} in {report:#?}"))
            .1
    }

    #[test]
    fn a_name_that_cannot_be_a_path_is_refused() {
        for name in ["", "   ", "a/b", "a\\b", "a\0b"] {
            match project_name(name) {
                Err(Error::InvalidProjectName(reported)) => assert_eq!(reported, name),
                other => panic!("{name:?} should be refused, got {other:?}"),
            }
        }

        assert_eq!(project_name("  Demo Project  ").unwrap(), "Demo Project");
    }

    #[test]
    fn a_refused_name_never_touches_the_filesystem() {
        let fixture = Fixture::scratch();

        assert!(matches!(
            run(&fixture.root, &args("bad/name", false), plain()),
            Err(Error::InvalidProjectName(_))
        ));
        assert_eq!(std::fs::read_dir(&fixture.root).unwrap().count(), 0);
    }

    #[test]
    fn the_scaffold_writes_every_directory_and_document() {
        let fixture = Fixture::scratch();
        let report = scaffold(&fixture.root, "Demo Project", "", false).unwrap();

        for label in [
            "specs/",
            "changes/",
            "design.md",
            "openspec.yaml",
            "specs/README.md",
            "specs/demo-project.md",
            "changes/README.md",
            "changes/bootstrap-demo-project.md",
        ] {
            assert_eq!(outcome(&report, label), Outcome::Created, "{label}");
        }
        assert!(!report.kept_files);

        // The result is a workspace the rest of the tool can open.
        let workspace = Workspace::open(&fixture.root).unwrap();
        assert!(workspace.markers.complete());
        assert_eq!(workspace.project_name(), "Demo Project");
    }

    #[test]
    fn the_configuration_records_the_interface_language() {
        let fixture = Fixture::scratch();
        scaffold(&fixture.root, "Demo Project", "Short text.", false).unwrap();

        let raw = std::fs::read_to_string(fixture.root.join("openspec.yaml")).unwrap();
        assert!(
            raw.contains(&format!("language: {}", i18n::current())),
            "{raw}"
        );
        assert!(raw.contains("Short text."));
    }

    #[test]
    fn a_second_pass_keeps_the_documents_that_are_already_there() {
        let fixture = Fixture::scratch();
        scaffold(&fixture.root, "Demo Project", "", false).unwrap();
        std::fs::write(fixture.root.join("design.md"), "# Mine\n").unwrap();

        let report = scaffold(&fixture.root, "Demo Project", "", false).unwrap();

        assert_eq!(outcome(&report, "design.md"), Outcome::Kept);
        assert!(report.kept_files);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("design.md")).unwrap(),
            "# Mine\n"
        );
    }

    #[test]
    fn forcing_rewrites_the_documents_and_reports_that_it_did() {
        let fixture = Fixture::scratch();
        scaffold(&fixture.root, "Demo Project", "", false).unwrap();
        std::fs::write(fixture.root.join("design.md"), "# Mine\n").unwrap();

        let report = scaffold(&fixture.root, "Demo Project", "", true).unwrap();

        assert_eq!(outcome(&report, "design.md"), Outcome::Overwritten);
        assert!(!report.kept_files);
        assert!(
            std::fs::read_to_string(fixture.root.join("design.md"))
                .unwrap()
                .contains("Design")
        );
    }

    /// A directory that already exists is reused; only a kept *file* means
    /// something the user wrote was left alone.
    #[test]
    fn a_pre_existing_directory_does_not_ask_for_force() {
        let fixture = Fixture::scratch();
        std::fs::create_dir_all(fixture.root.join("specs")).unwrap();

        let report = scaffold(&fixture.root, "Demo Project", "", false).unwrap();

        assert_eq!(outcome(&report, "specs/"), Outcome::Kept);
        assert!(!report.kept_files);
        assert!(
            !render_footer(&report, &fixture.root, plain())
                .iter()
                .any(|line| line.contains("--force"))
        );
    }

    #[test]
    fn an_initialised_workspace_is_refused_unless_force_is_given() {
        let fixture = Fixture::scratch();
        assert!(run(&fixture.root, &args("Demo Project", false), plain()).is_ok());

        assert!(matches!(
            run(&fixture.root, &args("Demo Project", false), plain()),
            Err(Error::AlreadyInitialized(_))
        ));
        assert!(run(&fixture.root, &args("Demo Project", true), plain()).is_ok());
    }

    #[test]
    fn the_heading_names_the_project_and_where_it_is_going() {
        let fixture = Fixture::scratch();
        let lines = render_header("Demo Project", &fixture.root, plain());

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "");
        assert!(lines[1].contains("Demo Project"));
        assert!(lines[2].contains("Target"));
        assert!(lines[2].contains("openspectation-cmd"));
        assert_eq!(lines[3], "");
    }

    #[test]
    fn the_report_aligns_every_outcome_in_one_column() {
        let fixture = Fixture::scratch();
        let report = scaffold(&fixture.root, "Demo Project", "", false).unwrap();
        let lines = render_report(&report, plain());

        assert_eq!(lines.len(), report.entries.len());

        let columns: Vec<usize> = lines
            .iter()
            .map(|line| line.find('✔').expect("an outcome badge"))
            .collect();
        assert!(
            columns.windows(2).all(|pair| pair[0] == pair[1]),
            "the outcomes are ragged: {lines:#?}"
        );
    }

    #[test]
    fn every_outcome_carries_its_own_glyph_and_label() {
        assert!(Outcome::Created.render(plain()).starts_with("✔ created"));
        assert!(
            Outcome::Overwritten
                .render(plain())
                .starts_with("↻ overwritten")
        );
        assert!(
            Outcome::Kept
                .render(plain())
                .starts_with("• already exists")
        );
    }

    #[test]
    fn the_footer_asks_for_force_only_when_a_document_was_kept() {
        let fixture = Fixture::scratch();

        let untouched = Report {
            entries: Vec::new(),
            kept_files: false,
        };
        let lines = render_footer(&untouched, &fixture.root, plain());
        assert!(lines.iter().any(|line| line.contains("Workspace ready.")));
        assert!(!lines.iter().any(|line| line.contains("--force")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("openspectation list"))
        );
        assert!(lines.iter().any(|line| line.contains("openspectation tui")));

        let kept = Report {
            entries: Vec::new(),
            kept_files: true,
        };
        assert!(
            render_footer(&kept, &fixture.root, plain())
                .iter()
                .any(|line| line.contains("--force"))
        );
    }

    #[test]
    fn the_hint_prefers_a_path_relative_to_the_current_directory() {
        let current = std::env::current_dir().unwrap();

        assert_eq!(display_for_hint(&current), ".");
        assert_eq!(display_for_hint(&current.join("workspace")), "workspace");

        let elsewhere = Path::new("/nowhere/at/all");
        assert_eq!(display_for_hint(elsewhere), "/nowhere/at/all");
    }
}
