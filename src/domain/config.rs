//! `openspec.yaml` model.
//!
//! Every field has a default so a partially written configuration still loads;
//! unknown keys are preserved-by-ignoring rather than rejected, which keeps the
//! tool forward compatible with richer OpenSpec files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Canonical file name written by `openspectation init`.
pub const CONFIG_FILE: &str = "openspec.yaml";

/// Alternative spelling accepted when reading an existing workspace.
pub const CONFIG_FILE_ALT: &str = "openspec.yml";

/// Configuration file name used by the upstream OpenSpec CLI, which keeps the
/// whole layout inside a dedicated directory and therefore does not need to
/// qualify the name.
pub const CONFIG_FILE_NESTED: [&str; 2] = ["config.yaml", "config.yml"];

/// Directory the upstream OpenSpec CLI nests its layout in.
pub const WORKSPACE_DIR: &str = "openspec";

/// Default boilerplate design document.
pub const DESIGN_FILE: &str = "design.md";

/// Document names that play the role of `design.md` in a nested layout, tried
/// in order when the configured one is absent.
pub const DESIGN_CANDIDATES: [&str; 3] = [DESIGN_FILE, "project.md", "AGENTS.md"];

/// Default directory holding the specifications.
pub const SPECS_DIR: &str = "specs";

/// Default directory holding the change proposals.
pub const CHANGES_DIR: &str = "changes";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// Schema version of the configuration file itself.
    pub version: u32,
    /// Free-form prose describing the project, written by the upstream
    /// OpenSpec CLI in place of a design document.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub context: String,
    pub project: ProjectConfig,
    pub layout: LayoutConfig,
    pub conventions: Conventions,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            version: 1,
            context: String::new(),
            project: ProjectConfig::default(),
            layout: LayoutConfig::default(),
            conventions: Conventions::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub name: String,
    pub description: String,
    /// Preferred language for the documents inside the workspace.
    pub language: String,
    pub owners: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutConfig {
    pub specs: String,
    pub changes: String,
    pub design: String,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            specs: SPECS_DIR.to_string(),
            changes: CHANGES_DIR.to_string(),
            design: DESIGN_FILE.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Conventions {
    /// File extensions treated as specification documents.
    pub extensions: Vec<String>,
    /// Allowed values of the `status` frontmatter field.
    pub statuses: Vec<String>,
    /// Whether requirement keywords must follow RFC 2119 casing.
    pub rfc2119: bool,
}

impl Default for Conventions {
    fn default() -> Self {
        Self {
            extensions: vec![
                "md".to_string(),
                "markdown".to_string(),
                "yaml".to_string(),
                "yml".to_string(),
            ],
            statuses: vec![
                "draft".to_string(),
                "review".to_string(),
                "approved".to_string(),
                "implemented".to_string(),
                "deprecated".to_string(),
            ],
            rfc2119: true,
        }
    }
}

impl WorkspaceConfig {
    /// Builds the configuration written by `init`.
    pub fn scaffold(name: &str, description: &str, language: &str) -> Self {
        Self {
            project: ProjectConfig {
                name: name.to_string(),
                description: description.to_string(),
                language: language.to_string(),
                owners: Vec::new(),
            },
            ..Self::default()
        }
    }

    /// Reads `openspec.yaml` (or `openspec.yml`) from a workspace root.
    ///
    /// Returns `Ok(None)` when neither file exists.
    pub fn load(root: &Path) -> Result<Option<Self>> {
        let Some(path) = Self::locate(root) else {
            return Ok(None);
        };

        let raw = std::fs::read_to_string(&path).map_err(|source| Error::io(&path, source))?;
        if raw.trim().is_empty() {
            return Ok(Some(Self::default()));
        }

        serde_norway::from_str(&raw)
            .map(Some)
            .map_err(|source| Error::yaml(&path, source))
    }

    /// Serializes the configuration with a short explanatory header.
    pub fn to_yaml(&self) -> Result<String> {
        let body =
            serde_norway::to_string(self).map_err(|source| Error::yaml(CONFIG_FILE, source))?;
        Ok(format!(
            "# OpenSpec workspace configuration\n\
             # Generated by openspectation. Edit freely: unknown keys are ignored.\n\
             {body}"
        ))
    }

    /// Path of the configuration file present in `root`, if any.
    pub fn locate(root: &Path) -> Option<PathBuf> {
        let qualified = [CONFIG_FILE, CONFIG_FILE_ALT]
            .into_iter()
            .map(|name| root.join(name))
            .find(|candidate| candidate.is_file());
        if qualified.is_some() {
            return qualified;
        }

        // `config.yaml` is far too generic a name to treat as an OpenSpec
        // marker on its own, so it only counts next to an actual layout.
        if !root.join(SPECS_DIR).is_dir() && !root.join(CHANGES_DIR).is_dir() {
            return None;
        }
        CONFIG_FILE_NESTED
            .into_iter()
            .map(|name| root.join(name))
            .find(|candidate| candidate.is_file())
    }

    /// Absolute path of the specs directory for this workspace.
    pub fn specs_dir(&self, root: &Path) -> PathBuf {
        root.join(&self.layout.specs)
    }

    /// Absolute path of the changes directory for this workspace.
    pub fn changes_dir(&self, root: &Path) -> PathBuf {
        root.join(&self.layout.changes)
    }

    /// Absolute path of the design document for this workspace.
    ///
    /// An explicitly configured path is always honoured; the default name only
    /// gives way to the alternatives when no `design.md` exists on disk.
    pub fn design_file(&self, root: &Path) -> PathBuf {
        let configured = root.join(&self.layout.design);
        if self.layout.design != DESIGN_FILE || configured.is_file() {
            return configured;
        }
        DESIGN_CANDIDATES
            .into_iter()
            .map(|name| root.join(name))
            .find(|candidate| candidate.is_file())
            .unwrap_or(configured)
    }

    /// Whether a file extension is considered a specification document.
    pub fn is_spec_extension(&self, extension: &str) -> bool {
        self.conventions
            .extensions
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(extension))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "openspectation-config-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, contents: &str) {
            std::fs::write(self.0.join(name), contents).unwrap();
        }

        fn dir(&self, name: &str) {
            std::fs::create_dir_all(self.0.join(name)).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_scaffolded_configuration_records_the_project_and_keeps_every_default() {
        let config = WorkspaceConfig::scaffold("Ledger", "Invoices", "es");

        assert_eq!(config.project.name, "Ledger");
        assert_eq!(config.project.description, "Invoices");
        assert_eq!(config.project.language, "es");
        assert!(config.project.owners.is_empty());
        assert_eq!(config.version, 1);
        assert_eq!(config.layout.specs, SPECS_DIR);
        assert_eq!(config.layout.changes, CHANGES_DIR);
        assert_eq!(config.layout.design, DESIGN_FILE);
        assert!(config.conventions.rfc2119);
    }

    #[test]
    fn a_workspace_without_a_configuration_file_loads_as_nothing() {
        let temp = TempDir::new();
        assert!(WorkspaceConfig::load(&temp.0).unwrap().is_none());
        assert!(WorkspaceConfig::locate(&temp.0).is_none());
    }

    #[test]
    fn an_empty_configuration_file_loads_as_the_defaults() {
        // An `openspec.yaml` the user has emptied still marks a workspace.
        let temp = TempDir::new();
        temp.write(CONFIG_FILE, "\n  \n");

        let config = WorkspaceConfig::load(&temp.0).unwrap().unwrap();
        assert_eq!(config.layout.specs, SPECS_DIR);
    }

    #[test]
    fn a_partial_configuration_fills_the_rest_from_the_defaults() {
        let temp = TempDir::new();
        temp.write(CONFIG_FILE, "project:\n  name: Ledger\n");

        let config = WorkspaceConfig::load(&temp.0).unwrap().unwrap();
        assert_eq!(config.project.name, "Ledger");
        assert_eq!(config.layout.changes, CHANGES_DIR);
        assert_eq!(config.conventions.extensions.len(), 4);
    }

    #[test]
    fn unknown_keys_are_ignored_rather_than_rejected() {
        // Forward compatibility with richer upstream OpenSpec files.
        let temp = TempDir::new();
        temp.write(
            CONFIG_FILE,
            "project:\n  name: Ledger\nrules:\n  - be brief\n",
        );

        assert!(WorkspaceConfig::load(&temp.0).is_ok());
    }

    #[test]
    fn malformed_yaml_names_the_file_it_came_from() {
        let temp = TempDir::new();
        temp.write(CONFIG_FILE, "project:\n  name: [unterminated\n");

        match WorkspaceConfig::load(&temp.0) {
            Err(Error::Yaml { path, .. }) => assert_eq!(path, temp.0.join(CONFIG_FILE)),
            other => panic!("expected Yaml, got {other:?}"),
        }
    }

    #[test]
    fn the_alternative_extension_is_accepted_when_reading() {
        let temp = TempDir::new();
        temp.write(CONFIG_FILE_ALT, "project:\n  name: Ledger\n");

        assert_eq!(
            WorkspaceConfig::locate(&temp.0),
            Some(temp.0.join(CONFIG_FILE_ALT))
        );
        assert_eq!(
            WorkspaceConfig::load(&temp.0)
                .unwrap()
                .unwrap()
                .project
                .name,
            "Ledger"
        );
    }

    #[test]
    fn a_generic_config_file_counts_only_next_to_an_actual_layout() {
        // `config.yaml` alone is far too common a name to claim a workspace.
        let temp = TempDir::new();
        temp.write(CONFIG_FILE_NESTED[0], "context: Something\n");
        assert!(WorkspaceConfig::locate(&temp.0).is_none());

        temp.dir(SPECS_DIR);
        assert_eq!(
            WorkspaceConfig::locate(&temp.0),
            Some(temp.0.join(CONFIG_FILE_NESTED[0]))
        );
    }

    #[test]
    fn the_qualified_name_wins_over_the_generic_one() {
        let temp = TempDir::new();
        temp.dir(SPECS_DIR);
        temp.write(CONFIG_FILE_NESTED[0], "context: Nested\n");
        temp.write(CONFIG_FILE, "project:\n  name: Qualified\n");

        assert_eq!(
            WorkspaceConfig::locate(&temp.0),
            Some(temp.0.join(CONFIG_FILE))
        );
    }

    #[test]
    fn the_layout_directories_hang_off_the_root_they_are_asked_about() {
        let config = WorkspaceConfig::default();
        let root = Path::new("/w");
        assert_eq!(config.specs_dir(root), PathBuf::from("/w/specs"));
        assert_eq!(config.changes_dir(root), PathBuf::from("/w/changes"));
    }

    #[test]
    fn a_configured_design_path_is_honoured_even_when_the_file_is_absent() {
        let temp = TempDir::new();
        temp.write("project.md", "# Project\n");

        let mut config = WorkspaceConfig::default();
        config.layout.design = "architecture.md".to_string();

        assert_eq!(config.design_file(&temp.0), temp.0.join("architecture.md"));
    }

    #[test]
    fn the_default_design_name_gives_way_to_the_alternatives_on_disk() {
        let temp = TempDir::new();
        let config = WorkspaceConfig::default();

        // Nothing on disk: the configured default is still what is reported.
        assert_eq!(config.design_file(&temp.0), temp.0.join(DESIGN_FILE));

        temp.write("AGENTS.md", "# Agents\n");
        assert_eq!(config.design_file(&temp.0), temp.0.join("AGENTS.md"));

        // The candidates are tried in order, so project.md outranks AGENTS.md.
        temp.write("project.md", "# Project\n");
        assert_eq!(config.design_file(&temp.0), temp.0.join("project.md"));

        temp.write(DESIGN_FILE, "# Design\n");
        assert_eq!(config.design_file(&temp.0), temp.0.join(DESIGN_FILE));
    }

    #[test]
    fn spec_extensions_are_matched_without_regard_to_case() {
        let config = WorkspaceConfig::default();
        for extension in ["md", "MD", "Markdown", "yaml", "YML"] {
            assert!(config.is_spec_extension(extension), "rejected {extension}");
        }
        for extension in ["txt", "rs", ""] {
            assert!(!config.is_spec_extension(extension), "accepted {extension}");
        }
    }

    #[test]
    fn the_serialized_form_carries_a_header_and_reads_back() {
        let config = WorkspaceConfig::scaffold("Ledger", "Invoices", "en");
        let yaml = config.to_yaml().unwrap();

        assert!(yaml.starts_with("# OpenSpec workspace configuration\n"));
        let parsed: WorkspaceConfig = serde_norway::from_str(&yaml).unwrap();
        assert_eq!(parsed.project.name, "Ledger");
        assert_eq!(parsed.layout.design, DESIGN_FILE);
        // An empty context is left out rather than written as a blank key.
        assert!(!yaml.contains("context:"));
    }
}
