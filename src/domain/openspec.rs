//! Thin wrapper around the upstream `openspec` CLI.
//!
//! Validation rules and the archive procedure live in that tool, and
//! reimplementing either here would guarantee drift the first time it ships a
//! new rule. Both are therefore delegated, with `--json` so the result can be
//! rendered inside the dashboard instead of dumped as raw text.
//!
//! Reading is not delegated: the workspace model already parses the same files,
//! and paying a Node process start for every keystroke would be absurd.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::domain::config;
use crate::error::{Error, Result};

/// Executable looked up on `PATH`.
pub const PROGRAM: &str = "openspec";

/// Severity attached to a single validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_uppercase().as_str() {
            "ERROR" => Self::Error,
            "WARNING" | "WARN" => Self::Warning,
            _ => Self::Info,
        }
    }

    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Info => "·",
            Self::Warning => "!",
            Self::Error => "✖",
        }
    }
}

/// One finding reported against an item.
#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    pub location: String,
    pub message: String,
}

/// Validation outcome for a single change or specification.
#[derive(Debug, Clone)]
pub struct ItemReport {
    pub id: String,
    pub kind: String,
    pub valid: bool,
    pub findings: Vec<Finding>,
    pub duration_ms: u64,
}

/// Everything the CLI reported for one invocation.
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub items: Vec<ItemReport>,
    pub passed: usize,
    pub failed: usize,
}

impl ValidationReport {
    pub fn is_ok(&self) -> bool {
        self.failed == 0
    }

    pub fn count(&self, severity: Severity) -> usize {
        self.items
            .iter()
            .flat_map(|item| item.findings.iter())
            .filter(|finding| finding.severity == severity)
            .count()
    }

    pub fn total_duration(&self) -> u64 {
        self.items.iter().map(|item| item.duration_ms).sum()
    }
}

/// Version string reported by the CLI, when it is installed.
///
/// This spawns a process, so callers are expected to probe once and keep the
/// answer rather than asking on every frame.
pub fn version() -> Option<String> {
    let output = Command::new(PROGRAM).arg("--version").output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Directory the CLI should run in.
///
/// It resolves its own root by walking upwards for an `openspec/` directory, so
/// a nested workspace has to be invoked from the repository that contains it.
pub fn working_directory(root: &Path) -> PathBuf {
    if root
        .file_name()
        .is_some_and(|name| name == config::WORKSPACE_DIR)
        && let Some(parent) = root.parent()
    {
        return parent.to_path_buf();
    }
    root.to_path_buf()
}

/// Runs `openspec validate` for one item, or for everything when `item` is
/// `None`.
pub fn validate(root: &Path, item: Option<&str>, strict: bool) -> Result<ValidationReport> {
    // A failing validation is a result, not an execution failure: the CLI exits
    // non-zero for it, so the payload is parsed before the status is judged.
    let output = run(PROGRAM, root, &validate_arguments(item, strict))?;
    read_validation(&output.stdout)
}

/// Runs `openspec archive` for one change.
///
/// Returns the lines the CLI printed, which are shown verbatim: the archive
/// procedure moves directories and rewrites specifications, and paraphrasing
/// what it did would be the wrong place to be creative.
pub fn archive(root: &Path, change: &str, skip_specs: bool) -> Result<Vec<String>> {
    let output = run(PROGRAM, root, &archive_arguments(change, skip_specs))?;
    if !output.status {
        return Err(Error::CliFailed {
            command: format!("archive {change}"),
            reason: first_meaningful_line(&output.stderr, &output.stdout),
        });
    }

    Ok(archive_lines(&output))
}

/// Most an archive run is allowed to report, so a chatty CLI cannot push the
/// dashboard off screen.
const MAX_ARCHIVE_LINES: usize = 24;

fn validate_arguments(item: Option<&str>, strict: bool) -> Vec<String> {
    let mut arguments: Vec<String> = vec!["validate".to_string()];
    match item {
        Some(name) => {
            arguments.push(name.to_string());
            arguments.push("--type".to_string());
            arguments.push("change".to_string());
        }
        None => arguments.push("--all".to_string()),
    }
    if strict {
        arguments.push("--strict".to_string());
    }
    arguments.push("--json".to_string());
    arguments.push("--no-interactive".to_string());
    arguments
}

fn archive_arguments(change: &str, skip_specs: bool) -> Vec<String> {
    let mut arguments = vec![
        "archive".to_string(),
        change.to_string(),
        "--yes".to_string(),
    ];
    if skip_specs {
        arguments.push("--skip-specs".to_string());
    }
    arguments
}

fn read_validation(stdout: &str) -> Result<ValidationReport> {
    let payload: RawValidation = parse_json(stdout, "validate")?;

    Ok(ValidationReport {
        passed: payload.summary.totals.passed,
        failed: payload.summary.totals.failed,
        items: payload.items.into_iter().map(ItemReport::from).collect(),
    })
}

fn archive_lines(output: &Output) -> Vec<String> {
    let mut lines: Vec<String> = output
        .stdout
        .lines()
        .chain(output.stderr.lines())
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(strip_ansi)
        .collect();
    lines.truncate(MAX_ARCHIVE_LINES);
    lines
}

#[derive(Debug)]
struct Output {
    status: bool,
    stdout: String,
    stderr: String,
}

/// Spawns `program` in the workspace's repository and captures both streams.
///
/// The program is a parameter rather than [`PROGRAM`] itself so the failure
/// modes — a missing executable against any other spawn error — can be tested
/// without an `openspec` installation.
fn run(program: &str, root: &Path, arguments: &[String]) -> Result<Output> {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(working_directory(root))
        // The CLI colours its output; the dashboard applies its own styling.
        .env("NO_COLOR", "1")
        .output()
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => Error::CliMissing,
            _ => Error::io(program, source),
        })?;

    Ok(Output {
        status: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Parses a JSON payload with the YAML reader.
///
/// YAML 1.2 is a superset of JSON, so the deserializer already in the
/// dependency tree reads these payloads correctly and no JSON crate is needed.
fn parse_json<T: for<'de> Deserialize<'de>>(raw: &str, command: &str) -> Result<T> {
    // The CLI occasionally prefixes the payload with a notice, so parsing
    // starts at the first brace rather than at the first byte.
    let start = raw.find('{').unwrap_or(0);
    serde_norway::from_str(&raw[start..]).map_err(|source| Error::CliFailed {
        command: command.to_string(),
        reason: source.to_string(),
    })
}

fn first_meaningful_line(stderr: &str, stdout: &str) -> String {
    stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(strip_ansi)
        .unwrap_or_else(|| "no output".to_string())
}

/// Removes CSI escape sequences from a line of CLI output.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut characters = line.chars();

    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            out.push(character);
            continue;
        }
        // Skip up to and including the final byte of the sequence.
        for escaped in characters.by_ref() {
            if escaped.is_ascii_alphabetic() {
                break;
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Wire format
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawValidation {
    items: Vec<RawItem>,
    summary: RawSummary,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawSummary {
    totals: RawTotals,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawTotals {
    passed: usize,
    failed: usize,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawItem {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    valid: bool,
    issues: Vec<RawIssue>,
    #[serde(rename = "durationMs")]
    duration_ms: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawIssue {
    level: String,
    path: String,
    message: String,
}

impl From<RawItem> for ItemReport {
    fn from(raw: RawItem) -> Self {
        Self {
            id: raw.id,
            kind: raw.kind,
            valid: raw.valid,
            duration_ms: raw.duration_ms,
            findings: raw
                .issues
                .into_iter()
                .map(|issue| Finding {
                    severity: Severity::parse(&issue.level),
                    location: issue.path,
                    message: issue.message,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD: &str = r#"{
      "items": [
        {"id": "add-history", "type": "change", "valid": false, "durationMs": 7,
         "issues": [{"level": "ERROR", "path": "requirements[0]", "message": "no scenario"},
                    {"level": "INFO", "path": "requirements[4]", "message": "very long"}]}
      ],
      "summary": {"totals": {"items": 1, "passed": 0, "failed": 1}}
    }"#;

    #[test]
    fn reads_the_cli_payload() {
        let report = read_validation(PAYLOAD).unwrap();

        assert!(!report.is_ok());
        assert_eq!((report.passed, report.failed), (0, 1));
        assert_eq!(report.count(Severity::Error), 1);
        assert_eq!(report.count(Severity::Info), 1);
        assert_eq!(report.count(Severity::Warning), 0);
        assert_eq!(report.total_duration(), 7);

        let item = &report.items[0];
        assert_eq!(item.id, "add-history");
        assert_eq!(item.kind, "change");
        assert!(!item.valid);
        assert_eq!(item.findings[0].location, "requirements[0]");
        assert_eq!(item.findings[0].message, "no scenario");
    }

    #[test]
    fn tolerates_a_notice_before_the_payload() {
        let noisy = format!("update available\n{PAYLOAD}");
        let raw: RawValidation = parse_json(&noisy, "validate").unwrap();
        assert_eq!(raw.items.len(), 1);
    }

    #[test]
    fn a_payload_that_cannot_be_read_names_the_command_that_produced_it() {
        let error = read_validation("{\"items\": [oops").unwrap_err();
        let Error::CliFailed { command, reason } = error else {
            panic!("expected a CLI failure");
        };
        assert_eq!(command, "validate");
        assert!(!reason.is_empty());
    }

    #[test]
    fn an_empty_report_is_a_passing_one() {
        let report = read_validation("{}").unwrap();
        assert!(report.is_ok());
        assert!(report.items.is_empty());
        assert_eq!(report.total_duration(), 0);
    }

    #[test]
    fn severities_are_read_leniently_and_ordered_by_weight() {
        assert_eq!(Severity::parse("ERROR"), Severity::Error);
        assert_eq!(Severity::parse(" error "), Severity::Error);
        assert_eq!(Severity::parse("warn"), Severity::Warning);
        assert_eq!(Severity::parse("WARNING"), Severity::Warning);
        assert_eq!(Severity::parse("info"), Severity::Info);
        // An unknown level is a note, never an error.
        assert_eq!(Severity::parse("catastrophe"), Severity::Info);
        assert_eq!(Severity::parse(""), Severity::Info);

        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);

        for severity in [Severity::Info, Severity::Warning, Severity::Error] {
            assert_eq!(severity.glyph().chars().count(), 1);
        }
    }

    #[test]
    fn validating_one_change_narrows_the_type() {
        assert_eq!(
            validate_arguments(None, false),
            ["validate", "--all", "--json", "--no-interactive"]
        );
        assert_eq!(
            validate_arguments(Some("add-history"), true),
            [
                "validate",
                "add-history",
                "--type",
                "change",
                "--strict",
                "--json",
                "--no-interactive"
            ]
        );
    }

    #[test]
    fn archiving_never_asks_for_confirmation() {
        assert_eq!(
            archive_arguments("add-history", false),
            ["archive", "add-history", "--yes"]
        );
        assert_eq!(
            archive_arguments("add-history", true),
            ["archive", "add-history", "--yes", "--skip-specs"]
        );
    }

    #[test]
    fn archive_output_is_cleaned_and_bounded() {
        let chatty = (0..40)
            .map(|index| format!("moved item {index}   "))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = archive_lines(&Output {
            status: true,
            stdout: format!("\u{1b}[32m✔\u{1b}[0m archived\n\n   \n{chatty}"),
            stderr: "  deprecation notice\n".to_string(),
        });

        assert_eq!(lines.len(), MAX_ARCHIVE_LINES);
        assert_eq!(lines[0], "✔ archived");
        assert_eq!(lines[1], "moved item 0");
    }

    #[test]
    fn stderr_wins_when_the_cli_reports_a_failure() {
        assert_eq!(
            first_meaningful_line("\n  \u{1b}[31mboom\u{1b}[0m  \n", "ignored"),
            "boom"
        );
        assert_eq!(first_meaningful_line("   \n", "  fallback\n"), "fallback");
        assert_eq!(first_meaningful_line("", ""), "no output");
    }

    #[test]
    fn strips_escape_sequences() {
        assert_eq!(strip_ansi("\u{1b}[32m✔\u{1b}[0m done"), "✔ done");
        // A sequence the line ends in the middle of consumes the rest.
        assert_eq!(strip_ansi("done \u{1b}[3"), "done ");
    }

    #[test]
    fn runs_a_nested_workspace_from_its_repository() {
        let nested = Path::new("/repo/openspec");
        assert_eq!(working_directory(nested), Path::new("/repo"));
        assert_eq!(working_directory(Path::new("/flat")), Path::new("/flat"));
        // The marker directory at the filesystem root has no repository above.
        assert_eq!(working_directory(Path::new("/")), Path::new("/"));
    }

    #[test]
    fn an_uninstalled_cli_is_reported_as_missing_rather_than_as_io() {
        let error = run("openspectation-no-such-program", &std::env::temp_dir(), &[]).unwrap_err();
        assert!(matches!(error, Error::CliMissing), "{error:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_run_captures_both_streams_from_the_workspace_without_colour() {
        let root = std::fs::canonicalize(std::env::temp_dir()).expect("temp dir");
        let script = r#"printf 'done'; printf 'warned' >&2; printf ' %s %s' "$NO_COLOR" "$(pwd -P)"; exit 3"#;
        let output = run("/bin/sh", &root, &["-c".to_string(), script.to_string()]).unwrap();

        assert!(!output.status);
        assert_eq!(output.stderr, "warned");
        assert_eq!(output.stdout, format!("done 1 {}", root.display()));
    }

    /// A spawn can fail for reasons other than the CLI being absent, and only
    /// the absent one earns the "install openspec" hint.
    #[cfg(unix)]
    #[test]
    fn a_program_that_cannot_be_executed_is_not_reported_as_missing() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        struct Scratch(PathBuf);

        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }

        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "openspectation-unexecutable-{}-{unique}",
            std::process::id()
        )));
        std::fs::write(&scratch.0, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&scratch.0, std::fs::Permissions::from_mode(0o600)).unwrap();

        let error = run(&scratch.0.to_string_lossy(), &std::env::temp_dir(), &[]).unwrap_err();
        assert!(matches!(error, Error::Io { .. }), "{error:?}");
    }
}
