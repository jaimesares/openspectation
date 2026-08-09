//! `openspectation validate [CHANGE]` — delegated to the upstream CLI.
//!
//! The exit status mirrors the validation result so this is usable in CI:
//! `0` when everything passes, `1` when anything fails.
//!
//! The report is rendered from the parsed result rather than printed as the
//! items arrive, which keeps the presentation testable without the upstream
//! CLI being installed.

use std::path::Path;

use rust_i18n::t;

use crate::cli::ValidateArgs;
use crate::domain::openspec::{self, Severity, ValidationReport};
use crate::domain::workspace::Workspace;
use crate::error::{Error, Result};
use crate::paint::Painter;

const INDENT: &str = "    ";

pub fn run(path: &Path, args: &ValidateArgs, painter: Painter) -> Result<()> {
    let workspace = Workspace::open(path)?;
    let target = resolve_target(&workspace, args.change.as_deref())?;

    let report = openspec::validate(&workspace.root, target.as_deref(), !args.loose)?;
    super::emit(&render(&report, painter));

    if report.is_ok() {
        Ok(())
    } else {
        // The findings went to stdout; stderr gets the one line a script or a
        // CI log needs, and the process exits non-zero.
        Err(Error::ValidationFailed(report.failed))
    }
}

/// The item to hand the upstream CLI: the canonical identifier of the change
/// that was addressed, or `None` for the whole workspace.
///
/// Resolving here means `--change add-o` reaches `add-oauth` and a typo is
/// refused before a process is spawned for it.
fn resolve_target(workspace: &Workspace, change: Option<&str>) -> Result<Option<String>> {
    let Some(requested) = change else {
        return Ok(None);
    };

    workspace
        .find_change(requested)
        .map(|found| Some(found.id.clone()))
        .ok_or_else(|| Error::UnknownChange(requested.to_string()))
}

fn render(report: &ValidationReport, painter: Painter) -> Vec<String> {
    let mut out = vec![
        String::new(),
        format!(
            "  {} {}",
            painter.accent_bold("◆"),
            painter.bold(t!("cli.cmd.validate.about"))
        ),
        String::new(),
    ];

    for item in &report.items {
        let glyph = if item.valid { "✔" } else { "✖" };
        let head = if item.valid {
            painter.success(format!("{glyph} {}", item.id))
        } else {
            painter.danger_bold(format!("{glyph} {}", item.id))
        };
        out.push(format!(
            "{INDENT}{head}  {}",
            painter.dim(format!("{} · {} ms", item.kind, item.duration_ms))
        ));

        for finding in &item.findings {
            let text = format!(
                "{} {} — {}",
                finding.severity.glyph(),
                finding.location,
                finding.message
            );
            let painted = match finding.severity {
                Severity::Error => painter.danger_bold(text),
                Severity::Warning => painter.warning(text),
                Severity::Info => painter.dim(text),
            };
            out.push(format!("{INDENT}    {painted}"));
        }
    }

    let summary = t!(
        "tui.log.validated",
        passed = report.passed,
        failed = report.failed,
        errors = report.count(Severity::Error),
        warnings = report.count(Severity::Warning),
        ms = report.total_duration()
    );

    out.push(String::new());
    out.push(format!(
        "{INDENT}{}",
        if report.is_ok() {
            painter.success(summary)
        } else {
            painter.danger_bold(summary)
        }
    ));
    out.push(String::new());

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::commands::tests::{Fixture, plain};
    use crate::domain::openspec::{Finding, ItemReport};

    fn finding(severity: Severity, location: &str, message: &str) -> Finding {
        Finding {
            severity,
            location: location.to_string(),
            message: message.to_string(),
        }
    }

    fn failing_report() -> ValidationReport {
        ValidationReport {
            items: vec![
                ItemReport {
                    id: "add-oauth".to_string(),
                    kind: "change".to_string(),
                    valid: false,
                    duration_ms: 7,
                    findings: vec![
                        finding(Severity::Error, "requirements[0]", "no scenario"),
                        finding(Severity::Warning, "requirements[1]", "very long"),
                        finding(Severity::Info, "requirements[2]", "consider splitting"),
                    ],
                },
                ItemReport {
                    id: "billing".to_string(),
                    kind: "spec".to_string(),
                    valid: true,
                    duration_ms: 3,
                    findings: Vec::new(),
                },
            ],
            passed: 1,
            failed: 1,
        }
    }

    #[test]
    fn a_failing_item_is_marked_and_every_finding_listed_under_it() {
        let lines = render(&failing_report(), plain());

        let failed = lines
            .iter()
            .position(|line| line.contains("✖ add-oauth"))
            .expect("the failing item");
        assert!(lines[failed].contains("change · 7 ms"));
        assert!(lines[failed + 1].contains("✖ requirements[0] — no scenario"));
        assert!(lines[failed + 2].contains("! requirements[1]"));
        assert!(lines[failed + 3].contains("· requirements[2]"));

        assert!(lines.iter().any(|line| line.contains("✔ billing")));
    }

    #[test]
    fn the_summary_counts_every_severity_and_the_total_time() {
        let lines = render(&failing_report(), plain());
        let summary = lines
            .iter()
            .find(|line| line.contains("passed"))
            .expect("the summary");

        assert!(summary.contains("1 passed"));
        assert!(summary.contains("1 failed"));
        assert!(summary.contains("1 errors"));
        assert!(summary.contains("1 warnings"));
        assert!(summary.contains("10 ms"), "{summary}");
    }

    #[test]
    fn a_report_without_a_single_item_still_prints_its_frame() {
        let lines = render(&ValidationReport::default(), plain());

        assert_eq!(lines.first().map(String::as_str), Some(""));
        assert!(lines.iter().any(|line| line.contains("Validate")));
        assert!(lines.iter().any(|line| line.contains("0 passed")));
        assert_eq!(lines.last().map(String::as_str), Some(""));
    }

    #[test]
    fn the_target_is_the_resolved_identifier_rather_than_what_was_typed() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        assert_eq!(
            resolve_target(&workspace, Some("add-o")).unwrap(),
            Some("add-oauth".to_string())
        );
    }

    #[test]
    fn validating_the_whole_workspace_addresses_no_single_item() {
        let fixture = Fixture::new();
        assert_eq!(resolve_target(&fixture.workspace(), None).unwrap(), None);
    }

    #[test]
    fn an_unknown_change_is_refused_before_a_process_is_spawned_for_it() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        match resolve_target(&workspace, Some("no-such-change")) {
            Err(Error::UnknownChange(name)) => assert_eq!(name, "no-such-change"),
            other => panic!("expected an unknown change, got {other:?}"),
        }

        let args = ValidateArgs {
            change: Some("no-such-change".to_string()),
            loose: false,
        };
        assert!(matches!(
            run(&fixture.root, &args, plain()),
            Err(Error::UnknownChange(_))
        ));
    }

    /// A missing upstream CLI must be reported as such and never read as a
    /// workspace that passed. Whether it is installed is a property of the
    /// machine, so the assertion is made either way: without it the failure is
    /// exactly `CliMissing`, and with it the outcome comes from the report.
    #[test]
    fn the_outcome_comes_from_the_upstream_cli_or_says_it_is_missing() {
        let fixture = Fixture::new();
        let args = ValidateArgs {
            change: None,
            loose: true,
        };
        let outcome = run(&fixture.root, &args, plain());

        match openspec::version() {
            None => assert!(matches!(outcome, Err(Error::CliMissing))),
            Some(_) => assert!(
                matches!(
                    outcome,
                    Ok(()) | Err(Error::ValidationFailed(_) | Error::CliFailed { .. })
                ),
                "the installed CLI produced {outcome:?}"
            ),
        }
    }
}
