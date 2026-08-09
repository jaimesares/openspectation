//! Boilerplate written by `openspectation init`.
//!
//! Generated documents are always English: they become part of the user's
//! repository, where the project convention is English source artefacts. The
//! interface language detected at runtime is recorded in `openspec.yaml`
//! (`project.language`) so downstream tooling can still act on it.

use crate::domain::util;

/// `design.md` scaffold.
pub fn design(name: &str, description: &str, today: &str) -> String {
    let summary = if description.trim().is_empty() {
        format!("High level design for {name}.")
    } else {
        description.trim().to_string()
    };

    format!(
        "---\n\
         title: {name} — Design\n\
         status: draft\n\
         version: 0.1.0\n\
         updated: {today}\n\
         tags:\n\
         \x20 - design\n\
         ---\n\
         \n\
         # {name} — Design\n\
         \n\
         {summary}\n\
         \n\
         ## Context\n\
         \n\
         Describe the problem space, the constraints and the assumptions that hold\n\
         at the time of writing.\n\
         \n\
         ## Goals\n\
         \n\
         - Goal one\n\
         - Goal two\n\
         \n\
         ## Non-goals\n\
         \n\
         - Everything explicitly out of scope\n\
         \n\
         ## Architecture\n\
         \n\
         Describe the components and how they interact.\n\
         \n\
         ```text\n\
         client -> api -> store\n\
         ```\n\
         \n\
         ## Decisions\n\
         \n\
         | Decision | Rationale | Status |\n\
         | -------- | --------- | ------ |\n\
         | Example  | Because   | draft  |\n\
         \n\
         ## Open questions\n\
         \n\
         - [ ] What is still undecided?\n\
         \n\
         ## References\n\
         \n\
         - [OpenSpec](https://github.com/Fission-AI/OpenSpec)\n"
    )
}

/// Example specification placed in `specs/`.
pub fn example_spec(name: &str, today: &str) -> String {
    let slug = slugify(name);

    format!(
        "---\n\
         title: {name} — Core Capability\n\
         status: draft\n\
         version: 0.1.0\n\
         owner: unassigned\n\
         updated: {today}\n\
         tags:\n\
         \x20 - core\n\
         ---\n\
         \n\
         # {name} — Core Capability\n\
         \n\
         One paragraph describing what this capability does and for whom.\n\
         \n\
         ## Requirement: {slug} baseline\n\
         \n\
         The system MUST expose the baseline behaviour described in this document.\n\
         The system SHOULD fail with an actionable message when the behaviour is\n\
         unavailable.\n\
         \n\
         ### Scenario: happy path\n\
         \n\
         - **Given** a valid request\n\
         - **When** the capability is invoked\n\
         - **Then** a successful response is returned\n\
         \n\
         ### Scenario: invalid input\n\
         \n\
         - **Given** a malformed request\n\
         - **When** the capability is invoked\n\
         - **Then** the request MUST be rejected without side effects\n\
         \n\
         ## Acceptance criteria\n\
         \n\
         - [ ] Happy path covered by an automated test\n\
         - [ ] Invalid input covered by an automated test\n\
         - [ ] Observability in place\n"
    )
}

/// Example change proposal placed in `changes/`.
pub fn example_change(name: &str, today: &str) -> String {
    format!(
        "---\n\
         title: Bootstrap the {name} specification set\n\
         status: draft\n\
         updated: {today}\n\
         tags:\n\
         \x20 - bootstrap\n\
         ---\n\
         \n\
         # Bootstrap the {name} specification set\n\
         \n\
         ## Why\n\
         \n\
         Explain the motivation and the cost of doing nothing.\n\
         \n\
         ## What changes\n\
         \n\
         Describe the delta against the current specifications.\n\
         \n\
         ## Impact\n\
         \n\
         | Area | Impact |\n\
         | ---- | ------ |\n\
         | Specs | new documents under specs/ |\n\
         \n\
         ## Tasks\n\
         \n\
         - [ ] Draft the capability specification\n\
         - [ ] Review with the stakeholders\n\
         - [ ] Approve and merge\n"
    )
}

/// `specs/README.md`, explaining the convention to newcomers.
pub fn specs_readme(name: &str) -> String {
    format!(
        "# Specifications — {name}\n\
         \n\
         One document per capability. Each document starts with a YAML\n\
         frontmatter block:\n\
         \n\
         ```yaml\n\
         ---\n\
         title: Capability name\n\
         status: draft | review | approved | implemented | deprecated\n\
         version: 0.1.0\n\
         owner: team-or-person\n\
         tags: [core]\n\
         ---\n\
         ```\n\
         \n\
         Requirements use RFC 2119 keywords (MUST, SHOULD, MAY) so they can be\n\
         counted and audited automatically.\n"
    )
}

/// `changes/README.md`, explaining the convention to newcomers.
pub fn changes_readme(name: &str) -> String {
    format!(
        "# Change proposals — {name}\n\
         \n\
         A change is either a single Markdown file or a directory:\n\
         \n\
         ```text\n\
         changes/\n\
         \x20 add-oauth-login/\n\
         \x20   proposal.md\n\
         \x20   tasks.md\n\
         \x20   design.md\n\
         \x20 rename-billing-api.md\n\
         ```\n\
         \n\
         Task checkboxes (`- [ ]` / `- [x]`) drive the progress reported by\n\
         `openspectation list --all` and by the dashboard.\n"
    )
}

/// Files written by `:new <id>`, shaped the way the OpenSpec CLI expects.
///
/// The scaffold is deliberately empty of opinion: headings the workflow
/// requires, and prompts for everything else. It is a starting point for a
/// person or an agent, not a document pretending to be finished.
pub fn change_scaffold(slug: &str) -> Vec<(&'static str, String)> {
    let title = util::humanize(slug);

    vec![
        (
            "proposal.md",
            format!(
                "# {title}\n\
                 \n\
                 ## Why\n\
                 \n\
                 Explain the motivation and the cost of doing nothing.\n\
                 \n\
                 ## What changes\n\
                 \n\
                 Describe the delta against the current specifications, then\n\
                 write it as `specs/<capability>/spec.md` inside this change,\n\
                 using `## ADDED Requirements`, `## MODIFIED Requirements` or\n\
                 `## REMOVED Requirements` sections.\n\
                 \n\
                 ## Non-goals\n\
                 \n\
                 - Everything explicitly out of scope\n\
                 \n\
                 ## Impact\n\
                 \n\
                 | Area | Impact |\n\
                 | ---- | ------ |\n\
                 | Specs | capabilities touched by this change |\n"
            ),
        ),
        (
            "tasks.md",
            "## 1. Implementation\n\
             \n\
             - [ ] 1.1 First step\n\
             - [ ] 1.2 Second step\n\
             \n\
             ## 2. Verification\n\
             \n\
             - [ ] 2.1 Automated coverage\n\
             - [ ] 2.2 Manual check, if the behaviour cannot be simulated\n"
                .to_string(),
        ),
        (
            "design.md",
            format!(
                "# {title} — Design\n\
                 \n\
                 ## Context\n\
                 \n\
                 Constraints and assumptions that hold at the time of writing.\n\
                 \n\
                 ## Decisions\n\
                 \n\
                 | Decision | Rationale |\n\
                 | -------- | --------- |\n\
                 | Example  | Because   |\n\
                 \n\
                 ## Open questions\n\
                 \n\
                 - [ ] What is still undecided?\n"
            ),
        ),
    ]
}

/// Lowercase, dash separated identifier derived from a project name.
///
/// Empty when the name carries nothing that can name a directory. Callers
/// create a path out of the result, so the empty string is the refusal and
/// has to be checked for.
pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut previous_dash = true;

    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        salvage(name)
    } else {
        trimmed.to_string()
    }
}

/// The best path component a name with no ASCII letters or digits can offer.
///
/// A name written in a script the slug above cannot represent still names a
/// directory perfectly well, so it is kept rather than refused, once every
/// character that cannot appear in a path component is gone. A name with no
/// letter or digit left in any script has nothing to name anything with, and
/// the empty string reports that instead of handing back the punctuation it
/// was given: this used to answer `!!!` for `!!!`, and the callers went on to
/// create a directory called that.
fn salvage(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|character| !is_path_hostile(*character))
        .collect();

    // Leading dots are trimmed rather than filtered out, because a name is
    // allowed to contain one and not allowed to *be* `.` or `..`.
    let kept = kept.trim().trim_matches('.').trim();

    if !kept.chars().any(char::is_alphanumeric) {
        return String::new();
    }

    kept.split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

/// Whether a character cannot appear in a path component somewhere this tool
/// runs. The Windows reserved set is refused everywhere: a workspace is meant
/// to be committed and read back on another machine.
fn is_path_hostile(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
        )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::spec::{SpecDocument, split_frontmatter};

    const NAME: &str = "Payments API";
    const TODAY: &str = "2026-01-01";

    #[test]
    fn slugifies_project_names() {
        assert_eq!(slugify("Payments API"), "payments-api");
        assert_eq!(slugify("  Weird__Name!!  "), "weird-name");
        // Only ASCII survives, so an accent breaks the word in two.
        assert_eq!(slugify("Especificación Técnica"), "especificaci-n-t-cnica");
    }

    /// A name the ASCII slug cannot represent is kept when it can still name a
    /// directory, because refusing it would turn away every language that does
    /// not spell in ASCII.
    #[test]
    fn a_name_without_ascii_characters_still_yields_a_slug() {
        assert_eq!(slugify("ñ"), "ñ");
        assert_eq!(slugify("日本語"), "日本語");
        assert_eq!(slugify("Проект Один"), "проект-один");
    }

    /// The result is joined onto a path, so a name with nothing to name a
    /// directory with has to come back empty rather than come back verbatim.
    /// `slugify("!!!")` once answered `!!!`, and `openspectation init '!!!'`
    /// went on to write `specs/!!!.md`.
    #[test]
    fn a_name_that_cannot_be_a_path_component_slugifies_to_nothing() {
        for name in ["", "   ", "!!!", "---", "...", "..", ".", "?*|", "\u{7}"] {
            assert_eq!(slugify(name), "", "{name:?} should slugify to nothing");
        }
    }

    /// The separators are removed rather than encoded: a slug that carried one
    /// would reach into a directory the caller never named.
    #[test]
    fn a_slug_never_carries_a_path_separator() {
        for name in ["ñ/ñ", "ñ\\ñ", "日本/語", "a/b"] {
            let slug = slugify(name);
            assert!(!slug.is_empty(), "{name:?} still names something");
            assert!(
                !slug.contains('/') && !slug.contains('\\'),
                "{name:?} produced {slug:?}"
            );
        }
    }

    #[test]
    fn generated_design_carries_frontmatter() {
        let content = design("Demo", "", TODAY);
        assert!(content.starts_with("---\n"));
        assert!(content.contains("status: draft"));

        let (frontmatter, body) = split_frontmatter(&content, Path::new("design.md")).unwrap();
        assert_eq!(frontmatter.title.as_deref(), Some("Demo — Design"));
        assert_eq!(frontmatter.tags, ["design"]);
        assert!(body.starts_with("# Demo — Design\n"));
    }

    #[test]
    fn a_description_replaces_the_generated_summary() {
        let described = design("Demo", "  Ships invoices on time.  ", TODAY);
        assert!(described.contains("\nShips invoices on time.\n"));
        assert!(!described.contains("High level design for Demo."));

        // Whitespace is not a description.
        assert!(design("Demo", "   \n ", TODAY).contains("High level design for Demo."));
    }

    #[test]
    fn the_example_specification_is_a_document_the_tool_can_read() {
        let content = example_spec(NAME, TODAY);
        let (frontmatter, _) = split_frontmatter(&content, Path::new("payments-api.md")).unwrap();

        assert_eq!(
            frontmatter.title.as_deref(),
            Some("Payments API — Core Capability")
        );
        assert_eq!(frontmatter.status.as_deref(), Some("draft"));
        assert_eq!(frontmatter.updated.as_deref(), Some(TODAY));
        assert_eq!(frontmatter.tags, ["core"]);

        let document = analysed(&content, "payments-api.md");
        // The slug of the project names the requirement, so `init` writes a
        // document whose requirement heading matches its own file name.
        assert!(
            document
                .body
                .contains("## Requirement: payments-api baseline")
        );
        // RFC 2119 keywords are what `domain::spec` counts; a scaffold without
        // them would report a workspace with zero requirements.
        assert_eq!(document.requirements, 3);
        assert_eq!((document.tasks_total, document.tasks_done), (3, 0));
    }

    #[test]
    fn the_example_change_ships_with_nothing_ticked() {
        let content = example_change(NAME, TODAY);
        let (frontmatter, body) =
            split_frontmatter(&content, Path::new("bootstrap-payments-api.md")).unwrap();

        assert_eq!(frontmatter.tags, ["bootstrap"]);
        assert!(body.starts_with("# Bootstrap the Payments API specification set\n"));

        let document = analysed(&content, "bootstrap-payments-api.md");
        assert_eq!((document.tasks_total, document.tasks_done), (3, 0));
    }

    #[test]
    fn both_readmes_name_the_project_and_its_conventions() {
        let specs = specs_readme(NAME);
        assert!(specs.starts_with("# Specifications — Payments API\n"));
        assert!(specs.contains("RFC 2119"));

        let changes = changes_readme(NAME);
        assert!(changes.starts_with("# Change proposals — Payments API\n"));
        assert!(changes.contains("- [ ]"));
    }

    #[test]
    fn the_change_scaffold_writes_the_files_the_cli_expects() {
        let files = change_scaffold("add-oauth-login");
        let names: Vec<&str> = files.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["proposal.md", "tasks.md", "design.md"]);

        let body = |wanted: &str| {
            files
                .iter()
                .find(|(name, _)| *name == wanted)
                .map(|(_, body)| body.clone())
                .expect("scaffold entry")
        };

        assert!(body("proposal.md").starts_with("# Add Oauth Login\n"));
        assert!(body("design.md").starts_with("# Add Oauth Login — Design\n"));

        // The proposal has to spell out the delta sections `domain::delta`
        // reads, since nothing else tells an author they exist.
        let proposal = body("proposal.md");
        for section in [
            "## ADDED Requirements",
            "## MODIFIED Requirements",
            "## REMOVED Requirements",
        ] {
            assert!(proposal.contains(section), "the proposal omits {section}");
        }

        let tasks = analysed(&body("tasks.md"), "tasks.md");
        assert_eq!((tasks.tasks_total, tasks.tasks_done), (4, 0));
    }

    /// The boilerplate becomes part of the user's repository, where the
    /// convention is English source artefacts, so it must not follow the
    /// interface language. Every character is ASCII bar the em dash used in the
    /// titles; a translated template would drag accents in with it.
    #[test]
    fn every_generated_document_stays_english() {
        for (file, content) in every_document() {
            for character in content.chars() {
                assert!(
                    character.is_ascii() || character == '—',
                    "{file} carries {character:?}"
                );
            }
        }
    }

    /// Complements the check above, which a translation into another Latin
    /// script language could slip past. Rendering the templates under a second
    /// catalogue is not an option — the active locale is process wide and this
    /// binary is threaded — but the absence of any catalogue lookup in this
    /// module is exactly as strong a statement.
    #[test]
    fn the_module_never_reaches_for_the_catalogue() {
        let source = include_str!("template.rs");
        // Split so this assertion is not itself a match.
        let catalogue = concat!("rust", "_i18n");
        assert!(
            !source.contains(catalogue),
            "generated boilerplate must not be translated"
        );
    }

    /// Every document `init` writes, keyed by its destination. Mirrors the file
    /// list in `commands::init`.
    fn every_document() -> Vec<(String, String)> {
        let slug = slugify(NAME);
        let mut documents = vec![
            ("design.md".to_string(), design(NAME, "", TODAY)),
            ("specs/README.md".to_string(), specs_readme(NAME)),
            (format!("specs/{slug}.md"), example_spec(NAME, TODAY)),
            ("changes/README.md".to_string(), changes_readme(NAME)),
            (
                format!("changes/bootstrap-{slug}.md"),
                example_change(NAME, TODAY),
            ),
        ];
        documents.extend(
            change_scaffold("add-oauth-login")
                .into_iter()
                .map(|(name, body)| (format!("changes/add-oauth-login/{name}"), body)),
        );
        documents
    }

    /// Runs a generated document through the same analysis the workspace model
    /// applies to a file on disk, without needing one.
    fn analysed(raw: &str, name: &str) -> SpecDocument {
        let path = Path::new(name);
        let (frontmatter, body) = split_frontmatter(raw, path).expect("valid frontmatter");
        SpecDocument::from_memory(
            path,
            Path::new(""),
            frontmatter.title.unwrap_or_default(),
            body,
            None,
        )
    }
}
