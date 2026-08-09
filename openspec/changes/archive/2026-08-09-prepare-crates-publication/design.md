# Design — prepare-crates-publication

## Context

See proposal.md for motivation. The relevant current state:

- `cargo package --list` reports 112 files; the crate would ship `.claude/`, `.opencode/`, `CLAUDE.md`, `demo/`, `scripts/` and the entire `openspec/` planning tree.
- `Cargo.toml` already carries `description`, `readme`, five keywords, two categories and `rust-version = "1.88"`, but declares `MIT OR Apache-2.0` with no license file behind it, and points `repository` at a placeholder. The maintainer has settled both: **MIT**, at **`https://github.com/jaimesares/openspectation`** (repository not created yet; there is no git remote).
- The crate compiles its locale catalogues in: `rust_i18n::i18n!("locales", …)` reads `locales/*.yml` at compile time and `build.rs` re-runs on changes there. Whatever packaging shape is chosen must keep `locales/` and `build.rs` inside the crate or the published `.crate` file does not compile.
- `serde_yaml` appears at fourteen call sites in six files: `error.rs` (the `Yaml` error variant wraps `serde_yaml::Error`), `domain/settings.rs`, `domain/spec.rs` (frontmatter `extra` map holds `serde_yaml::Value`), `domain/config.rs`, `domain/openspec.rs`, and the key-parity test in `i18n.rs`.
- Shell-outs are argv-array based (`Command::new` + `.args`), never a shell: the upstream `openspec` CLI (`domain/openspec.rs`), `git` (`domain/history.rs`, with `GIT_OPTIONAL_LOCKS=0` and pinned diff flags), and `$VISUAL`/`$EDITOR` (`tui/mod.rs`, value split on whitespace by design).
- Three `unsafe` sites: the SIGPIPE restorer in `lib.rs` and two `env::set_var` calls in test code (`set_var` is unsafe in edition 2024).
- The runtime invariants (i18n key parity, runtime clap help, deferred blocking work, viewer cache identity keys, exact-width Markdown pre-wrap) are untouched. The one source-level change — the YAML import rename — sits below all of them: same API, same parse behaviour, and the i18n parity test itself is one of the renamed call sites, so the invariant's own guard exercises the new engine.

## Goals / Non-Goals

**Goals:**

- A `cargo publish --dry-run` that packages a minimal, compilable crate.
- A dependency tree with zero RustSec advisories — maintained YAML engine included.
- License claim backed by a file; README accurate for a stranger arriving from crates.io.
- A written security disposition for every risk surface: fixed, or accepted with a reason.
- CI and release automation that work the day the maintainer pushes, with no long-lived secrets, and release binaries for every target people realistically install on.

**Non-Goals:**

- Running the release: no push, no tag, no `cargo publish`, no GitHub repo creation.
- Distro packaging (dnf/Flatpak/AUR), binary signing, SBOMs.
- Any YAML engine beyond a drop-in fork — no move to a different parser family.
- MSRV changes.
- Translating repository boilerplate — license, CI, CHANGELOG stay English per project convention.

## Decisions

### 1. Packaging: `include` allowlist, not `exclude`

`include = ["src/**", "locales/*.yml", "build.rs", "README.md", "LICENSE", "Cargo.toml"]` (cargo adds `Cargo.lock`, `Cargo.toml.orig` and `.cargo_vcs_info.json` itself).

An exclude list is the obvious choice and is rejected because it fails open: every future directory — planning trees, recordings, agent configs — ships to crates.io unless someone remembers the list. The allowlist fails closed; forgetting it costs a missing file at `cargo publish --dry-run`, which is loud, not a leak, which is silent. The dry-run compiles the packaged crate, so a forgotten `locales/` or `build.rs` is caught before anything is public.

### 2. YAML engine: migrate to `serde_norway`

`serde_yaml` is archived upstream and carries a RustSec unmaintained advisory; the maintainer chose migration over a documented exception. The replacement is `serde_norway`: a maintained hard-fork of `serde_yaml` with the identical API, whose engine (`unsafe-libyaml-norway`) is the same transpiled-to-Rust libyaml the original used — pure Rust, no C toolchain, cross-compiles everywhere the release matrix goes. The migration is a dependency line plus an import rename at the fourteen call sites; the existing inline tests over frontmatter parsing, settings round-trips, workspace config and the locale-catalogue parity check are the behaviour gate.

Alternatives rejected:
- `serde_yml`: archived after unsoundness findings; strictly worse than the frozen original.
- `serde-saphyr` / `yaml-serde` / `serde-yaml-ng`: different engine or younger fork — a parser-family change risks parse-edge differences in `openspec.yaml`, `config.yaml`, frontmatter and settings, which is more change than this release needs.
- Cargo's package-rename trick (`serde_yaml = { package = "serde_norway" }`) keeping imports untouched: rejected because it makes the manifest lie to the reader; fourteen mechanical renames are cheaper than a permanent aliasing surprise.

### 3. CI: one workflow — fmt, clippy, test, MSRV; three OS targets

Single `ci.yml`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` on a `{ubuntu, macos, windows}-latest` matrix with stable Rust, plus one `cargo check` job pinned to 1.88 so the declared `rust-version` cannot rot silently.

Windows has never been exercised (`restore_sigpipe` is already `cfg(unix)`; settings paths degrade to `None` without `HOME`). It goes into the matrix anyway: crossterm/ratatui support it and the tests use temp dirs, so it should pass — if it does not, the entry is marked `continue-on-error`, the gap is recorded in the README, and fixing it becomes a follow-up change. Testing only Linux was rejected because crates.io users install from all three.

### 4. Release: tag-driven workflow, eight binary targets, crates.io Trusted Publishing

`release.yml` triggers on `v*` tags and builds `openspectation` + `ospc` archives for every target people realistically install on:

| Target | Builder |
| --- | --- |
| `x86_64-unknown-linux-gnu` | ubuntu runner, native |
| `x86_64-unknown-linux-musl` | ubuntu runner, `cross` |
| `aarch64-unknown-linux-gnu` | ubuntu runner, `cross` |
| `aarch64-unknown-linux-musl` | ubuntu runner, `cross` |
| `x86_64-apple-darwin` | macos runner, `--target` cross-build |
| `aarch64-apple-darwin` | macos runner, native |
| `x86_64-pc-windows-msvc` | windows runner, native |
| `aarch64-pc-windows-msvc` | windows runner, `--target` cross-build |

`cross` (container-based) covers the non-native Linux targets because the tree is pure Rust end to end — after decision 2 even the YAML engine is transpiled Rust — so no target needs a foreign C toolchain. Apple and Windows hosts cross-build their sibling architecture with a plain `--target` flag, which their native toolchains support. Musl builds are fully static, the artifact for containers and old distros.

Publishing: `cargo publish` runs from the Linux job authenticated via crates.io Trusted Publishing (OIDC) — no long-lived secret to leak or rotate. Constraint accepted: crates.io only lets an *existing* crate configure a trusted publisher, so release 0.1.0 is published manually by the maintainer from their machine — which is exactly what the maintainer asked to keep for themselves — and the workflow takes over from 0.1.1.

### 5. `.claude/` and `.opencode/` get committed

They hold the generated opsx commands and skills that drive the OpenSpec workflow this repository itself uses; the `openspec/` tree they front is already committed. A contributor cloning the repo without them loses the planning workflow. Gitignoring them was rejected for that reason; the security pass verifies they contain no machine-specific paths or personal data before the commit, and packaging decision 1 keeps them out of the crate regardless.

### 6. Security review is a disposition table, not a hunt

The review walks a fixed surface list (shell-outs, `unsafe`, filesystem writes, path traversal, scripts, dependency advisories, workflow permissions) and produces one line per surface: **fixed** or **accepted because…**. The table lands in `security-review.md` in the change directory; dependency policy lives in `deny.toml` where the tooling enforces it stays deliberate. Acceptance criteria: `cargo audit` and `cargo deny check` fully clean (the serde_norway migration removes the one known advisory), no shell-string interpolation anywhere, every `unsafe` justified in a comment, GitHub workflows request `contents: read` by default and `id-token: write` only where OIDC needs it.

### 7. Licensing: MIT, single file

The maintainer chose MIT alone. One `LICENSE` file at the root with the standard MIT text, copyright line from the maintainer's git identity; `license = "MIT"` replaces `MIT OR Apache-2.0` in the manifest; the README `## License` section matches. The rust-lang dual-license convention was considered and dropped by owner decision — simpler, and nothing downstream requires the Apache patent grant. `deny.toml`'s license allowlist still admits `Apache-2.0`, `MIT`, `Unicode-3.0` and friends, because that list governs the *dependencies'* licenses, not ours.

## Risks / Trade-offs

- [A parse difference hides in serde_norway despite the shared lineage] → the fork tracks the same libyaml transpile and API; the inline suites over frontmatter, settings, config and locale catalogues run against it in the same task that swaps the import. Any surprise fails `cargo test` before anything ships.
- [Windows CI turns red on first run] → matrix entry flips to `continue-on-error`, README states Linux/macOS support, follow-up change filed. Nothing blocks the release.
- [`openspectation` name already taken or squatted on crates.io] → checked at the start of implementation (`cargo search` / crates.io), not the end; if taken, the maintainer decides the fallback name before any file references it further.
- [A `cross` target fails in CI without a tag to test on] → the release workflow gets a `workflow_dispatch` path that builds all eight targets and skips publishing, so the matrix is provable before the first tag exists.
- [Trusted Publishing misconfigured after manual 0.1.0] → release workflow fails loudly at the publish step; binaries still attach to the GitHub Release, so a failed publish is recoverable by re-running after fixing the crates.io setting.
- [Allowlist forgets a future compile-time input] → `cargo publish --dry-run` in CI on every push to main compiles the packaged crate, catching it before tag time.
