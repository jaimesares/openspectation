# openspectation

[![crates.io](https://img.shields.io/crates/v/openspectation.svg)](https://crates.io/crates/openspectation)
[![CI](https://github.com/jaimesares/openspectation/actions/workflows/ci.yml/badge.svg)](https://github.com/jaimesares/openspectation/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A fast, fully localized terminal workbench for OpenSpec workspaces: browse a
change, open its artifacts, read the requirement delta, validate it, tick tasks
off and archive it, without leaving the terminal.

```
ospc                                 # open the workbench in the nearest workspace
ospc --change add-oauth              # open straight into one change
ospc diff add-oauth                  # requirement delta on stdout
ospc validate --path ~/work/api      # strict validation, exits non-zero on failure
ospc init "Payments API"             # scaffold a new workspace
```

`openspectation` and `ospc` are the same binary; the short name is there
because it is the one you type.

## Install

```bash
cargo install openspectation
```

Prebuilt binaries for Linux (glibc and musl), macOS and Windows — x86_64 and
aarch64 — are attached to every
[GitHub release](https://github.com/jaimesares/openspectation/releases).

Building from source requires Rust 1.88 or newer (edition 2024):

```bash
cargo build --release
install -Dm755 target/release/openspectation ~/.local/bin/openspectation
install -Dm755 target/release/ospc ~/.local/bin/ospc
```

`validate` and `archive` are delegated to the upstream
[OpenSpec CLI](https://github.com/Fission-AI/OpenSpec), which owns the
validation rules and the archive procedure. Everything else works without it.

```bash
npm install -g openspec
```

## Workspace layout

`init` writes the structure the rest of the tool understands:

```text
.
├── openspec.yaml          # workspace configuration
├── design.md              # architecture and decisions
├── specs/                 # one document per capability
│   ├── README.md
│   └── <project>.md
└── changes/               # change proposals
    ├── README.md
    └── bootstrap-<project>.md
```

A change proposal is either a single Markdown file (`changes/rename-api.md`) or
a directory (`changes/add-oauth/{proposal,tasks,design}.md`). Task checkboxes
(`- [ ]` / `- [x]`) anywhere in a proposal drive the progress reported by
`list --all` and by the dashboard. Proposals under `changes/archive/` are read
as individual shipped changes and listed after the active ones.

### Nested layouts

The upstream OpenSpec CLI keeps everything inside an `openspec/` directory
instead of at the repository root:

```text
my-project/
└── openspec/
    ├── config.yaml
    ├── specs/<capability>/spec.md
    └── changes/
        ├── <change-id>/{proposal,tasks,design}.md
        └── archive/<date>-<change-id>/
```

Pointing `--path` at either the repository or the `openspec/` directory works:
when the given directory carries no marker, the nested one is tried before
reporting a failure. Such a workspace is read as it is written by that CLI:

- `config.yaml` counts as the configuration file, but only next to a `specs/`
  or `changes/` directory — the name is too generic to trust on its own.
- Its `context:` block stands in for the missing `design.md` wherever a design
  document is reported. The **Project** tab lists the configuration file itself
  rather than that synthetic view, so what is previewed is what the editor
  writes. `project.md` and `AGENTS.md` are accepted as design documents too.
- The project is named after the repository directory, not after `openspec/`.
- A document called `spec.md` is titled after the capability directory holding
  it, and a proposal is titled by its first level-one heading — a `## Why`
  section names a section, not a change.
- A proposal that declares no status gets one from its task list: none done is
  a draft, some done is in review, all done is implemented.

Every document may open with a YAML frontmatter block:

```yaml
---
title: Invoice generation
status: draft | review | approved | implemented | deprecated
version: 1.2.0
owner: billing-team
tags: [billing, core]
---
```

Requirements are counted from RFC 2119 keywords (`MUST`, `SHALL`, `SHOULD`,
`REQUIRED`) appearing as standalone words outside code fences.

`openspec.yaml` can relocate any of the three paths:

```yaml
layout:
  specs: specs
  changes: changes
  design: design.md
conventions:
  extensions: [md, markdown, yaml, yml]
```

## Commands

### Global options

| Option | Environment | Description |
| ------ | ----------- | ----------- |
| `-p, --path <PATH>` | `OPENSPECTATION_PATH` | Workspace root, or a repository containing one in `openspec/`. Defaults to `.`. A missing path or a path pointing at a file exits with a localized error and status `2`. |
| `--lang <LOCALE>` | `OPENSPECTATION_LANG` | Forces the interface language. |
| `--no-color` | `NO_COLOR`, `CLICOLOR_FORCE` | Controls ANSI output. Colors are also disabled automatically when stdout is not a terminal. |
| | `OPENSPECTATION_SETTINGS` | Relocates the preferences file. See [Preferences](#preferences). |

### `init <NAME>`

Creates the structure above inside `--path`, using `NAME` as the project name.
Existing files are kept unless `--force` is passed. `-d/--description` is stored
in `openspec.yaml` and reused in the generated boilerplate.

`init` always writes the flat layout at the root of `--path`; it never creates a
nested `openspec/` directory. Nested layouts are read, not generated — use
`openspec init` for those.

### `list`

Prints a colorized table of everything under `specs/`.

- `--all` also prints change proposals with a progress bar per proposal.
- `--status <STATUS>` narrows both the table and the summary.
- `--sort <path|name|title|size|status|updated>`; `path` (the default) groups
  documents by directory, any other key produces a flat list showing full
  relative paths.

### `tui`

The default surface. `--tick-rate <MS>` sets the redraw interval (default
250 ms), `-c/--change <ID>` opens straight into one proposal and
`--theme <PALETTE>` picks the colors.

```text
┌ openspectation · katame · ~/Dev/katame/openspec  3 active · 5 archived  ⚙ ┐
├──────────────────────┬─────────────────────────────────────────────────────┤
│ ARTIFACTS            │ ● proposal.md                                  EDIT │
│  ▌  proposal.md      │  1 ## Why                                           │
│     tasks.md 34/39   │  2                                                  │
│   ▾ specs 1          │  3 `history.ts` is twenty-four lines that delegate  │
│     ▾ edit-history   │  4                                                  │
│         spec.md      │  5 Sixteen call sites bypass it. Every `notifyChan  │
│                      │  6                                                  │
├──────────────────────┤  7 ## What Changes                                  │
│ Changes 3  Archive … │  8                                                  │
│ ◐ Add Native Hist…   │  9 - A new `createHistory()` plugin owning a snaps  │
│   ━━━━━━━━━━  85%    │ 10 - The existing `createUndo()` and `createRedo()` │
│ ◐ Prepare Public…    │ ~                                                   │
├──────────────────────┴─────────────────────────────────────────────────────┤
│ : type a command · ? for help                                              │
│ Ctrl+S save  ·  Esc preview  ·  ↑↓←→ move                                  │
└────────────────────────────────────────────────────────────────────────────┘
```

Three panes: the **artifact tree** of whatever is selected, the **tab strip and
entry list** under it, and the **viewer**. The activity log appears under the
viewer when there is something to report.

#### Tabs

The sidebar has four, selected with `1`–`4`, with `s`, or by clicking:

| Tab | Holds |
| --- | ----- |
| **Changes** | The active proposals |
| **Archive** | Everything under `changes/archive/`, read as shipped history |
| **Specs** | One entry per capability under `specs/` |
| **Project** | The design document and the configuration file |

The archive is a tab rather than a filter: it is the part of a workspace whose
entries are finished, and separating it keeps the working list the size of the
work. The project tab exists so the design document is not wedged at the top of
the capability list, which is where it used to live.

#### Viewer

Four modes: the rendered document, the requirement delta (`d`), an interactive
task list (`t`) and a **text editor** (`e`).

`e` opens the file under the cursor for editing, in place. It is modal — every
printable key is text — so leaving is deliberate: `Esc` or `Ctrl+E` returns to
the preview and `Ctrl+S` writes the file. Unsaved work survives the trip to the
preview and back, the title carries a `●` while it is unwritten, and quitting
with an unsaved buffer asks first.

What is edited is the **file**, read fresh from disk — not the parsed document,
whose body has had its frontmatter stripped. Saving goes through a sibling
temporary file and a rename, so a crash cannot truncate what it replaces. Files
over 4 MiB are refused rather than loaded; `E` hands those to `$EDITOR`.

| Key | Action |
| --- | ------ |
| `j` `k` / `↑` `↓` | Move within the focused pane |
| `h` `l` | Fold and unfold in the tree; switch pane elsewhere |
| `←` `→` / `Tab` | Switch pane |
| `Enter` | Open the artifact, or fold the directory |
| `1` … `4` · `s` | Select a tab, or step through them |
| `e` | Edit the open document in place |
| `Ctrl+S` | Write the edited document to disk |
| `Esc` | Leave the editor for the preview |
| `E` | Open the artifact in `$EDITOR`, reload on exit |
| `d` | Requirement delta declared by the change |
| `t` | Interactive task list |
| `m` | Back to the rendered document |
| `space` | Toggle the task under the cursor, writing `tasks.md` |
| `v` | `openspec validate --strict` on the selected change |
| `a` | Archive the change, after confirming |
| `g` | Workspace metrics; `Esc` returns |
| `/` | Filter the list |
| `:` | Command line |
| `r` | Re-read the workspace |
| `L` | Cycle the interface language live |
| `,`, `F2` | Preferences |
| `?`, `F1` | Keyboard shortcuts |
| `q`, `Ctrl+c` | Quit |

Outside the editor, `Esc` unwinds one step at a time: overlay, viewer mode,
filter, activity log, then quit.

#### Preferences

The `⚙` in the header opens them, as do `,`, `F2` and `:settings`. Two rows —
the palette and the interface language — moved with `↑` `↓` and changed with
`←` `→`, `Enter` or a click on the value.

There is no save button. A change applies to the running dashboard and is
written out in the same keystroke, so the panel holds no state of its own that
could get out of step with the file:

```text
$XDG_CONFIG_HOME/openspectation/settings.yaml     # ~/.config/… when unset
```

```yaml
theme: nord
language: es
```

Preferences are the user's, not the workspace's, which is why they do not go
into `openspec.yaml` — a file the whole team commits. `OPENSPECTATION_SETTINGS`
relocates the file wholesale, and a file that cannot be read or parsed costs a
preference, never the session.

A single invocation still outranks what is stored: `--theme` and its
`OPENSPECTATION_THEME` win over `theme:`, and `--lang`, `OPENSPECTATION_LANG`
win over `language:`, which in turn beats the system locale. `:theme`, `:lang`
and `L` change the running session only — they are switches, not settings.

#### Themes

`--theme <PALETTE>`, `OPENSPECTATION_THEME`, the preferences panel, or
`:theme <name>` inside the dashboard. A bare `:theme` cycles to the next one, in
the order below.

| Palette | Colors |
| ------- | ------ |
| `terminal` *(default)* | The 16 ANSI slots and the terminal's own default foreground and background, so the dashboard follows whatever scheme the emulator is configured with — a transparent background included |
| `dark` | Fixed blue-violet palette on near-black |
| `light` | Fixed palette for a bright background; every foreground clears WCAG AA against it |
| `nord` | Nord |
| `gruvbox` | Gruvbox, dark medium |
| `dracula` | Dracula |

`terminal` is the default because a fixed palette on an unknown background is
the one combination that can turn unreadable. It is also the only one that
gives up the distinct panel backgrounds: ANSI has no slot for "the background,
slightly lighter", so the panes are separated by their borders instead. Pick a
named palette when that hierarchy matters more than matching the terminal.

```console
$ ospc tui --theme gruvbox
$ OPENSPECTATION_THEME=nord ospc tui
```

Adding one is a `const` in [`src/tui/theme.rs`](src/tui/theme.rs) plus its
`Palette` variant; nothing else knows the palettes by name.

#### Mouse

Clicking a tab selects it, clicking an entry selects it, and clicking a
directory in the tree folds it. Clicking a task toggles it, and clicking inside
the editor puts the cursor there. The wheel moves the selection over a list and
scrolls the viewer.

#### Command line

| Command | Action |
| ------- | ------ |
| `:validate [--loose] [change]` | Validate, strict unless opted out |
| `:archive [--skip-specs] [change]` | Archive, after confirming |
| `:new <change-id>` | Scaffold `proposal.md`, `tasks.md`, `design.md` |
| `:filter <expression>` | See below |
| `:edit` · `:w` | Edit the open document · write it |
| `:external [path]` | Hand a file to `$EDITOR` |
| `:diff` · `:tasks` | Switch the viewer mode |
| `:changes` · `:archived` · `:specs` · `:project` | Select a tab |
| `:theme [name]` | Switch palette; without a name, cycle to the next |
| `:settings` | Open the preferences panel |
| `:dashboard` · `:reload` · `:lang [code]` · `:clear` · `:quit` | |

Filter expressions are shared by `/` and `:filter`: plain text matches titles,
paths, tags and status, while `tasks<100%`, `tasks>=50`, `status:draft`,
`archived` and `active` are understood as predicates. The filter applies within
the active tab.

#### Watching

The workspace is re-read whenever its files change, by fingerprinting sizes and
modification times about once a second. Nothing needs to be pressed when an
agent writes a change into the tree.

### `diff [CHANGE]`

Prints the requirement delta a change declares — which requirements it adds,
modifies, removes or renames, with the line each one is written on. Defaults to
every active change.

### `validate [CHANGE]`

Runs `openspec validate --strict --json` and renders the findings. Exits `1`
when anything fails, so it drops into CI. `--loose` relaxes the rule set.

### `archive <CHANGE>`

Runs `openspec archive`, asking first unless `-y/--yes` is given.
`--skip-specs` leaves the specifications untouched.

## Localization

Translations live in `locales/en.yml` and `locales/es.yml` and are compiled into
the binary by [`rust-i18n`](https://crates.io/crates/rust-i18n).

The active locale is resolved in this order:

1. `--lang`
2. `OPENSPECTATION_LANG`
3. `LC_ALL`, `LC_MESSAGES`, `LANG`, `LANGUAGE`
4. `en`

Region subtags degrade gracefully: `es_ES.UTF-8` resolves to `es`, and an
unknown language falls back to English.

### Adding a language

1. Copy `locales/en.yml` to `locales/<code>.yml` and translate the values.
2. Add `<code>` to `available-locales` in `Cargo.toml`.
3. Add a display name to `i18n::display_name` in `src/i18n.rs`.

Nothing else needs touching: the locale list, the `--lang` help and the `L`
key all read from the compiled catalogue.

The help output is rebuilt at runtime with translated strings, including the
section headings. Two things stay in English: clap's *parse-error* messages
(`invalid value 'x' for '--status'`), which clap does not expose for
translation, and generated document boilerplate, which becomes part of your
repository. Output from the upstream `openspec` CLI is shown verbatim, in
whatever language that tool speaks.

## Architecture

```text
src/
├── lib.rs           # locale bootstrap, argument parsing, exit codes
├── main.rs          # the `openspectation` binary
├── bin/ospc.rs      # the `ospc` binary, same entry point
├── cli.rs           # clap definitions + runtime help translation
├── i18n.rs          # locale detection, activation, cycling
├── error.rs         # thiserror types with localized rendering
├── paint.rs         # ANSI styling for the non-interactive commands
├── commands/        # init, list, tui, diff, validate, archive
├── domain/          # workspace model
│   ├── config · spec · change · scanner · workspace   # what is on disk
│   ├── delta · tasks · filter                         # what it means
│   ├── editor                                         # the text buffer
│   ├── settings                                       # user preferences
│   ├── openspec                                       # the upstream CLI
│   └── watch                                          # change detection
└── tui/             # terminal lifecycle, state, tree, theme, renderers, views
```

`domain` never touches the terminal, so the CLI renderer and the workbench
share one model. The Markdown renderer pre-wraps to an exact width, which is
what makes the viewer scrollbar accurate and lets it render only the visible
slice of a document.

Blocking work — validation, archiving, `$EDITOR` — is queued by the application
and executed by the event loop *after* the frame announcing it has been drawn,
so the state machine never owns the terminal and the user always sees what the
tool is about to do.

Exit codes: `0` success, `1` I/O or parse failure, `2` usage or workspace error.

## Development

```bash
cargo test                 # unit tests, including TUI renders against TestBackend
cargo clippy --all-targets
cargo fmt
```

## License

This project is licensed under the [MIT License](LICENSE).
