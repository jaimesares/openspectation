## Why

The sidebar's width is derived from the terminal width and capped at 42 columns. Change titles and artifact paths are routinely longer than that, so they are truncated with no recourse, while the viewer keeps space the user may not want it to have. The right balance between the two depends on the workspace and on the task, which makes it the user's call rather than a constant.

## What Changes

- The sidebar's width becomes adjustable by dragging the divider between it and the viewer, by keyboard while the sidebar has focus, and by a command.
- A width the user sets is remembered between sessions, alongside the palette and the language.
- The user can return the sidebar to sizing itself automatically.
- The minimum width stays where it is today, and the viewer keeps a guaranteed minimum of its own.
- The 42-column ceiling stays for the automatic width and stops applying to a width the user asked for.

## Capabilities

### New Capabilities

- `sidebar-layout`: how wide the sidebar is, how the user changes it, and what is remembered.

### Modified Capabilities

None. No existing capability has a spec yet.

## Impact

- `src/tui/ui/editor.rs` — `draw`
- `src/tui/app.rs` — `Regions`, `on_mouse`, key handling
- `src/tui/command.rs` — a width command
- `src/domain/settings.rs` — an optional stored width
- New user-facing strings, so both `locales/en.yml` and `locales/es.yml` move together
- No new dependencies: mouse capture is already enabled and crossterm already delivers drag events, which the application currently discards
