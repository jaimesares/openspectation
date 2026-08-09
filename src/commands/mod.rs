//! Subcommand implementations.
//!
//! Each command assembles the whole block it is about to show before any of it
//! reaches the terminal, and hands the result to `emit`. Rendering that
//! returns its text can be read back and asserted on; a `println!` buried in a
//! loop can only be observed by running the binary. The commands that shell out
//! still print in stages, so a long running subprocess is announced before it
//! starts rather than after it finishes.

pub mod archive;
pub mod diff;
pub mod init;
pub mod list;
pub mod tui;
pub mod validate;

use crate::cli::{Cli, Commands};
use crate::error::Result;
use crate::paint::Painter;

/// Routes a parsed command line to its implementation.
pub fn dispatch(cli: &Cli, painter: Painter) -> Result<()> {
    match &cli.command {
        Commands::Init(args) => init::run(&cli.path, args, painter),
        Commands::List(args) => list::run(&cli.path, args, painter),
        Commands::Tui(args) => tui::run(&cli.path, args),
        Commands::Diff(args) => diff::run(&cli.path, args, painter),
        Commands::Validate(args) => validate::run(&cli.path, args, painter),
        Commands::Archive(args) => archive::run(&cli.path, args, painter),
    }
}

/// Writes rendered lines to stdout, one line each.
///
/// The single place in this layer that touches the terminal, so the bytes a
/// command produces are exactly the lines it returned.
fn emit(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

#[cfg(test)]
mod tests {
    //! Workspaces on disk shared by the tests of every subcommand.
    //!
    //! One fixture type rather than one per module: the commands read the same
    //! model, and a listing, a delta and an archive refusal are only meaningful
    //! against the same tree.

    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::dispatch;
    use crate::cli::{
        ArchiveArgs, Cli, Commands, DiffArgs, InitArgs, ListArgs, SortKey, TuiArgs, ValidateArgs,
    };
    use crate::domain::workspace::Workspace;
    use crate::error::Error;
    use crate::paint::Painter;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// A workspace in a directory of its own, removed when the test ends.
    pub(super) struct Fixture {
        pub(super) root: PathBuf,
    }

    impl Fixture {
        /// A flat workspace: configuration, design document, three
        /// specifications across two groups, one active change declaring a
        /// requirement delta, and one archived change.
        pub(super) fn new() -> Self {
            let fixture = Self::scratch();
            let root = &fixture.root;

            write(
                &root.join("openspec.yaml"),
                "version: 1\nproject:\n  name: Demo Project\n  description: A workspace to read from.\n",
            );
            write(
                &root.join("design.md"),
                "# Demo Project\n\nHow the pieces fit together.\n",
            );

            write(
                &root.join("specs/billing.md"),
                "---\ntitle: Billing\nstatus: approved\n---\n\n# Billing\n\n\
                 The system MUST issue an invoice for every order.\n",
            );
            // Named so that ordering by path and ordering by name disagree: it
            // sits at the root of specs/ yet sorts after the nested documents.
            write(
                &root.join("specs/workflow.md"),
                "---\ntitle: Workflow\nstatus: review\n---\n\n\
                 A proposal SHOULD be reviewed before it is approved.\n",
            );
            write(
                &root.join("specs/auth/spec.md"),
                "---\nstatus: draft\n---\n\n# Authentication\n\n## Requirements\n\n\
                 ### Requirement: Tokens MUST expire after an hour\n",
            );
            write(
                &root.join("specs/auth/tokens.md"),
                "---\ntitle: Tokens\nstatus: implemented\n---\n\n\
                 Rotation SHOULD happen out of band.\n",
            );

            write(
                &root.join("changes/add-oauth/proposal.md"),
                "# Add OAuth\n\nBecause tokens expire.\n",
            );
            write(
                &root.join("changes/add-oauth/tasks.md"),
                "## 1. Groundwork\n\n- [x] 1.1 Pick a library\n- [ ] 1.2 Wire the refresh flow\n",
            );
            write(
                &root.join("changes/add-oauth/specs/auth/spec.md"),
                "## ADDED Requirements\n\n### Requirement: Tokens SHALL refresh silently\n\n\
                 #### Scenario: Expired token\n\n- **WHEN** the token expired\n- **THEN** it refreshes\n\n\
                 ## REMOVED Requirements\n\n### Requirement: Sessions SHALL live forever\n",
            );
            write(
                &root.join("changes/add-oauth/specs/telemetry/spec.md"),
                "## MODIFIED Requirements\n\n### Requirement: Events SHOULD carry a trace id\n\n\
                 ## RENAMED Requirements\n\n### Requirement: Metrics SHALL be sampled\n",
            );
            write(
                &root.join("changes/archive/2026-01-01-old-thing/proposal.md"),
                "# Old thing\n\nShipped already.\n",
            );

            fixture
        }

        /// A workspace whose layout exists but holds no document at all.
        pub(super) fn bare() -> Self {
            let fixture = Self::scratch();
            std::fs::create_dir_all(fixture.root.join("specs")).unwrap();
            std::fs::create_dir_all(fixture.root.join("changes")).unwrap();
            fixture
        }

        /// An empty directory, for the commands that create the workspace.
        pub(super) fn scratch() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "openspectation-cmd-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        pub(super) fn workspace(&self) -> Workspace {
            Workspace::open(&self.root).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// A painter that emits no escape sequence, so assertions read the text
    /// rather than the styling and do not depend on stdout being a terminal.
    pub(super) fn plain() -> Painter {
        Painter::detect(true)
    }

    fn cli(root: &Path, command: Commands) -> Cli {
        Cli {
            path: root.to_path_buf(),
            lang: None,
            no_color: true,
            command,
        }
    }

    #[test]
    fn every_subcommand_reaches_its_implementation() {
        let fixture = Fixture::new();

        let listing = cli(
            &fixture.root,
            Commands::List(ListArgs {
                all: true,
                status: None,
                sort: SortKey::Path,
            }),
        );
        assert!(dispatch(&listing, plain()).is_ok());

        let delta = cli(
            &fixture.root,
            Commands::Diff(DiffArgs {
                change: Some("add-oauth".to_string()),
            }),
        );
        assert!(dispatch(&delta, plain()).is_ok());

        let scaffold = Fixture::scratch();
        let creation = cli(
            &scaffold.root,
            Commands::Init(InitArgs {
                name: "Routed Project".to_string(),
                description: String::new(),
                force: false,
            }),
        );
        assert!(dispatch(&creation, plain()).is_ok());
        assert!(scaffold.root.join("openspec.yaml").is_file());

        // The two commands that shell out are routed with an argument that is
        // refused before a process is started for it.
        let validation = cli(
            &fixture.root,
            Commands::Validate(ValidateArgs {
                change: Some("no-such-change".to_string()),
                loose: false,
            }),
        );
        assert!(matches!(
            dispatch(&validation, plain()),
            Err(Error::UnknownChange(_))
        ));

        let archiving = cli(
            &fixture.root,
            Commands::Archive(ArchiveArgs {
                change: "no-such-change".to_string(),
                yes: true,
                skip_specs: false,
            }),
        );
        assert!(matches!(
            dispatch(&archiving, plain()),
            Err(Error::UnknownChange(_))
        ));
    }

    /// The router must not swallow a failure on its way back out, and the
    /// interactive command has to refuse before it takes over the screen.
    #[test]
    fn a_failing_subcommand_reports_through_the_router() {
        let fixture = Fixture::new();
        let (_guard, settings) = crate::domain::settings::redirect_for_test("commands-dispatch");

        let interactive = cli(
            &fixture.root,
            Commands::Tui(TuiArgs {
                tick_rate: 250,
                change: Some("no-such-change".to_string()),
                theme: None,
            }),
        );

        assert!(matches!(
            dispatch(&interactive, plain()),
            Err(Error::UnknownChange(_))
        ));

        let _ = std::fs::remove_file(&settings);
    }
}
