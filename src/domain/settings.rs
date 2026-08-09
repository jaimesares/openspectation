//! Preferences that belong to the user rather than to a workspace.
//!
//! A palette and an interface language say nothing about the specifications in
//! a repository, so they are not written into `openspec.yaml`, which is a file
//! the whole team commits. They live under the XDG configuration directory and
//! apply to every workspace the tool is pointed at.
//!
//! Loading never fails. A settings file that cannot be read or parsed is a
//! preference lost, not a session refused: the defaults are always usable, and
//! refusing to start over a stale colour name would be absurd.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::util;
use crate::error::Result;

/// Directory created under the configuration root.
pub const DIRECTORY: &str = "openspectation";

/// File the preferences are written to.
pub const FILE: &str = "settings.yaml";

/// Environment variable that relocates the settings file wholesale.
///
/// Exists so a test — or a throwaway invocation — can be given its own file
/// instead of writing into the caller's real configuration.
pub const PATH_ENV: &str = "OPENSPECTATION_SETTINGS";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Palette name, as accepted by `--theme`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Locale code, as accepted by `--lang`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Sidebar width in columns, absent while the sidebar sizes itself.
    ///
    /// What the user asked for, not what last fitted on screen: a terminal too
    /// narrow to honour it clamps the frame and leaves this alone.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "columns")]
    pub sidebar_width: Option<u16>,
}

/// Reads a column count, treating anything that is not one as unset.
///
/// A malformed number would otherwise fail the whole document and take the
/// palette and the language down with it, which is a lot to lose over a width.
fn columns<'de, D>(deserializer: D) -> std::result::Result<Option<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_norway::Value::deserialize(deserializer)?;
    Ok(value.as_u64().and_then(|number| u16::try_from(number).ok()))
}

impl Settings {
    /// Where the preferences are read from and written to.
    ///
    /// `None` when neither the override nor a home directory is known, which is
    /// the one case where nothing can be persisted.
    pub fn path() -> Option<PathBuf> {
        if let Ok(explicit) = std::env::var(PATH_ENV)
            && !explicit.trim().is_empty()
        {
            return Some(PathBuf::from(explicit));
        }

        let root = match std::env::var("XDG_CONFIG_HOME") {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
            _ => PathBuf::from(std::env::var("HOME").ok()?).join(".config"),
        };

        Some(root.join(DIRECTORY).join(FILE))
    }

    /// Reads the preferences, falling back to the defaults on any failure.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_norway::from_str(&raw).unwrap_or_default()
    }

    /// Writes the preferences, creating the configuration directory if needed.
    pub fn save(&self) -> Result<()> {
        let Some(path) = Self::path() else {
            return Ok(());
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|source| crate::error::Error::io(parent, source))?;
        }

        let body = serde_norway::to_string(self)
            .map_err(|source| crate::error::Error::yaml(&path, source))?;
        util::write_atomic(&path, &header(&body))
    }
}

/// Prefaces the file with what wrote it, since nothing else in the directory
/// says so.
fn header(body: &str) -> String {
    format!("# Written by openspectation. Safe to edit or delete.\n{body}")
}

/// Serialises the tests that point [`PATH_ENV`] at a file of their own.
///
/// The variable is process-wide and the test binary is threaded, so without
/// this two tests would read each other's file — and any test building an
/// [`App`](crate::tui::app::App) would otherwise read the developer's real
/// preferences and fail on whatever they happen to have chosen.
#[cfg(test)]
pub(crate) static TEST_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Points every user-level file at scratch paths for the duration of the guard.
///
/// The settings file is the one this module owns, but an
/// [`App`](crate::tui::app::App) also opens the activity record and can append
/// to it on any reload. Both are redirected together because both are reached
/// through the same constructor: a caller that remembered one and forgot the
/// other would write a test's events into the developer's own history.
#[cfg(test)]
pub(crate) fn redirect_for_test(
    name: &str,
) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
    let guard = TEST_ENV
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scratch = |kind: &str, extension: &str| {
        std::env::temp_dir().join(format!(
            "openspectation-{kind}-{}-{name}.{extension}",
            std::process::id()
        ))
    };

    let path = scratch("settings", "yaml");
    let record = scratch("history", "log");
    let cache = scratch("history-cache", "log");
    for stale in [&path, &record, &cache] {
        let _ = std::fs::remove_file(stale);
    }

    // Safe: the caller holds the `TEST_ENV` mutex, so no other thread touches
    // the environment while the variables move.
    unsafe {
        std::env::set_var(PATH_ENV, &path);
        std::env::set_var(crate::domain::history::PATH_ENV, &record);
        std::env::set_var(crate::domain::history::CACHE_ENV, &cache);
    }
    (guard, path)
}

/// The scratch activity record belonging to a [`redirect_for_test`] guard.
#[cfg(test)]
pub(crate) fn test_history_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var(crate::domain::history::PATH_ENV).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_reads_back() {
        let (_guard, path) = redirect_for_test("roundtrip");

        Settings {
            theme: Some("nord".to_string()),
            language: Some("es".to_string()),
            sidebar_width: Some(52),
        }
        .save()
        .unwrap();

        let loaded = Settings::load();
        assert_eq!(loaded.theme.as_deref(), Some("nord"));
        assert_eq!(loaded.language.as_deref(), Some("es"));
        assert_eq!(loaded.sidebar_width, Some(52));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unset_preference_is_left_out_of_the_file() {
        let (_guard, path) = redirect_for_test("partial");

        Settings {
            theme: Some("dracula".to_string()),
            language: None,
            sidebar_width: None,
        }
        .save()
        .unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("theme: dracula"));
        assert!(!raw.contains("language"));
        assert!(!raw.contains("sidebar_width"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_width_that_is_not_a_number_is_read_as_unset() {
        let (_guard, path) = redirect_for_test("width");
        std::fs::write(&path, "theme: nord\nsidebar_width: wide\n").unwrap();

        let loaded = Settings::load();
        // The rest of the file survives: the width is one preference, not the
        // whole set of them.
        assert_eq!(loaded.theme.as_deref(), Some("nord"));
        assert_eq!(loaded.sidebar_width, None);

        std::fs::write(&path, "theme: nord\n").unwrap();
        assert_eq!(Settings::load().sidebar_width, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_corrupt_file_yields_the_defaults() {
        let (_guard, path) = redirect_for_test("corrupt");
        std::fs::write(&path, "theme: [not, a, name\n").unwrap();

        assert_eq!(Settings::load(), Settings::default());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_yields_the_defaults() {
        let (_guard, path) = redirect_for_test("absent");
        assert_eq!(Settings::load(), Settings::default());
        assert!(!path.exists());
    }

    #[test]
    fn the_file_says_what_wrote_it() {
        // Nothing else in the configuration directory names the tool.
        let rendered = header("theme: nord\n");
        assert!(rendered.starts_with("# Written by openspectation."));
        assert!(rendered.ends_with("theme: nord\n"));
    }

    /// Runs `body` with the process environment set to `variables`, restoring
    /// whatever was there before. Holds [`TEST_ENV`] throughout, which is what
    /// makes mutating process-wide state safe in a threaded test binary.
    fn with_environment(variables: &[(&str, Option<&str>)], body: impl FnOnce()) {
        let _guard = TEST_ENV
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let previous: Vec<(String, Option<String>)> = variables
            .iter()
            .map(|(key, _)| ((*key).to_string(), std::env::var(key).ok()))
            .collect();

        // Safe: the guard above is held for the whole scope, so no other thread
        // reads or writes the environment while these move.
        unsafe {
            for (key, value) in variables {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));

        unsafe {
            for (key, value) in &previous {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }

        if let Err(payload) = outcome {
            std::panic::resume_unwind(payload);
        }
    }

    #[test]
    fn the_override_wins_over_the_configuration_directory() {
        with_environment(
            &[
                (PATH_ENV, Some("/scratch/custom.yaml")),
                ("XDG_CONFIG_HOME", Some("/xdg")),
            ],
            || {
                assert_eq!(
                    Settings::path(),
                    Some(PathBuf::from("/scratch/custom.yaml"))
                )
            },
        );
    }

    #[test]
    fn a_blank_override_is_ignored_rather_than_used_as_a_path() {
        with_environment(
            &[
                (PATH_ENV, Some("   ")),
                ("XDG_CONFIG_HOME", Some("/xdg")),
                ("HOME", Some("/home/someone")),
            ],
            || {
                assert_eq!(
                    Settings::path(),
                    Some(PathBuf::from("/xdg").join(DIRECTORY).join(FILE))
                );
            },
        );
    }

    #[test]
    fn without_xdg_the_path_falls_back_to_the_home_directory() {
        with_environment(
            &[
                (PATH_ENV, None),
                ("XDG_CONFIG_HOME", None),
                ("HOME", Some("/home/someone")),
            ],
            || {
                assert_eq!(
                    Settings::path(),
                    Some(
                        PathBuf::from("/home/someone")
                            .join(".config")
                            .join(DIRECTORY)
                            .join(FILE)
                    )
                );
            },
        );

        // A blank XDG value is treated as absent, not as the filesystem root.
        with_environment(
            &[
                (PATH_ENV, None),
                ("XDG_CONFIG_HOME", Some("  ")),
                ("HOME", Some("/home/someone")),
            ],
            || {
                assert!(
                    Settings::path()
                        .unwrap()
                        .starts_with("/home/someone/.config")
                );
            },
        );
    }

    #[test]
    fn with_nowhere_to_write_nothing_is_persisted_and_nothing_fails() {
        // The one case where preferences simply cannot be kept.
        with_environment(
            &[(PATH_ENV, None), ("XDG_CONFIG_HOME", None), ("HOME", None)],
            || {
                assert_eq!(Settings::path(), None);
                assert_eq!(Settings::load(), Settings::default());
                assert!(Settings::default().save().is_ok());
            },
        );
    }

    #[test]
    fn saving_creates_the_configuration_directory() {
        let unique = std::process::id();
        let directory = std::env::temp_dir().join(format!("openspectation-settings-dir-{unique}"));
        let _ = std::fs::remove_dir_all(&directory);
        let path = directory.join("nested").join(FILE);

        with_environment(&[(PATH_ENV, Some(path.to_str().unwrap()))], || {
            Settings {
                theme: Some("nord".to_string()),
                ..Settings::default()
            }
            .save()
            .unwrap();
            assert!(path.is_file());
        });

        let _ = std::fs::remove_dir_all(&directory);
    }
}
