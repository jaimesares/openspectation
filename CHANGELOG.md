# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-08-10

### Added

- `install.sh`: one-line install of the prebuilt binaries on Linux and macOS,
  telling glibc and musl apart and verifying the archive's checksum.
- Every release archive now ships a `.sha256` beside it.

### Fixed

- A name carrying no letter or digit is refused instead of becoming a path.
  `openspectation init '!!!'` wrote `specs/!!!.md`, and `:new !!!` created
  `changes/!!!/`. Names written in other scripts are unaffected.
- `:new` keeps every word of the name it is given. `:new Payment Links`
  created a change called `payment` and said nothing about the rest.

## [0.1.0] - 2026-08-09

### Added

- Interactive TUI workbench: artifact tree, tabbed sidebar (changes, archive,
  specs, project), Markdown viewer with an exact scrollbar, requirement-delta
  view, interactive task list and an in-place text editor with atomic saves.
- CLI commands: `init`, `list`, `diff`, `validate`, `archive` — the last two
  delegate to the upstream OpenSpec CLI.
- Two binaries from one crate: `openspectation` and the short alias `ospc`.
- Full interface localization (English, Spanish), including runtime-translated
  clap help.
- Flat and nested (upstream CLI) workspace layouts, re-read automatically via
  size+mtime fingerprinting.
- Persisted user preferences — palette and language — under
  `$XDG_CONFIG_HOME/openspectation/settings.yaml`.
- Six palettes, defaulting to the terminal's own scheme.
- Activity timeline reconstructed from the workspace's git history.

[Unreleased]: https://github.com/jaimesares/openspectation/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/jaimesares/openspectation/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/jaimesares/openspectation/releases/tag/v0.1.0
