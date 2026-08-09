# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`openspectation` is a localized Rust CLI + TUI for OpenSpec workspaces (browse changes, read requirement deltas, tick tasks, edit documents in place, validate and archive). Edition 2024, minimum Rust 1.88.

## Commands

`cargo` is not on `PATH` in non-interactive shells here — use `~/.cargo/bin/cargo`.

```bash
cargo build --release          # binaries: target/release/{openspectation,ospc}
cargo test                     # unit tests, all inline; no tests/ directory
cargo test workspace           # single test or module by substring
cargo test --lib tui::command  # one module's tests
cargo clippy --all-targets
cargo fmt
cargo llvm-cov --summary-only  # coverage per file (cargo install cargo-llvm-cov)
./scripts/install-local.sh     # release build + install both binaries into ~/.local/bin
```

Running the tool against a workspace: `cargo run -- --path <dir> tui` (or `list`, `diff`, `validate`, `archive`, `init`).

`validate` and `archive` shell out to the upstream `openspec` CLI (`npm i -g openspec`); everything else works without it.

## Architecture

Four layers, declared in `src/lib.rs`:

- `cli.rs` — clap derive definitions + runtime help translation
- `domain/` — everything filesystem-related; **never touches the terminal**
- `commands/` — one module per subcommand, routed by `commands::dispatch`
- `tui/` — terminal lifecycle, app state, renderers

`src/main.rs` and `src/bin/ospc.rs` are both three-line shims over `openspectation::run()`; the crate is a library so the two binaries share one compilation.

### Invariants worth knowing before editing

**Every user-facing string goes through `t!`.** ~470 call sites. `locales/en.yml` and `locales/es.yml` must declare *identical* key sets — a test in `src/i18n.rs` (`catalogues_declare_the_same_keys`) fails otherwise, because `rust-i18n` would silently paper over a missing key with the fallback locale. `build.rs` reruns the build when `locales/` changes.

Adding a locale takes exactly three edits: `locales/<code>.yml`, `available-locales` in `Cargo.toml`, and `i18n::display_name`. The `i18n!` macro invocation in `lib.rs` and the `[package.metadata.i18n]` block in `Cargo.toml` must stay in sync.

**clap help is rebuilt at runtime.** clap needs `&'static str`; translations are runtime values, so `cli::localized_command()` reconstructs the derived `Command`, swapping every description via `mut_arg`/`mut_subcommand` and `leak()`. A new subcommand or flag needs both its derive definition *and* an entry in `localized_command()` plus locale keys — otherwise its help silently stays in English. The locale must be resolved before clap builds, which is why `i18n::prescan_locale()` scans raw argv for `--lang` in `run()`.

**Errors carry data, never prose.** `error::Error` has a `thiserror` `Display` that is stable English (logs/`Debug`), plus `message()`/`hint()` returning translated text and `render()` producing the colored stderr block. `exit_code()`: `2` = usage/workspace error, `1` = I/O, parse or CLI failure, `0` = success.

**Blocking work is queued, not called inline.** Validation, archiving and `$EDITOR` are pushed as `app::Pending` and executed by the event loop in `tui/mod.rs::execute_pending` *after* the announcing frame has been drawn. Key handlers must never shell out or block. Terminal teardown is unconditional, including via a chained panic hook.

**The viewer cache is keyed on identity, not position.** `App::sync_viewer` caches pre-wrapped lines under a `ViewerKey`. It holds the open document's path and the selected change's id — never list indices — because a filter can leave the selected index alone while changing what that index resolves to, which a positional key reports as a cache hit. Regression test: `filtering_to_another_change_rerenders_the_viewer`.

**Preferences are separate from workspace configuration.** `domain::settings` reads and writes `$XDG_CONFIG_HOME/openspectation/settings.yaml` (override: `OPENSPECTATION_SETTINGS`) and holds the palette, the language and the sidebar width — user-level, not committed with the repo. Loading never fails; a corrupt file yields the defaults. Precedence is flag/env > stored > default, so `TuiArgs::theme` is an `Option<Palette>` with no clap default (a default would be indistinguishable from an explicit `terminal`), and `i18n::resolve` slots the stored language between `OPENSPECTATION_LANG` and the system locale. The settings panel persists; `:theme`, `:lang` and `L` are session-only switches.

Any test that builds an `App` must hold `domain::settings::redirect_for_test`'s guard — `App::new` calls `Settings::load()`, which otherwise reads the developer's real preferences.

**The Markdown renderer pre-wraps to an exact width** (`tui/markdown.rs::render`) rather than delegating to `Paragraph::wrap`, so the line count is known exactly — that is what makes the scrollbar accurate and lets the viewer render only the visible slice. Wrapping changes there ripple into scroll math.

**The editor edits the file, not the parsed document.** `SpecDocument` carries a body with frontmatter stripped; saving that back would delete the block. `domain::editor::Buffer` loads from disk, holds `Vec<char>` per line (byte-boundary cost with accented text), saves via sibling temp file + rename, and refuses files over `MAX_EDITABLE_BYTES` (4 MiB).

**Task toggling re-reads before writing** (`domain/tasks.rs`) and errors with `TaskMoved` if the addressed line is no longer the checkbox it was collected from — an agent editing `tasks.md` between frames loses nothing.

### Workspace model

Two on-disk layouts are read (only the flat one is *written*, by `init`):

- flat: `openspec.yaml` + `specs/` + `changes/` + `design.md` at `--path`
- nested (upstream CLI): everything under `openspec/`, config named `config.yaml`, specs at `specs/<capability>/spec.md`

`Workspace::discover` falls back to `<path>/openspec` when the given directory carries no marker. `Markers::any()` — one of specs/changes/design/config — is enough to count as a workspace. In a nested layout the config's `context:` block stands in for the missing design document, and `project.md` / `AGENTS.md` are accepted as design documents.

Change proposals are either `changes/<id>/{proposal,tasks,design}.md` or a single `changes/<id>.md`; `changes/archive/` holds shipped ones. A change declares its requirement delta in `changes/<id>/specs/<cap>/spec.md` under `## ADDED|MODIFIED|REMOVED|RENAMED Requirements` — `domain/delta.rs` reads those declarations rather than diffing two file versions. Requirements themselves are counted from RFC 2119 keywords outside code fences (`domain/spec.rs`).

Workspace reloads are driven by a size+mtime fingerprint recomputed on a timer (`domain/watch.rs`) — deliberately no filesystem-notification dependency.

### Themes

Adding a palette is a `const Theme` in `src/tui/theme.rs` plus a `Palette` variant; nothing else knows palettes by name. `Palette` variants intentionally carry no doc comments — clap would print them verbatim in English under `Possible values`. `Theme::TERMINAL` (the default) uses only the 16 ANSI slots and `Reset`, so it has no distinct panel backgrounds by design.

## Tests

All tests are inline `#[cfg(test)]` modules next to the code, except `src/tui/ui/tests.rs`, which renders the real UI against ratatui's `TestBackend` and asserts on the text a terminal would show — layout regressions that hide a pane still compile. Its `Fixture` builds a nested workspace in a unique temp directory and removes it on `Drop`.

## Conventions

Generated document boilerplate (`domain/template.rs`) is always English — it becomes part of the user's repository. The detected interface language is recorded as `project.language` in `openspec.yaml` instead.

Module-level `//!` docs explain *why* a design decision was made; keep that habit rather than describing what the code plainly does.
