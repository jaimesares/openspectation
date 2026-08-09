# Tasks — prepare-crates-publication

Repository URL everywhere a URL is written: `https://github.com/jaimesares/openspectation`. No task adds a user-facing string or builds an `App` in a test; the only `src/` edits are the YAML import renames in group 2.

## 1. Preflight

- [x] 1.1 Check `openspectation` availability on crates.io (`cargo search openspectation` and https://crates.io/crates/openspectation); record the result. If taken, stop and ask the maintainer for a fallback name before touching any file that repeats it.
- [x] 1.2 Pin the current `serde_norway` release (latest 0.9.x on crates.io) and confirm it has no RustSec advisories before wiring it in.

## 2. YAML engine migration (domain layer; behaviour gate is the existing suite)

- [x] 2.1 Swap `serde_yaml = "0.9"` for `serde_norway` in `Cargo.toml`.
- [x] 2.2 Rename the fourteen `serde_yaml::` references to `serde_norway::` across `src/error.rs`, `src/domain/settings.rs`, `src/domain/spec.rs`, `src/domain/config.rs`, `src/domain/openspec.rs` and the parity test in `src/i18n.rs`; `grep -r serde_yaml src/` must come back empty.
- [x] 2.3 `cargo test` green — frontmatter, settings round-trip, workspace config and locale-parity tests are the proof the fork parses what the original parsed; `cargo clippy --all-targets` clean.

## 3. Licensing (MIT, single file)

- [x] 3.1 Add `LICENSE` with the standard MIT text, copyright line from the maintainer's git identity.
- [x] 3.2 Set `license = "MIT"` in `Cargo.toml`, replacing `MIT OR Apache-2.0`.
- [x] 3.3 Replace the README `## License` section with the MIT wording.

## 4. Crate packaging

- [x] 4.1 Add the `include` allowlist to `Cargo.toml` per design decision 1; set `repository` and `homepage` to the URL above; review `description`, `keywords` (≤5), `categories` against crates.io limits.
- [x] 4.2 Verify with `cargo package --list`: `src/`, `locales/`, `build.rs`, `README.md`, `LICENSE` and manifest files present; `.claude/`, `.opencode/`, `CLAUDE.md`, `demo/`, `scripts/`, `openspec/` absent.
- [x] 4.3 Run `cargo publish --dry-run` and confirm the packaged crate compiles (proves `locales/` and `build.rs` made it in).

## 5. Security review (disposition per surface: fixed, or accepted with reason)

- [x] 5.1 Shell-out surfaces: argv construction, PATH resolution and working directories for the `openspec` CLI (`domain/openspec.rs`), `git` (`domain/history.rs`) and `$VISUAL`/`$EDITOR` (`tui/mod.rs`); confirm no string ever reaches a shell.
- [x] 5.2 The three `unsafe` sites (`lib.rs` SIGPIPE, `env::set_var` in `domain/settings.rs` and `domain/history.rs` tests): confirm test-only where claimed and each carries a justifying comment.
- [x] 5.3 Filesystem handling: sibling-temp-plus-rename save path, `MAX_EDITABLE_BYTES` cap, walkdir symlink behaviour, settings/state/cache locations under XDG variables, task-toggle re-read (`TaskMoved`).
- [x] 5.4 Shell scripts (`scripts/install-local.sh`, `demo/*.sh`): `set -euo pipefail`, quoting, no network fetches piped to a shell, no secrets.
- [x] 5.5 Scan `.claude/`, `.opencode/`, `locales/`, `demo/` and the git history for machine-specific paths, personal data or credentials before anything is committed or pushed.
- [x] 5.6 Install and run `cargo audit`; add `deny.toml` (advisories deny, license allowlist for the dependency tree, crates-io-only sources) and run `cargo deny check`; with serde_norway in place both must pass with zero exceptions — fix or document anything new.
- [x] 5.7 Write the disposition table (surface → fixed/accepted → reason) into the change directory as `security-review.md`.

## 6. GitHub scaffolding

- [x] 6.1 Add `.github/workflows/ci.yml`: fmt check, clippy `-D warnings`, test on `{ubuntu, macos, windows}-latest` stable, a `cargo check` job pinned to Rust 1.88, and a `cargo publish --dry-run` job on pushes to main; `permissions: contents: read`.
- [x] 6.2 Add `.github/workflows/release.yml` on `v*` tags building the eight targets from design decision 4 (`cross` for non-native Linux, `--target` cross-builds on macOS/Windows), attaching per-target archives of both binaries to the GitHub Release, then `cargo publish` via crates.io Trusted Publishing (`id-token: write` on that job only); a `workflow_dispatch` path builds all targets and skips publishing.
- [x] 6.3 Add `.github/dependabot.yml` for `cargo` and `github-actions` ecosystems, weekly.
- [x] 6.4 Validate both workflows with `actionlint` (install if missing).
- [x] 6.5 Commit `.claude/` and `.opencode/` (per design decision 5, after 5.5 passes); extend `.gitignore` only if the scan surfaced local artifacts worth ignoring.

## 7. README and changelog

- [x] 7.1 Add crates.io version, CI status and license badges under the README title, pointing at `jaimesares/openspectation`.
- [x] 7.2 Add `cargo install openspectation` as the primary install path, keeping the build-from-source block; note the MSRV and the prebuilt release binaries next to it.
- [x] 7.3 Seed `CHANGELOG.md` in Keep a Changelog format with the 0.1.0 entry.
- [x] 7.4 Write `RELEASING.md`: the manual first-publish runbook (create the `jaimesares/openspectation` repo, push, first `cargo publish` from the maintainer's machine, configure Trusted Publishing on crates.io, tag `v0.1.1+` for the automated path) — documenting, not executing.
- [x] 7.5 Link pass over README and RELEASING.md against the final URL.

## 8. Final verification

- [x] 8.1 `cargo fmt`, `cargo clippy --all-targets`, `cargo test` — all green.
- [x] 8.2 Re-run `cargo package --list` and `cargo publish --dry-run` as the final packaging gate; confirm nothing from groups 6–7 leaked into the crate.
