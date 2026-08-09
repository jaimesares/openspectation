# Prepare the crate for publication on crates.io and GitHub

## Why

Anyone running `cargo publish` today would ship the wrong crate: `cargo package --list` bundles 112 files, including `.claude/`, `.opencode/`, `CLAUDE.md`, the whole `demo/` toolkit and the `openspec/` planning tree — agent configuration and planning documents, not the tool. The manifest declares a license no file in the repository backs, the declared `repository` URL has no matching git remote, the YAML engine is an archived crate with a RustSec unmaintained advisory, and there is no CI behind the claims a public README makes. The project is one `cargo publish` away from an embarrassing first release, so the gaps get closed now, before the version number becomes public history.

The release itself — creating the GitHub repository, pushing, running `cargo publish` — stays in the maintainer's hands. This change prepares everything up to that point and stops.

## What Changes

- **YAML engine migration**: `serde_yaml` (archived upstream, RustSec unmaintained advisory) is replaced by `serde_norway`, the maintained drop-in fork with the same API. Fourteen call sites across six files rename their imports; the existing inline test suite is the behaviour gate.
- **Crate packaging**: an explicit `include` list in `Cargo.toml` so the published crate carries only what compiles and documents it: `src/`, `locales/`, `build.rs`, `README.md`, the license file and the manifest. Verified with `cargo package --list` and `cargo publish --dry-run`.
- **Licensing**: MIT, single license. A `LICENSE` file at the repository root, `license = "MIT"` in the manifest (currently `MIT OR Apache-2.0`), and a matching README section.
- **Manifest metadata**: `repository` and `homepage` set to `https://github.com/jaimesares/openspectation`; keywords, categories, description and `rust-version` reviewed against crates.io limits; crate name availability on crates.io checked and recorded.
- **Security review** of every file in the tree, with each finding either fixed or explicitly accepted in writing:
  - the three shell-out surfaces (`openspec` CLI, `git`, `$EDITOR`/`$VISUAL`) — argv construction, PATH resolution, working directories;
  - the three `unsafe` sites (`lib.rs` SIGPIPE handler, `env::set_var` in test code);
  - filesystem handling — temp-file save path, editor size cap, walkdir traversal and symlink behaviour, settings/state file locations;
  - the shell scripts under `scripts/` and `demo/`;
  - the dependency tree via `cargo audit` and `cargo deny` (licenses + advisories), expected clean once the YAML migration lands.
- **GitHub scaffolding**: a CI workflow (fmt, clippy, test), a tag-driven release workflow that builds both binaries for every practical target — x86_64 and aarch64, Linux glibc and musl, macOS, Windows — attaches them to the GitHub Release and publishes to crates.io via Trusted Publishing, a Dependabot config for cargo and actions ecosystems, and a decision on whether `.claude/` and `.opencode/` are committed or ignored (they are currently untracked).
- **README release polish**: crates.io / CI / license badges, a `cargo install openspectation` install path alongside the build-from-source one, and a link pass over the settled repository URL.
- **`CHANGELOG.md`** seeded with the 0.1.0 entry, Keep a Changelog format.
- Explicitly out of scope: pushing to GitHub, tagging, and running `cargo publish`.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None — packaging, documentation, repository tooling and a behaviour-preserving dependency swap. `serde_norway` is a fork of the same engine with the same API; the inline tests over frontmatter, settings, workspace config and locale catalogues are the proof it parses what `serde_yaml` parsed. No requirement changes, so the change sets `skip_specs: true`. If the security review turns up a defect that requires a behaviour change, that fix becomes its own change with its own delta.

## Impact

- **Files**: `Cargo.toml` (dependency, metadata, `include`), six source files renaming YAML imports (`error.rs`, `i18n.rs`, `domain/{settings,spec,config,openspec}.rs`), new `LICENSE`, `CHANGELOG.md`, `RELEASING.md`, `.github/workflows/*.yml`, `.github/dependabot.yml`, `README.md`, possibly `.gitignore`.
- **User-facing strings**: none — locale catalogues are untouched. License text, CI configuration and README are repository boilerplate, which stays English by convention.
- **Dependencies**: `serde_yaml` → `serde_norway` — a swap, not an addition; the existing set cannot cover it because the existing engine itself is the finding. No other dependency moves. `cargo-audit` and `cargo-deny` run as developer/CI tools; nothing links into the binary.
- **Maintainer decisions, resolved**: license MIT, repository `jaimesares/openspectation`, release artifacts for all practical targets. Remaining manual steps (create the repo, first publish) are documented in `RELEASING.md`, not executed.
