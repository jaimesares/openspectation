## Why

Selecting a row in the sidebar erases the information that row exists to carry. The status glyph loses its status colour and the progress bar loses the contrast between its filled and empty halves, so the selected change — the one being worked on — is the only one whose progress cannot be read.

## What Changes

- Selecting a change keeps its status glyph in the colour of its status, and keeps its progress bar readable as a bar.
- Selecting a specification document keeps its path and size visually subordinate to its title instead of flattening into it.
- The empty part of a progress bar keeps its contrast against the selection background, including in the `terminal` palette where several colour roles collapse onto the same ANSI slots.

## Capabilities

### New Capabilities

- `sidebar-list`: how the sidebar list presents changes and specification documents, including what a selected row shows.

### Modified Capabilities

None. No existing capability has a spec yet.

## Impact

- `src/tui/ui/editor.rs` — `draw_list`, `change_item`, `spec_item`
- `src/tui/theme.rs` — a `track_selected` colour role, one value per palette
- `src/tui/ui/tests.rs` — a rendering assertion on colour rather than on text alone
- No user-facing strings, so no locale changes. No new dependencies.
