# Security review — disposition table

Review date: 2026-08-09. Scope: every file in the tree, the dependency graph
and the CI/release automation added by this change. Disposition is either
**fixed** (changed in this change) or **accepted** (reviewed, left as is, with
the reason).

| Surface | Disposition | Reason |
| --- | --- | --- |
| `openspec` CLI shell-out (`domain/openspec.rs`) | accepted | `Command::new` with argv arrays — no shell, no string interpolation. Program resolved from `PATH` by name, cwd pinned to the workspace root, `NO_COLOR=1` forced. |
| `git` shell-out (`domain/history.rs`) | accepted | Argv arrays with pinned format flags (`-c diff.noprefix=false`, `--no-ext-diff`, `--no-color`, `--no-renames`), `GIT_OPTIONAL_LOCKS=0`. Runtime code never mutates `PATH`; the test helper that does is `#[cfg(test)]` and mutex-serialized. |
| `$VISUAL` / `$EDITOR` spawn (`tui/mod.rs`) | accepted | The user's own configured editor: value split on whitespace and spawned directly, never through a shell. Same trust model as every CLI that honours `EDITOR`. |
| `unsafe`: SIGPIPE restorer (`lib.rs`) | accepted | Installs the default handler for a standard signal; touches no Rust state. Carries a justifying comment. |
| `unsafe`: `env::set_var` ×2 (`domain/settings.rs`, `domain/history.rs`) | fixed | Both are `#[cfg(test)]` and serialized under the `TEST_ENV` mutex. Justifying comments were missing — added by this change. |
| Editor file writes (`domain/editor.rs`, `domain/util.rs`) | accepted | `write_atomic`: sibling temp file + rename, so a crash cannot truncate the target. 4 MiB `MAX_EDITABLE_BYTES` cap refuses oversized files. |
| Task toggling (`domain/tasks.rs`) | accepted | Re-reads the file before writing and errors with `TaskMoved` when the addressed line changed — no blind write over concurrent edits. |
| Directory traversal (`domain/scanner.rs`, `domain/watch.rs`) | accepted | `WalkDir` with `follow_links(false)` in both; roots come from the user's own `--path`. |
| Settings/state/cache files (`domain/settings.rs`, `domain/history.rs`) | accepted | XDG locations with env overrides; a corrupt file costs a preference, never the session; no secrets stored. |
| Shell scripts (`scripts/install-local.sh`, `demo/*.sh`) | accepted | All `set -euo pipefail`, quoted expansions, no network fetches piped to a shell, no credentials. "Token" strings in `demo/seed.sh` are fictional demo-workspace content. |
| Repository contents and git history | accepted | Scanned for credentials, keys and machine paths: none. Only git author metadata (name, email) appears — inherent to publishing a repository. `.claude/` and `.opencode/` contain only generated OpenSpec commands and skills. |
| Dependency advisories | fixed | `serde_yaml` (archived, RustSec unmaintained) replaced with `serde_norway` 0.9.42. `cargo audit`: 0 advisories across 236 locked dependencies. |
| Dependency licenses and sources | accepted | `deny.toml`: allowlist MIT, Apache-2.0, Unicode-3.0, BSD-3-Clause (encoding_rs), Zlib (foldhash); crates.io-only sources; yanked = deny. `cargo deny check`: advisories, bans, licenses, sources all ok. |
| GitHub workflow permissions | accepted | `contents: read` at workflow level; `contents: write` only on the release-asset job; `id-token: write` only on the crates.io publish job. No stored publish token — OIDC Trusted Publishing. `actionlint` clean. |
| Crate packaging | fixed | `include` allowlist (anchored patterns) ships 51 files: `src/`, `locales/`, `build.rs`, `README.md`, `LICENSE`, manifests. Planning trees, agent configs, demo toolkit and scripts excluded. |
