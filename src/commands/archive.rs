//! `openspectation archive <CHANGE>` — delegated to the upstream CLI.
//!
//! Archiving moves a directory and rewrites specifications, so it asks before
//! acting unless `--yes` is given, and prints what the CLI reported verbatim.
//!
//! The output is printed in two stages on purpose: the heading announces the
//! change before the subprocess starts, and the result follows once it has
//! finished. Building the whole block first would leave the terminal blank for
//! as long as the CLI takes.

use std::io::{IsTerminal, Write};
use std::path::Path;

use rust_i18n::t;

use crate::cli::ArchiveArgs;
use crate::domain::openspec;
use crate::domain::workspace::Workspace;
use crate::error::{Error, Result};
use crate::paint::Painter;

const INDENT: &str = "    ";

pub fn run(path: &Path, args: &ArchiveArgs, painter: Painter) -> Result<()> {
    let workspace = Workspace::open(path)?;
    let id = resolve_target(&workspace, &args.change)?;

    if !args.yes && !confirm(&id, painter)? {
        super::emit(&[render_cancelled(painter)]);
        return Ok(());
    }

    super::emit(&render_header(&id, painter));
    let lines = openspec::archive(&workspace.root, &id, args.skip_specs)?;
    super::emit(&render_result(&lines, painter));

    Ok(())
}

/// The identifier to archive, refusing what cannot be archived.
fn resolve_target(workspace: &Workspace, requested: &str) -> Result<String> {
    let change = workspace
        .find_change(requested)
        .ok_or_else(|| Error::UnknownChange(requested.to_string()))?;

    if change.archived {
        return Err(Error::AlreadyArchived(change.id.clone()));
    }

    Ok(change.id.clone())
}

fn render_header(change: &str, painter: Painter) -> Vec<String> {
    vec![
        String::new(),
        format!(
            "  {} {}",
            painter.accent_bold("◆"),
            painter.bold(t!("tui.log.archiving", change = change))
        ),
        String::new(),
    ]
}

fn render_result(lines: &[String], painter: Painter) -> Vec<String> {
    let mut out: Vec<String> = lines
        .iter()
        .map(|line| format!("{INDENT}{}", painter.text(line)))
        .collect();

    out.push(String::new());
    out.push(format!(
        "{INDENT}{}",
        painter.success(t!("tui.log.archived"))
    ));
    out.push(String::new());

    out
}

fn render_cancelled(painter: Painter) -> String {
    format!("{INDENT}{}", painter.dim(t!("tui.log.cancelled")))
}

/// Asks on the terminal; a non-interactive stdin is a refusal, not a yes.
fn confirm(change: &str, painter: Painter) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        return Err(Error::ConfirmationRequired);
    }

    print!(
        "  {} {} ",
        painter.warning(t!("tui.confirm.archive", change = change)),
        painter.dim(t!("cli.cmd.archive.prompt"))
    );
    std::io::stdout().flush().map_err(Error::Terminal)?;

    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(Error::Terminal)?;

    Ok(accepted(&answer))
}

/// Whether a typed answer authorises the archive.
///
/// Both interface languages are accepted because the prompt is translated but
/// the key the user reaches for is not.
fn accepted(answer: &str) -> bool {
    matches!(
        answer.trim().to_lowercase().as_str(),
        "y" | "yes" | "s" | "si" | "sí"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::commands::tests::{Fixture, plain};

    fn args(change: &str, yes: bool) -> ArchiveArgs {
        ArchiveArgs {
            change: change.to_string(),
            yes,
            skip_specs: false,
        }
    }

    #[test]
    fn only_an_explicit_yes_authorises_the_archive() {
        for answer in ["y", "Y", "yes", "YES\n", " s ", "si", "sí", "Sí\r\n"] {
            assert!(accepted(answer), "{answer:?} should authorise");
        }
        for answer in ["", "\n", "n", "no", "yeah", "yep", "sure", "1"] {
            assert!(!accepted(answer), "{answer:?} should not authorise");
        }
    }

    #[test]
    fn an_unknown_change_is_refused_by_name() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        match resolve_target(&workspace, "no-such-change") {
            Err(Error::UnknownChange(name)) => assert_eq!(name, "no-such-change"),
            other => panic!("expected an unknown change, got {other:?}"),
        }

        // `--yes` skips the prompt, so nothing here can block on stdin.
        assert!(matches!(
            run(&fixture.root, &args("no-such-change", true), plain()),
            Err(Error::UnknownChange(_))
        ));
    }

    #[test]
    fn an_already_archived_change_is_refused_before_the_cli_is_reached() {
        let fixture = Fixture::new();
        let workspace = fixture.workspace();

        match resolve_target(&workspace, "2026-01-01-old-thing") {
            Err(Error::AlreadyArchived(name)) => assert_eq!(name, "2026-01-01-old-thing"),
            other => panic!("expected an archived change, got {other:?}"),
        }

        assert!(matches!(
            run(&fixture.root, &args("2026-01-01-old-thing", true), plain()),
            Err(Error::AlreadyArchived(_))
        ));
    }

    #[test]
    fn a_unique_prefix_addresses_the_change_it_names() {
        let fixture = Fixture::new();
        assert_eq!(
            resolve_target(&fixture.workspace(), "add-o").unwrap(),
            "add-oauth"
        );
    }

    #[test]
    fn the_heading_names_the_change_being_archived() {
        let lines = render_header("add-oauth", plain());

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "");
        assert!(lines[1].contains("add-oauth"));
        assert_eq!(lines[2], "");
    }

    #[test]
    fn the_result_repeats_what_the_cli_reported_and_closes_on_a_confirmation() {
        let reported = vec![
            "✔ moved changes/add-oauth".to_string(),
            "✔ updated specs/auth/spec.md".to_string(),
        ];
        let lines = render_result(&reported, plain());

        assert_eq!(lines[0], "    ✔ moved changes/add-oauth");
        assert_eq!(lines[1], "    ✔ updated specs/auth/spec.md");
        assert_eq!(lines[2], "");
        assert!(lines[3].contains("Archived."));
        assert_eq!(lines[4], "");
    }

    #[test]
    fn a_silent_cli_still_produces_the_closing_confirmation() {
        let lines = render_result(&[], plain());

        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("Archived."));
    }

    #[test]
    fn a_refused_prompt_reports_the_cancellation() {
        assert!(render_cancelled(plain()).contains("Cancelled."));
    }
}
