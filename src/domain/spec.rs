//! Specification documents: frontmatter, derived metrics and lifecycle status.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rust_i18n::t;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Documents larger than this are truncated before being kept in memory.
const MAX_INLINE_BYTES: u64 = 1024 * 1024;

/// File stems naming a role rather than a subject. A document called
/// `spec.md` is described by the directory holding it, not by its own name.
const GENERIC_STEMS: [&str; 6] = ["spec", "readme", "index", "proposal", "tasks", "design"];

/// RFC 2119 keywords counted as requirement markers.
const REQUIREMENT_KEYWORDS: [&str; 6] = [
    "MUST",
    "SHALL",
    "SHOULD",
    "REQUIRED",
    "SHALL NOT",
    "MUST NOT",
];

/// Lifecycle stage of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpecStatus {
    Draft,
    Review,
    Approved,
    Implemented,
    Deprecated,
    Unknown,
}

impl SpecStatus {
    /// Every status in presentation order.
    pub const ALL: [Self; 6] = [
        Self::Draft,
        Self::Review,
        Self::Approved,
        Self::Implemented,
        Self::Deprecated,
        Self::Unknown,
    ];

    /// Parses the many spellings found in real frontmatter.
    pub fn parse(raw: &str) -> Self {
        let normalized = raw.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        match normalized.as_str() {
            "draft" | "wip" | "proposal" | "proposed" => Self::Draft,
            "review" | "in-review" | "reviewing" | "pending-review" => Self::Review,
            "approved" | "accepted" | "ratified" | "stable" => Self::Approved,
            "implemented" | "done" | "shipped" | "complete" | "completed" => Self::Implemented,
            "deprecated" | "obsolete" | "archived" | "rejected" | "superseded" => Self::Deprecated,
            _ => Self::Unknown,
        }
    }

    /// Stable machine readable identifier.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Review => "review",
            Self::Approved => "approved",
            Self::Implemented => "implemented",
            Self::Deprecated => "deprecated",
            Self::Unknown => "unknown",
        }
    }

    /// Localized label shown to users.
    pub fn label(self) -> String {
        match self {
            Self::Draft => t!("status.draft"),
            Self::Review => t!("status.review"),
            Self::Approved => t!("status.approved"),
            Self::Implemented => t!("status.implemented"),
            Self::Deprecated => t!("status.deprecated"),
            Self::Unknown => t!("status.unknown"),
        }
        .to_string()
    }

    /// Single glyph badge used in dense listings.
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Draft => "◌",
            Self::Review => "◐",
            Self::Approved => "●",
            Self::Implemented => "✔",
            Self::Deprecated => "✖",
            Self::Unknown => "·",
        }
    }
}

/// Storage format of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Markdown,
    Yaml,
}

impl DocKind {
    pub fn from_extension(extension: &str) -> Self {
        match extension.to_ascii_lowercase().as_str() {
            "yaml" | "yml" => Self::Yaml,
            _ => Self::Markdown,
        }
    }
}

/// Frontmatter block understood by openspectation.
///
/// Unknown keys are collected into `extra` so they can still be displayed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Frontmatter {
    pub title: Option<String>,
    pub status: Option<String>,
    pub version: Option<String>,
    pub owner: Option<String>,
    #[serde(alias = "tag")]
    pub tags: Vec<String>,
    pub updated: Option<String>,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_norway::Value>,
}

/// A single specification document with its derived metrics.
#[derive(Debug, Clone)]
pub struct SpecDocument {
    pub path: PathBuf,
    /// Path relative to the directory that was scanned.
    pub relative: PathBuf,
    /// File name including extension.
    pub file_name: String,
    /// Best effort human readable title.
    pub title: String,
    pub status: SpecStatus,
    pub kind: DocKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub frontmatter: Frontmatter,
    /// Document body with the frontmatter block removed.
    pub body: String,
    /// Lines consumed by the frontmatter, so a body line maps back to a file
    /// line. Anything addressing the file on disk must add this offset.
    pub frontmatter_lines: usize,
    pub lines: usize,
    pub words: usize,
    pub headings: usize,
    pub requirements: usize,
    pub tasks_total: usize,
    pub tasks_done: usize,
    /// Set when the file exceeded `MAX_INLINE_BYTES` and was cut short.
    pub truncated: bool,
}

impl SpecDocument {
    /// Reads and analyses a single document.
    pub fn load(path: &Path, base: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path).map_err(|source| Error::io(path, source))?;
        let size = metadata.len();
        let modified = metadata.modified().ok();

        let raw = read_capped(path, size)?;
        let truncated = size > MAX_INLINE_BYTES;

        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        let kind = DocKind::from_extension(&extension);

        let (frontmatter, body) = match kind {
            DocKind::Markdown => split_frontmatter(&raw, path)?,
            // A standalone YAML spec is entirely metadata.
            DocKind::Yaml => (parse_frontmatter(&raw, path)?, raw.clone()),
        };

        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(&file_name)
            .to_string();

        let title = frontmatter
            .title
            .clone()
            .or_else(|| first_heading(&body))
            .unwrap_or_else(|| fallback_title(path, &stem));

        let status = frontmatter
            .status
            .as_deref()
            .map_or(SpecStatus::Unknown, SpecStatus::parse);

        let metrics = Metrics::from_body(&body);
        let relative = path.strip_prefix(base).unwrap_or(path).to_path_buf();
        // The body is a whole-line suffix of the raw file, so the difference is
        // exactly the frontmatter block plus the blank lines that followed it.
        let frontmatter_lines = raw.lines().count().saturating_sub(body.lines().count());

        Ok(Self {
            path: path.to_path_buf(),
            relative,
            file_name,
            title,
            status,
            kind,
            size,
            modified,
            frontmatter,
            body,
            frontmatter_lines,
            lines: metrics.lines,
            words: metrics.words,
            headings: metrics.headings,
            requirements: metrics.requirements,
            tasks_total: metrics.tasks_total,
            tasks_done: metrics.tasks_done,
            truncated,
        })
    }

    /// Builds a document from prose that has no file of its own.
    ///
    /// Used for the `context` block of a nested workspace, which stands in for
    /// the design document the upstream layout does not have.
    pub fn from_memory(
        path: &Path,
        base: &Path,
        title: String,
        body: String,
        modified: Option<SystemTime>,
    ) -> Self {
        let metrics = Metrics::from_body(&body);
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();

        Self {
            relative: path.strip_prefix(base).unwrap_or(path).to_path_buf(),
            path: path.to_path_buf(),
            file_name,
            title,
            status: SpecStatus::Unknown,
            kind: DocKind::Markdown,
            size: body.len() as u64,
            modified,
            frontmatter: Frontmatter::default(),
            body,
            frontmatter_lines: 0,
            lines: metrics.lines,
            words: metrics.words,
            headings: metrics.headings,
            requirements: metrics.requirements,
            tasks_total: metrics.tasks_total,
            tasks_done: metrics.tasks_done,
            truncated: false,
        }
    }

    /// Relative path rendered with forward slashes for stable display.
    pub fn display_path(&self) -> String {
        self.relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Parent directory of the document relative to the scan root.
    pub fn group(&self) -> Option<String> {
        let parent = self.relative.parent()?;
        if parent.as_os_str().is_empty() {
            return None;
        }
        Some(
            parent
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    /// Case insensitive match used by the TUI filter.
    pub fn matches(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_lowercase();
        self.title.to_lowercase().contains(&needle)
            || self.display_path().to_lowercase().contains(&needle)
            || self.path.to_string_lossy().to_lowercase().contains(&needle)
            || self.status.key().contains(needle.as_str())
            || self
                .frontmatter
                .tags
                .iter()
                .any(|tag| tag.to_lowercase().contains(&needle))
    }
}

struct Metrics {
    lines: usize,
    words: usize,
    headings: usize,
    requirements: usize,
    tasks_total: usize,
    tasks_done: usize,
}

impl Metrics {
    fn from_body(body: &str) -> Self {
        let mut metrics = Self {
            lines: 0,
            words: 0,
            headings: 0,
            requirements: 0,
            tasks_total: 0,
            tasks_done: 0,
        };
        let mut in_code_fence = false;

        for line in body.lines() {
            metrics.lines += 1;
            let trimmed = line.trim_start();

            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_code_fence = !in_code_fence;
                continue;
            }
            if in_code_fence {
                continue;
            }

            metrics.words += line.split_whitespace().count();

            if trimmed.starts_with('#') {
                metrics.headings += 1;
            }

            if let Some(state) = checkbox_state(trimmed) {
                metrics.tasks_total += 1;
                if state {
                    metrics.tasks_done += 1;
                }
            }

            if contains_requirement_keyword(line) {
                metrics.requirements += 1;
            }
        }

        metrics
    }
}

/// Detects `- [ ]` / `- [x]` list items, returning whether they are checked.
pub fn checkbox_state(trimmed: &str) -> Option<bool> {
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;
    let rest = rest.trim_start();

    if let Some(after) = rest.strip_prefix("[ ]")
        && (after.is_empty() || after.starts_with(char::is_whitespace))
    {
        return Some(false);
    }
    for marker in ["[x]", "[X]", "[✓]"] {
        if let Some(after) = rest.strip_prefix(marker)
            && (after.is_empty() || after.starts_with(char::is_whitespace))
        {
            return Some(true);
        }
    }
    None
}

fn contains_requirement_keyword(line: &str) -> bool {
    REQUIREMENT_KEYWORDS.iter().any(|keyword| {
        line.match_indices(keyword).any(|(index, matched)| {
            let before_ok = index == 0
                || line[..index]
                    .chars()
                    .next_back()
                    .is_none_or(|character| !character.is_alphanumeric());
            let after_index = index + matched.len();
            let after_ok = line[after_index..]
                .chars()
                .next()
                .is_none_or(|character| !character.is_alphanumeric());
            before_ok && after_ok
        })
    })
}

/// Splits a `---` delimited YAML frontmatter block from the Markdown body.
pub fn split_frontmatter(raw: &str, path: &Path) -> Result<(Frontmatter, String)> {
    let normalized = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let trimmed = normalized.trim_start_matches(['\n', '\r']);

    let Some(rest) = trimmed.strip_prefix("---") else {
        return Ok((Frontmatter::default(), normalized.to_string()));
    };
    // The opening fence must be alone on its line.
    let Some(rest) = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
    else {
        return Ok((Frontmatter::default(), normalized.to_string()));
    };

    let Some(end) = find_closing_fence(rest) else {
        return Ok((Frontmatter::default(), normalized.to_string()));
    };

    let (front, remainder) = rest.split_at(end.start);
    let body = remainder[end.length..]
        .trim_start_matches(['\n', '\r'])
        .to_string();

    Ok((parse_frontmatter(front, path)?, body))
}

struct Fence {
    start: usize,
    length: usize,
}

fn find_closing_fence(input: &str) -> Option<Fence> {
    let mut offset = 0usize;
    for line in input.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        if content == "---" || content == "..." {
            return Some(Fence {
                start: offset,
                length: line.len(),
            });
        }
        offset += line.len();
    }
    None
}

fn parse_frontmatter(raw: &str, path: &Path) -> Result<Frontmatter> {
    if raw.trim().is_empty() {
        return Ok(Frontmatter::default());
    }
    // A spec file may legitimately contain YAML that is not a mapping; such a
    // document simply has no metadata rather than being an error.
    match serde_norway::from_str::<Frontmatter>(raw) {
        Ok(frontmatter) => Ok(frontmatter),
        Err(error) if raw.trim_start().starts_with(['-', '[']) => {
            let _ = error;
            Ok(Frontmatter::default())
        }
        Err(error) => Err(Error::yaml(path, error)),
    }
}

/// Title for a document that carries neither frontmatter nor a heading.
fn fallback_title(path: &Path, stem: &str) -> String {
    if GENERIC_STEMS.contains(&stem.to_ascii_lowercase().as_str())
        && let Some(parent) = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
    {
        return super::util::humanize(parent);
    }
    super::util::humanize(stem)
}

fn first_heading(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|line| line.starts_with('#'))
        .map(|line| line.trim_start_matches('#').trim().to_string())
        .filter(|line| !line.is_empty())
}

fn read_capped(path: &Path, size: u64) -> Result<String> {
    use std::io::Read;

    if size <= MAX_INLINE_BYTES {
        return std::fs::read_to_string(path)
            .or_else(|_| {
                std::fs::read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            })
            .map_err(|source| Error::io(path, source));
    }

    let file = std::fs::File::open(path).map_err(|source| Error::io(path, source))?;
    let mut buffer = Vec::with_capacity(MAX_INLINE_BYTES as usize);
    file.take(MAX_INLINE_BYTES)
        .read_to_end(&mut buffer)
        .map_err(|source| Error::io(path, source))?;

    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_frontmatter_from_body() {
        let raw = "---\ntitle: Auth\nstatus: approved\n---\n# Auth\n\nBody.\n";
        let (frontmatter, body) = split_frontmatter(raw, Path::new("spec.md")).unwrap();
        assert_eq!(frontmatter.title.as_deref(), Some("Auth"));
        assert_eq!(frontmatter.status.as_deref(), Some("approved"));
        assert!(body.starts_with("# Auth"));
    }

    #[test]
    fn keeps_body_when_there_is_no_frontmatter() {
        let raw = "# Auth\n\nBody.\n";
        let (frontmatter, body) = split_frontmatter(raw, Path::new("spec.md")).unwrap();
        assert!(frontmatter.title.is_none());
        assert_eq!(body, raw);
    }

    #[test]
    fn recognises_task_checkboxes() {
        assert_eq!(checkbox_state("- [ ] pending"), Some(false));
        assert_eq!(checkbox_state("- [x] done"), Some(true));
        assert_eq!(checkbox_state("- plain item"), None);
    }

    #[test]
    fn counts_only_standalone_rfc2119_keywords() {
        assert!(contains_requirement_keyword("The system MUST log in."));
        assert!(!contains_requirement_keyword("MUSTARD is not a keyword."));
    }

    #[test]
    fn parses_status_aliases() {
        assert_eq!(SpecStatus::parse("In Review"), SpecStatus::Review);
        assert_eq!(SpecStatus::parse("shipped"), SpecStatus::Implemented);
        assert_eq!(SpecStatus::parse("nonsense"), SpecStatus::Unknown);
    }

    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// A directory removed when the guard drops, so a failing assertion cannot
    /// leave a workspace behind in the temp directory.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "openspectation-spec-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn status_spellings_normalize_case_underscores_and_spaces() {
        for raw in ["PENDING_REVIEW", "pending review", "  Pending-Review  "] {
            assert_eq!(
                SpecStatus::parse(raw),
                SpecStatus::Review,
                "failed on {raw}"
            );
        }
        assert_eq!(SpecStatus::parse("wip"), SpecStatus::Draft);
        assert_eq!(SpecStatus::parse("ratified"), SpecStatus::Approved);
        assert_eq!(SpecStatus::parse("superseded"), SpecStatus::Deprecated);
        assert_eq!(SpecStatus::parse(""), SpecStatus::Unknown);
    }

    #[test]
    fn each_status_has_its_own_key_glyph_and_label() {
        let keys: Vec<&str> = SpecStatus::ALL.iter().map(|s| s.key()).collect();
        let glyphs: Vec<&str> = SpecStatus::ALL.iter().map(|s| s.glyph()).collect();
        for collection in [&keys, &glyphs] {
            let mut unique = collection.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(
                unique.len(),
                SpecStatus::ALL.len(),
                "duplicate in {collection:?}"
            );
        }
        for status in SpecStatus::ALL {
            assert!(!status.label().trim().is_empty());
            // A key round-trips through the parser, which the filter relies on.
            if status != SpecStatus::Unknown {
                assert_eq!(SpecStatus::parse(status.key()), status);
            }
        }
    }

    #[test]
    fn only_yaml_extensions_are_read_as_metadata_only_documents() {
        assert_eq!(DocKind::from_extension("yaml"), DocKind::Yaml);
        assert_eq!(DocKind::from_extension("YML"), DocKind::Yaml);
        assert_eq!(DocKind::from_extension("md"), DocKind::Markdown);
        assert_eq!(DocKind::from_extension(""), DocKind::Markdown);
    }

    #[test]
    fn checkboxes_are_recognized_behind_any_bullet_marker() {
        assert_eq!(checkbox_state("* [x] done"), Some(true));
        assert_eq!(checkbox_state("+ [ ] pending"), Some(false));
        assert_eq!(checkbox_state("- [X] done"), Some(true));
        assert_eq!(checkbox_state("- [✓] done"), Some(true));
        assert_eq!(checkbox_state("- [x]"), Some(true));
    }

    #[test]
    fn a_bracket_that_is_not_a_checkbox_is_not_counted() {
        assert_eq!(checkbox_state("- [x]no space"), None);
        assert_eq!(checkbox_state("- [y] unknown"), None);
        assert_eq!(checkbox_state("-[x] no bullet space"), None);
        assert_eq!(checkbox_state("[x] no bullet"), None);
        assert_eq!(checkbox_state(""), None);
    }

    #[test]
    fn requirement_keywords_are_counted_only_as_whole_words() {
        for line in [
            "The system MUST log in.",
            "It SHALL NOT retry.",
            "REQUIRED",
            "(MUST)",
        ] {
            assert!(contains_requirement_keyword(line), "missed {line:?}");
        }
        for line in [
            "MUSTARD is not a keyword.",
            "The must is lowercase.",
            "SHOULDER",
            "plain prose",
        ] {
            assert!(
                !contains_requirement_keyword(line),
                "false positive {line:?}"
            );
        }
    }

    #[test]
    fn metrics_ignore_everything_inside_a_code_fence() {
        // The same rule the renderer follows, so the counts and the view agree.
        let body = "\
# Heading

The system MUST work.

```
# not a heading
The example MUST NOT count.
- [ ] not a task
```

- [x] done
- [ ] pending
";
        let metrics = Metrics::from_body(body);
        assert_eq!(metrics.headings, 1);
        assert_eq!(metrics.requirements, 1);
        assert_eq!(metrics.tasks_total, 2);
        assert_eq!(metrics.tasks_done, 1);
        assert_eq!(metrics.lines, body.lines().count());
    }

    #[test]
    fn a_tilde_fence_hides_its_contents_too() {
        let metrics = Metrics::from_body("~~~\n# hidden\n~~~\n# shown\n");
        assert_eq!(metrics.headings, 1);
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_frontmatter() {
        let raw = "\u{feff}---\ntitle: Auth\n---\nBody.\n";
        let (frontmatter, body) = split_frontmatter(raw, Path::new("spec.md")).unwrap();
        assert_eq!(frontmatter.title.as_deref(), Some("Auth"));
        assert_eq!(body, "Body.\n");
    }

    #[test]
    fn the_frontmatter_block_may_close_with_three_dots() {
        let raw = "---\ntitle: Auth\n...\nBody.\n";
        let (frontmatter, body) = split_frontmatter(raw, Path::new("spec.md")).unwrap();
        assert_eq!(frontmatter.title.as_deref(), Some("Auth"));
        assert_eq!(body, "Body.\n");
    }

    #[test]
    fn windows_line_endings_are_accepted() {
        let raw = "---\r\ntitle: Auth\r\n---\r\nBody.\r\n";
        let (frontmatter, _) = split_frontmatter(raw, Path::new("spec.md")).unwrap();
        assert_eq!(frontmatter.title.as_deref(), Some("Auth"));
    }

    #[test]
    fn an_unterminated_frontmatter_block_is_treated_as_body() {
        // Losing the whole document to a missing fence would be worse than
        // showing it without metadata.
        let raw = "---\ntitle: Auth\n\nBody without a closing fence.\n";
        let (frontmatter, body) = split_frontmatter(raw, Path::new("spec.md")).unwrap();
        assert!(frontmatter.title.is_none());
        assert_eq!(body, raw);
    }

    #[test]
    fn a_horizontal_rule_is_not_mistaken_for_a_fence() {
        let raw = "--- not alone on its line\nBody.\n";
        let (frontmatter, body) = split_frontmatter(raw, Path::new("spec.md")).unwrap();
        assert!(frontmatter.title.is_none());
        assert_eq!(body, raw);
    }

    #[test]
    fn an_empty_frontmatter_block_yields_no_metadata() {
        let (frontmatter, body) =
            split_frontmatter("---\n---\nBody.\n", Path::new("s.md")).unwrap();
        assert!(frontmatter.title.is_none());
        assert_eq!(body, "Body.\n");
    }

    #[test]
    fn frontmatter_that_is_a_list_is_no_metadata_rather_than_an_error() {
        let raw = "---\n- one\n- two\n---\nBody.\n";
        let (frontmatter, body) = split_frontmatter(raw, Path::new("spec.md")).unwrap();
        assert!(frontmatter.title.is_none());
        assert_eq!(body, "Body.\n");
    }

    #[test]
    fn malformed_frontmatter_names_the_file_that_carries_it() {
        let raw = "---\ntitle: [unterminated\n---\nBody.\n";
        match split_frontmatter(raw, Path::new("/w/spec.md")) {
            Err(Error::Yaml { path, .. }) => assert_eq!(path, PathBuf::from("/w/spec.md")),
            other => panic!("expected Yaml, got {other:?}"),
        }
    }

    #[test]
    fn unknown_frontmatter_keys_survive_for_display() {
        let raw = "---\ntitle: Auth\nowner: ada\ntag:\n  - security\nreviewer: bob\n---\nBody.\n";
        let (frontmatter, _) = split_frontmatter(raw, Path::new("spec.md")).unwrap();
        assert_eq!(frontmatter.owner.as_deref(), Some("ada"));
        // `tag` is accepted as an alias of `tags`.
        assert_eq!(frontmatter.tags, ["security"]);
        assert!(frontmatter.extra.contains_key("reviewer"));
    }

    #[test]
    fn a_title_prefers_frontmatter_then_the_first_heading_then_the_file_name() {
        let temp = TempDir::new();

        let declared = temp.write("a.md", "---\ntitle: Declared\n---\n# Heading\n");
        assert_eq!(
            SpecDocument::load(&declared, &temp.0).unwrap().title,
            "Declared"
        );

        let heading = temp.write("b.md", "# From The Heading\n");
        assert_eq!(
            SpecDocument::load(&heading, &temp.0).unwrap().title,
            "From The Heading"
        );

        let bare = temp.write("user-authentication.md", "Body only.\n");
        assert_eq!(
            SpecDocument::load(&bare, &temp.0).unwrap().title,
            "User Authentication"
        );
    }

    #[test]
    fn a_generically_named_file_is_titled_after_its_directory() {
        // `spec.md` names a role, not a subject; the capability directory does.
        let temp = TempDir::new();
        let path = temp.write("billing-engine/spec.md", "Body only.\n");
        assert_eq!(
            SpecDocument::load(&path, &temp.0).unwrap().title,
            "Billing Engine"
        );
    }

    #[test]
    fn an_empty_heading_does_not_become_the_title() {
        assert_eq!(first_heading("#\n\nBody.\n"), None);
        assert_eq!(first_heading("Body only.\n"), None);
        assert_eq!(
            first_heading("## Second level\n"),
            Some("Second level".into())
        );
    }

    #[test]
    fn the_frontmatter_line_count_maps_a_body_line_back_to_the_file() {
        // Task toggling addresses lines in the file, not in the stripped body.
        let temp = TempDir::new();
        let path = temp.write(
            "tasks.md",
            "---\ntitle: Tasks\nstatus: draft\n---\n\n- [ ] first\n",
        );
        let document = SpecDocument::load(&path, &temp.0).unwrap();

        assert_eq!(document.status, SpecStatus::Draft);
        assert_eq!(document.body, "- [ ] first\n");
        assert_eq!(document.frontmatter_lines, 5);
    }

    #[test]
    fn a_yaml_document_is_metadata_from_top_to_bottom() {
        let temp = TempDir::new();
        let path = temp.write("api.yaml", "title: API\nstatus: approved\n");
        let document = SpecDocument::load(&path, &temp.0).unwrap();

        assert_eq!(document.kind, DocKind::Yaml);
        assert_eq!(document.title, "API");
        assert_eq!(document.status, SpecStatus::Approved);
        assert_eq!(document.body, "title: API\nstatus: approved\n");
    }

    #[test]
    fn loading_a_missing_file_reports_the_path() {
        let missing = Path::new("/tmp/openspectation-no-such-spec-xyz.md");
        assert!(matches!(
            SpecDocument::load(missing, Path::new("/tmp")),
            Err(Error::Io { .. })
        ));
    }

    #[test]
    fn a_document_is_placed_by_its_path_relative_to_the_scan_root() {
        let temp = TempDir::new();
        let nested = temp.write("billing/invoices/spec.md", "# Invoices\n");
        let document = SpecDocument::load(&nested, &temp.0).unwrap();
        assert_eq!(document.display_path(), "billing/invoices/spec.md");
        assert_eq!(document.group().as_deref(), Some("billing/invoices"));
        assert_eq!(document.file_name, "spec.md");

        let flat = temp.write("loose.md", "# Loose\n");
        assert_eq!(SpecDocument::load(&flat, &temp.0).unwrap().group(), None);
    }

    #[test]
    fn a_search_reaches_the_title_the_path_the_status_and_the_tags() {
        let temp = TempDir::new();
        let path = temp.write(
            "billing/spec.md",
            "---\ntitle: Invoicing\nstatus: approved\ntags:\n  - finance\n---\n# Invoicing\n",
        );
        let document = SpecDocument::load(&path, &temp.0).unwrap();

        for needle in ["invoic", "BILLING", "approved", "finance", ""] {
            assert!(document.matches(needle), "missed {needle:?}");
        }
        assert!(!document.matches("nonexistent"));
    }

    #[test]
    fn a_document_built_in_memory_still_carries_its_metrics() {
        // The `context:` block of a nested workspace has no file of its own.
        let document = SpecDocument::from_memory(
            Path::new("/w/openspec/project.md"),
            Path::new("/w/openspec"),
            "Project".to_string(),
            "# Context\n\nThe system MUST ship.\n\n- [x] done\n".to_string(),
            None,
        );

        assert_eq!(document.title, "Project");
        assert_eq!(document.status, SpecStatus::Unknown);
        assert_eq!(document.headings, 1);
        assert_eq!(document.requirements, 1);
        assert_eq!((document.tasks_done, document.tasks_total), (1, 1));
        assert_eq!(document.frontmatter_lines, 0);
        assert!(!document.truncated);
        assert_eq!(document.display_path(), "project.md");
    }
}
