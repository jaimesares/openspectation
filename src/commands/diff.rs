//! `openspectation diff [CHANGE]` — requirement deltas, on stdout.
//!
//! The selection is resolved before anything is rendered, so an unknown change
//! is refused instead of producing a heading followed by nothing.

use std::path::Path;

use rust_i18n::t;

use crate::cli::DiffArgs;
use crate::domain::change::ChangeProposal;
use crate::domain::delta::{self, CapabilityDelta, DeltaKind};
use crate::domain::workspace::Workspace;
use crate::error::{Error, Result};
use crate::paint::Painter;

const INDENT: &str = "    ";

pub fn run(path: &Path, args: &DiffArgs, painter: Painter) -> Result<()> {
    let workspace = Workspace::open(path)?;
    let selection = select(&workspace, args.change.as_deref())?;
    super::emit(&render(&selection, &workspace, painter));
    Ok(())
}

/// The proposals to report on: one named change, or every active one.
fn select<'a>(workspace: &'a Workspace, change: Option<&str>) -> Result<Vec<&'a ChangeProposal>> {
    match change {
        Some(id) => workspace
            .find_change(id)
            .map(|found| vec![found])
            .ok_or_else(|| Error::UnknownChange(id.to_string())),
        None => Ok(workspace
            .changes
            .iter()
            .filter(|change| !change.archived)
            .collect()),
    }
}

fn render(selection: &[&ChangeProposal], workspace: &Workspace, painter: Painter) -> Vec<String> {
    let mut out = vec![
        String::new(),
        format!(
            "  {} {}",
            painter.accent_bold("◆"),
            painter.bold(t!("tui.diff.title"))
        ),
        String::new(),
    ];

    let mut reported = false;

    for change in selection {
        let deltas = delta::for_change(change, workspace);
        if deltas.is_empty() {
            continue;
        }
        reported = true;

        out.push(format!(
            "{INDENT}{} {}",
            painter.dim("▸"),
            painter.bold(&change.id)
        ));

        for capability in &deltas {
            out.extend(render_capability(capability, painter));
        }
        out.push(String::new());
    }

    if !reported {
        out.push(format!("{INDENT}{}", painter.dim(t!("tui.empty.diff"))));
        out.push(String::new());
    }

    out
}

fn render_capability(capability: &CapabilityDelta, painter: Painter) -> Vec<String> {
    let status = if capability.existing {
        t!("tui.diff.existing", count = capability.current_requirements)
    } else {
        t!("tui.diff.new_capability")
    };

    let mut out = vec![format!(
        "{INDENT}  {} {}  {}",
        painter.accent("▎"),
        painter.text(&capability.capability),
        painter.dim(status)
    )];

    for requirement in &capability.requirements {
        let head = format!("{} {}", requirement.kind.glyph(), requirement.kind.tag());
        out.push(format!(
            "{INDENT}    {} {}",
            colored_tag(&head, requirement.kind, painter),
            painter.text(&requirement.name)
        ));

        let mut trailer = format!("L{}", requirement.line);
        if requirement.scenarios > 0 {
            trailer.push_str(&format!(
                " · {}",
                t!("tui.diff.scenarios", count = requirement.scenarios)
            ));
        }
        out.push(format!("{INDENT}      {}", painter.dim(trailer)));
    }

    out
}

fn colored_tag(text: &str, kind: DeltaKind, painter: Painter) -> String {
    match kind {
        DeltaKind::Added => painter.success(text),
        DeltaKind::Modified => painter.warning(text),
        DeltaKind::Removed => painter.danger_bold(text),
        DeltaKind::Renamed => painter.info(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::commands::tests::{Fixture, plain};

    fn args(change: Option<&str>) -> DiffArgs {
        DiffArgs {
            change: change.map(str::to_string),
        }
    }

    fn rendered(fixture: &Fixture, change: Option<&str>) -> Vec<String> {
        let workspace = fixture.workspace();
        let selection = select(&workspace, change).unwrap();
        render(&selection, &workspace, plain())
    }

    #[test]
    fn every_declared_operation_is_tagged_with_its_keyword() {
        let fixture = Fixture::new();
        let lines = rendered(&fixture, None);

        assert!(lines.iter().any(|line| line.contains("Requirement delta")));
        assert!(lines.iter().any(|line| line.contains("▸ add-oauth")));
        assert!(lines.iter().any(|line| {
            line.contains("+ ADDED") && line.contains("Tokens SHALL refresh silently")
        }));
        assert!(lines.iter().any(|line| line.contains("- REMOVED")));
        assert!(lines.iter().any(|line| line.contains("~ MODIFIED")));
        assert!(lines.iter().any(|line| line.contains("» RENAMED")));
    }

    #[test]
    fn a_capability_the_workspace_already_has_is_reported_against_its_size() {
        let fixture = Fixture::new();
        let lines = rendered(&fixture, None);

        let auth = lines
            .iter()
            .find(|line| line.contains("▎ auth"))
            .expect("the auth capability");
        assert!(auth.contains("current specification"));
        assert!(auth.contains("1 requirements"), "{auth}");

        let telemetry = lines
            .iter()
            .find(|line| line.contains("▎ telemetry"))
            .expect("the telemetry capability");
        assert!(telemetry.contains("new capability"));
    }

    #[test]
    fn a_requirement_reports_its_line_and_only_counts_scenarios_when_it_has_them() {
        let fixture = Fixture::new();
        let lines = rendered(&fixture, None);

        let position = lines
            .iter()
            .position(|line| line.contains("Tokens SHALL refresh silently"))
            .expect("the added requirement");
        assert!(lines[position + 1].trim().starts_with('L'));
        assert!(lines[position + 1].contains("1 scenarios"));

        let removed = lines
            .iter()
            .position(|line| line.contains("Sessions SHALL live forever"))
            .expect("the removed requirement");
        assert!(!lines[removed + 1].contains("scenarios"));
    }

    #[test]
    fn an_archived_change_is_left_out_of_the_default_selection() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        let ids: Vec<&str> = select(&workspace, None)
            .unwrap()
            .iter()
            .map(|change| change.id.as_str())
            .collect();
        assert_eq!(ids, ["add-oauth"]);

        // Naming it explicitly still reaches it.
        let named = select(&workspace, Some("2026-01-01-old-thing")).unwrap();
        assert_eq!(named.len(), 1);
        assert!(named[0].archived);
    }

    #[test]
    fn a_unique_prefix_addresses_the_change_it_names() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        let selection = select(&workspace, Some("add-o")).unwrap();
        assert_eq!(selection[0].id, "add-oauth");
    }

    #[test]
    fn an_unknown_change_is_refused_by_name() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        match select(&workspace, Some("no-such-change")) {
            Err(Error::UnknownChange(name)) => assert_eq!(name, "no-such-change"),
            other => panic!("expected an unknown change, got {other:?}"),
        }

        assert!(matches!(
            run(&fixture.root, &args(Some("no-such-change")), plain()),
            Err(Error::UnknownChange(_))
        ));
    }

    /// A change that touches no capability is skipped rather than given an
    /// empty heading of its own, and a selection of nothing but those reads as
    /// the empty result it is.
    #[test]
    fn a_change_without_a_delta_is_skipped_and_leaves_the_report_empty() {
        let fixture = Fixture::new();
        let lines = rendered(&fixture, Some("2026-01-01-old-thing"));

        assert!(
            lines
                .iter()
                .any(|line| line.contains("declares no requirement delta"))
        );
        assert!(!lines.iter().any(|line| line.contains("▸ ")));

        let bare = Fixture::bare();
        assert!(
            render(&[], &bare.workspace(), plain())
                .iter()
                .any(|line| line.contains("declares no requirement delta"))
        );
    }

    #[test]
    fn the_command_prints_the_delta_it_renders() {
        let fixture = Fixture::new();
        assert!(run(&fixture.root, &args(None), plain()).is_ok());
        assert!(run(&fixture.root, &args(Some("add-oauth")), plain()).is_ok());
    }
}
