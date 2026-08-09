//! Structured error type shared by every subcommand.
//!
//! [`Error`](enum@Error) carries data, never prose: the `thiserror` `Display` output is a
//! stable English string meant for logs and `Debug` output, while
//! [`Error::render`] produces the colored, translated message shown to users.

use std::path::{Path, PathBuf};

use rust_i18n::t;
use thiserror::Error;

use crate::paint::Painter;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("not a directory: {0}")]
    NotADirectory(PathBuf),

    #[error("not an OpenSpec workspace: {0}")]
    NotAWorkspace(PathBuf),

    #[error("workspace already initialized: {0}")]
    AlreadyInitialized(PathBuf),

    #[error("invalid project name: {0}")]
    InvalidProjectName(String),

    #[error("no change proposal named {0}")]
    UnknownChange(String),

    #[error("change already archived: {0}")]
    AlreadyArchived(String),

    #[error("validation failed for {0} items")]
    ValidationFailed(usize),

    #[error("confirmation required but stdin is not a terminal")]
    ConfirmationRequired,

    #[error("{0}:{1} is no longer a task")]
    TaskMoved(PathBuf, usize),

    #[error("{0} is too large to edit in place")]
    TooLargeToEdit(PathBuf),

    #[error("the openspec CLI is not installed")]
    CliMissing,

    #[error("openspec {command} failed: {reason}")]
    CliFailed { command: String, reason: String },

    #[error("filesystem error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid YAML in {path}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_norway::Error,
    },

    #[error("terminal error: {0}")]
    Terminal(#[from] std::io::Error),
}

impl Error {
    /// Wraps an [`std::io::Error`] together with the path that produced it.
    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// Wraps a [`serde_norway::Error`] together with the offending file.
    pub fn yaml(path: impl AsRef<Path>, source: serde_norway::Error) -> Self {
        Self::Yaml {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// Localized one line description of the failure.
    pub fn message(&self) -> String {
        match self {
            Self::PathNotFound(path) => {
                t!("error.path_not_found", path = display(path)).to_string()
            }
            Self::NotADirectory(path) => {
                t!("error.not_a_directory", path = display(path)).to_string()
            }
            Self::NotAWorkspace(path) => {
                t!("error.not_a_workspace", path = display(path)).to_string()
            }
            Self::AlreadyInitialized(path) => {
                t!("error.already_initialized", path = display(path)).to_string()
            }
            Self::InvalidProjectName(name) => t!("error.invalid_name", name = name).to_string(),
            Self::UnknownChange(name) => t!("error.unknown_change", name = name).to_string(),
            Self::AlreadyArchived(name) => t!("error.already_archived", name = name).to_string(),
            Self::ValidationFailed(count) => {
                t!("error.validation_failed", count = count).to_string()
            }
            Self::ConfirmationRequired => t!("error.confirmation_required").to_string(),
            Self::TaskMoved(path, line) => {
                t!("error.task_moved", path = display(path), line = line).to_string()
            }
            Self::TooLargeToEdit(path) => {
                t!("error.too_large_to_edit", path = display(path)).to_string()
            }
            Self::CliMissing => t!("error.cli_missing").to_string(),
            Self::CliFailed { command, reason } => {
                t!("error.cli_failed", command = command, reason = reason).to_string()
            }
            Self::Io { path, source } => {
                t!("error.io", path = display(path), reason = source).to_string()
            }
            Self::Yaml { path, source } => {
                t!("error.yaml", path = display(path), reason = source).to_string()
            }
            Self::Terminal(source) => t!("error.terminal", reason = source).to_string(),
        }
    }

    /// Localized remediation advice, when one exists for this failure.
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::PathNotFound(_) => Some(t!("error.path_not_found_hint").to_string()),
            Self::NotADirectory(_) => Some(t!("error.not_a_directory_hint").to_string()),
            Self::NotAWorkspace(path) => {
                Some(t!("error.not_a_workspace_hint", path = display(path)).to_string())
            }
            Self::AlreadyInitialized(_) => Some(t!("error.already_initialized_hint").to_string()),
            Self::InvalidProjectName(_) => Some(t!("error.invalid_name_hint").to_string()),
            Self::UnknownChange(_) => Some(t!("error.unknown_change_hint").to_string()),
            Self::TaskMoved(..) => Some(t!("error.task_moved_hint").to_string()),
            Self::TooLargeToEdit(_) => Some(t!("error.too_large_to_edit_hint").to_string()),
            Self::CliMissing => Some(t!("error.cli_missing_hint").to_string()),
            Self::ConfirmationRequired => Some(t!("error.confirmation_required_hint").to_string()),
            Self::AlreadyArchived(_)
            | Self::ValidationFailed(_)
            | Self::CliFailed { .. }
            | Self::Io { .. }
            | Self::Yaml { .. }
            | Self::Terminal(_) => None,
        }
    }

    /// Full colored block written to stderr before exiting.
    pub fn render(&self, painter: Painter) -> String {
        let mut out = format!(
            "{} {}",
            painter.danger_bold(format!("{}:", t!("error.label"))),
            painter.plain(self.message())
        );

        if let Some(hint) = self.hint() {
            out.push('\n');
            out.push_str(&format!(
                "{} {}",
                painter.info_bold(format!("{}:", t!("error.hint_label"))),
                painter.dim(&hint)
            ));
        }

        out
    }

    /// Process exit code associated with the failure.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::PathNotFound(_)
            | Self::NotADirectory(_)
            | Self::NotAWorkspace(_)
            | Self::AlreadyInitialized(_)
            | Self::InvalidProjectName(_)
            | Self::UnknownChange(_)
            | Self::AlreadyArchived(_)
            | Self::ConfirmationRequired
            | Self::CliMissing => 2,
            Self::Io { .. }
            | Self::Yaml { .. }
            | Self::Terminal(_)
            | Self::TaskMoved(..)
            | Self::TooLargeToEdit(_)
            | Self::ValidationFailed(_)
            | Self::CliFailed { .. } => 1,
        }
    }
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn io_error() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied")
    }

    fn yaml_error() -> serde_norway::Error {
        serde_norway::from_str::<u8>("[1, 2]").unwrap_err()
    }

    /// One sample per variant, with the two properties the CLI contract fixes:
    /// the process exit code and whether the failure offers remediation advice.
    /// Adding a variant without extending this list leaves it unexercised.
    fn every_variant() -> Vec<(Error, u8, bool)> {
        vec![
            (Error::PathNotFound("/nope".into()), 2, true),
            (Error::NotADirectory("/nope".into()), 2, true),
            (Error::NotAWorkspace("/nope".into()), 2, true),
            (Error::AlreadyInitialized("/nope".into()), 2, true),
            (Error::InvalidProjectName("bad/name".into()), 2, true),
            (Error::UnknownChange("add-oauth".into()), 2, true),
            (Error::AlreadyArchived("add-oauth".into()), 2, false),
            (Error::ValidationFailed(3), 1, false),
            (Error::ConfirmationRequired, 2, true),
            (Error::TaskMoved("/tasks.md".into(), 12), 1, true),
            (Error::TooLargeToEdit("/huge.md".into()), 1, true),
            (Error::CliMissing, 2, true),
            (
                Error::CliFailed {
                    command: "validate".into(),
                    reason: "exit 1".into(),
                },
                1,
                false,
            ),
            (Error::io("/locked", io_error()), 1, false),
            (Error::yaml("/config.yaml", yaml_error()), 1, false),
            (Error::Terminal(io_error()), 1, false),
        ]
    }

    #[test]
    fn every_variant_maps_to_its_documented_exit_code() {
        for (error, expected, _) in every_variant() {
            assert_eq!(
                error.exit_code(),
                expected,
                "wrong exit code for {}",
                Error::to_string(&error)
            );
        }
    }

    #[test]
    fn only_recoverable_failures_carry_a_hint() {
        for (error, _, expects_hint) in every_variant() {
            assert_eq!(
                error.hint().is_some(),
                expects_hint,
                "wrong hint presence for {}",
                Error::to_string(&error)
            );
        }
    }

    #[test]
    fn every_variant_produces_a_non_empty_translated_message() {
        // Asserts translation happened at all rather than the wording, which
        // depends on the ambient locale.
        for (error, _, _) in every_variant() {
            let message = error.message();
            assert!(
                !message.trim().is_empty(),
                "empty message for {}",
                Error::to_string(&error)
            );
            if let Some(hint) = error.hint() {
                assert!(
                    !hint.trim().is_empty(),
                    "empty hint for {}",
                    Error::to_string(&error)
                );
            }
        }
    }

    #[test]
    fn the_message_interpolates_the_data_the_variant_carries() {
        assert!(
            Error::PathNotFound("/tmp/missing".into())
                .message()
                .contains("/tmp/missing")
        );
        assert!(
            Error::InvalidProjectName("bad/name".into())
                .message()
                .contains("bad/name")
        );
        assert!(
            Error::UnknownChange("add-oauth".into())
                .message()
                .contains("add-oauth")
        );
        assert!(Error::ValidationFailed(7).message().contains('7'));
        assert!(
            Error::CliFailed {
                command: "archive".into(),
                reason: "boom".into(),
            }
            .message()
            .contains("archive")
        );

        let moved = Error::TaskMoved("/w/tasks.md".into(), 42).message();
        assert!(moved.contains("/w/tasks.md") && moved.contains("42"));
    }

    #[test]
    fn the_hint_interpolates_the_path_when_the_key_takes_one() {
        assert!(
            Error::NotAWorkspace("/tmp/plain".into())
                .hint()
                .unwrap()
                .contains("/tmp/plain")
        );
    }

    #[test]
    fn display_stays_stable_english_for_logs() {
        // `Display` feeds logs and `Debug` output, so it must not follow the
        // interface language.
        assert_eq!(
            Error::PathNotFound("/tmp/missing".into()).to_string(),
            "path not found: /tmp/missing"
        );
        assert_eq!(
            Error::CliMissing.to_string(),
            "the openspec CLI is not installed"
        );
        assert_eq!(
            Error::ValidationFailed(3).to_string(),
            "validation failed for 3 items"
        );
        assert_eq!(
            Error::CliFailed {
                command: "validate".into(),
                reason: "exit 1".into(),
            }
            .to_string(),
            "openspec validate failed: exit 1"
        );
    }

    #[test]
    fn the_constructors_attach_the_offending_path_to_the_source() {
        match Error::io("/locked", io_error()) {
            Error::Io { path, source } => {
                assert_eq!(path, PathBuf::from("/locked"));
                assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
            }
            other => panic!("expected Io, got {other:?}"),
        }

        match Error::yaml("/config.yaml", yaml_error()) {
            Error::Yaml { path, .. } => assert_eq!(path, PathBuf::from("/config.yaml")),
            other => panic!("expected Yaml, got {other:?}"),
        }
    }

    #[test]
    fn an_io_error_converts_into_the_terminal_variant() {
        let error: Error = io_error().into();
        assert!(matches!(error, Error::Terminal(_)));
        assert_eq!(error.exit_code(), 1);
    }

    #[test]
    fn render_stacks_the_hint_under_the_message_only_when_one_exists() {
        let painter = Painter::detect(true);

        let with_hint = Error::PathNotFound("/tmp/missing".into());
        let block = with_hint.render(painter);
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(&with_hint.message()));
        assert!(lines[1].contains(&with_hint.hint().unwrap()));

        let without_hint = Error::ValidationFailed(2);
        let block = without_hint.render(painter);
        assert_eq!(block.lines().count(), 1);
        assert!(block.contains(&without_hint.message()));
    }
}
