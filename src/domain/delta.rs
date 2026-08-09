//! Requirement-level deltas declared by a change proposal.
//!
//! An OpenSpec change does not rewrite a capability: it ships a delta file at
//! `changes/<id>/specs/<capability>/spec.md` whose sections declare what the
//! change does to that capability.
//!
//! ```text
//! ## ADDED Requirements
//!
//! ### Requirement: Undo SHALL restore the document
//!
//! #### Scenario: Undo reverts a formatting command
//! ```
//!
//! The operation is declared in the file rather than inferred, so the delta is
//! read directly instead of being computed against the current specification.
//! The current specification is still consulted, but only to report whether the
//! capability already exists and how large it is today.

use std::path::{Path, PathBuf};

use rust_i18n::t;

use crate::domain::change::ChangeProposal;
use crate::domain::spec::SpecDocument;
use crate::domain::workspace::Workspace;

/// Directory inside a change that holds its capability deltas.
const DELTA_DIR: &str = "specs";

/// What a change does to a requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeltaKind {
    Added,
    Modified,
    Removed,
    Renamed,
}

impl DeltaKind {
    /// Reads the operation from a `## ADDED Requirements` style heading.
    pub fn parse(heading: &str) -> Option<Self> {
        let first = heading.split_whitespace().next()?;
        match first.to_ascii_uppercase().as_str() {
            "ADDED" => Some(Self::Added),
            "MODIFIED" => Some(Self::Modified),
            "REMOVED" => Some(Self::Removed),
            "RENAMED" => Some(Self::Renamed),
            _ => None,
        }
    }

    /// Uppercase operation exactly as it appears in the delta file.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Added => "ADDED",
            Self::Modified => "MODIFIED",
            Self::Removed => "REMOVED",
            Self::Renamed => "RENAMED",
        }
    }

    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Added => "+",
            Self::Modified => "~",
            Self::Removed => "-",
            Self::Renamed => "»",
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Added => t!("delta.added"),
            Self::Modified => t!("delta.modified"),
            Self::Removed => t!("delta.removed"),
            Self::Renamed => t!("delta.renamed"),
        }
        .to_string()
    }
}

/// One requirement touched by a change.
#[derive(Debug, Clone)]
pub struct RequirementDelta {
    pub kind: DeltaKind,
    pub name: String,
    pub scenarios: usize,
    /// One-based line of the `### Requirement:` heading in the delta file.
    pub line: usize,
}

/// Every requirement a change touches in a single capability.
#[derive(Debug, Clone)]
pub struct CapabilityDelta {
    pub capability: String,
    pub path: PathBuf,
    /// `true` when `specs/<capability>/` already exists in the workspace.
    pub existing: bool,
    /// Requirements the current specification declares today, if it exists.
    pub current_requirements: usize,
    pub requirements: Vec<RequirementDelta>,
}

impl CapabilityDelta {
    /// Number of requirements affected by a single operation.
    pub fn count(&self, kind: DeltaKind) -> usize {
        self.requirements
            .iter()
            .filter(|delta| delta.kind == kind)
            .count()
    }

    /// The operation to show as the headline badge for this capability.
    pub fn headline(&self) -> Option<DeltaKind> {
        self.requirements.first().map(|delta| delta.kind)
    }
}

/// Reads every capability delta declared by a change.
pub fn for_change(change: &ChangeProposal, workspace: &Workspace) -> Vec<CapabilityDelta> {
    let mut out: Vec<CapabilityDelta> = Vec::new();

    for document in &change.documents {
        let Some(capability) = capability_of(&change.path, &document.path) else {
            continue;
        };

        let requirements = parse(&document.body);
        if requirements.is_empty() {
            continue;
        }

        let current = current_specification(workspace, &capability);

        out.push(CapabilityDelta {
            capability,
            path: document.path.clone(),
            existing: current.is_some(),
            current_requirements: current.map_or(0, count_requirement_headings),
            requirements,
        });
    }

    out.sort_by(|left, right| left.capability.cmp(&right.capability));
    out
}

/// Capability a delta document belongs to, or `None` for other documents.
///
/// Only `specs/<capability>/…` inside the change is a delta; `proposal.md`,
/// `tasks.md` and `design.md` describe the change, they do not alter a spec.
fn capability_of(change_root: &Path, document: &Path) -> Option<String> {
    let relative = document.strip_prefix(change_root).ok()?;
    let mut components = relative.components();

    if components.next()?.as_os_str() != DELTA_DIR {
        return None;
    }

    let capability = components.next()?.as_os_str().to_str()?.to_string();
    (!capability.is_empty()).then_some(capability)
}

/// The workspace document describing a capability as it stands today.
fn current_specification<'a>(
    workspace: &'a Workspace,
    capability: &str,
) -> Option<&'a SpecDocument> {
    workspace.specs.iter().find(|document| {
        document
            .relative
            .components()
            .next()
            .is_some_and(|component| component.as_os_str() == capability)
    })
}

fn count_requirement_headings(document: &SpecDocument) -> usize {
    document
        .body
        .lines()
        .filter(|line| requirement_name(line.trim()).is_some())
        .count()
}

/// Parses the delta sections of a capability delta document.
pub fn parse(body: &str) -> Vec<RequirementDelta> {
    let mut out: Vec<RequirementDelta> = Vec::new();
    let mut kind: Option<DeltaKind> = None;
    let mut in_code_fence = false;

    for (index, raw) in body.lines().enumerate() {
        let line = raw.trim();

        if line.starts_with("```") || line.starts_with("~~~") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
            continue;
        }

        if let Some(heading) = line.strip_prefix("## ") {
            // A section that is not an operation ends the current one, so a
            // trailing `## Notes` cannot swallow requirements into it.
            kind = DeltaKind::parse(heading.trim());
            continue;
        }

        let Some(current) = kind else {
            continue;
        };

        if let Some(name) = requirement_name(line) {
            out.push(RequirementDelta {
                kind: current,
                name,
                scenarios: 0,
                line: index + 1,
            });
            continue;
        }

        if line.starts_with("#### Scenario:")
            && let Some(last) = out.last_mut()
        {
            last.scenarios += 1;
        }
    }

    out
}

/// Name declared by a `### Requirement: <name>` heading.
fn requirement_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("### Requirement:")?.trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT: &str = "\
## Purpose

Prose that is not a delta.

## ADDED Requirements

### Requirement: Undo SHALL restore the document

#### Scenario: Undo reverts formatting
#### Scenario: Undo restores the selection

### Requirement: Redo SHALL reapply a step

## REMOVED Requirements

### Requirement: The stack SHALL be cleared on load

## Notes

### Requirement: Not a delta, the section ended
";

    #[test]
    fn reads_the_operation_from_the_section_heading() {
        let deltas = parse(DOCUMENT);
        let kinds: Vec<DeltaKind> = deltas.iter().map(|delta| delta.kind).collect();
        assert_eq!(
            kinds,
            [DeltaKind::Added, DeltaKind::Added, DeltaKind::Removed]
        );
    }

    #[test]
    fn counts_scenarios_per_requirement() {
        let deltas = parse(DOCUMENT);
        assert_eq!(deltas[0].scenarios, 2);
        assert_eq!(deltas[1].scenarios, 0);
    }

    #[test]
    fn a_non_operation_section_ends_the_previous_one() {
        let deltas = parse(DOCUMENT);
        assert_eq!(deltas.len(), 3);
        assert!(
            !deltas
                .iter()
                .any(|delta| delta.name.contains("Not a delta"))
        );
    }

    #[test]
    fn ignores_headings_inside_code_fences() {
        let raw = "## ADDED Requirements\n\n```md\n### Requirement: Example\n```\n";
        assert!(parse(raw).is_empty());
    }

    #[test]
    fn only_the_specs_directory_of_a_change_holds_deltas() {
        let root = Path::new("/w/changes/add-history");
        assert_eq!(
            capability_of(root, Path::new("/w/changes/add-history/specs/edit/spec.md")).as_deref(),
            Some("edit")
        );
        assert_eq!(
            capability_of(root, Path::new("/w/changes/add-history/tasks.md")),
            None
        );
    }
}
