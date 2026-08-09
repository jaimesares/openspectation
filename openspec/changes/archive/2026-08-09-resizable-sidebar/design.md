## Context

`editor::draw` computes `sidebar_width` as `(area.width / 3).clamp(28, SIDEBAR_MAX)`, with `SIDEBAR_MAX` at 42, and splits the area horizontally from it. Below `NARROW_WIDTH` the three-column layout is abandoned entirely and the focused pane takes the terminal.

Mouse capture is already enabled and crossterm already delivers `MouseEventKind::Drag`, but `App::on_mouse` matches only the wheel and a left press, so every drag event is currently discarded. `domain::settings` already persists user-level preferences — the palette and the language — and already survives a corrupt file by falling back to defaults.

## Goals / Non-Goals

**Goals:**

- The width is under the user's control through three routes, none of which requires the other two.
- The choice persists, and can be undone.
- Nothing changes for a user who never touches it.

**Non-Goals:**

- The horizontal divider between the artifact tree and the change list. That split stays automatic; this change is about the sidebar against the viewer.
- A row in the preferences panel. The panel is for choices with no handle on screen; a width that can be dragged has one.
- Per-workspace widths. The width is a property of the person and their terminal, not of the repository.

## Decisions

**`Option<u16>`, not `u16`.**

`None` means automatic and keeps the existing formula. `Some(n)` is a width the user asked for. A plain `u16` carrying a default cannot be distinguished from a chosen width, which makes "return to automatic" unimplementable and freezes the automatic formula for anyone who has ever opened the interface. This is the same reasoning that makes `TuiArgs::theme` an `Option<Palette>` with no clap default.

**The upper bound means something different once the width is explicit.**

The automatic width keeps its 42-column cap, which exists so the sidebar does not swallow a wide terminal by default. An explicit width is bounded instead by the viewer's reserve — `n.clamp(28, area.width.saturating_sub(VIEWER_RESERVE))`. Keeping 42 as a hard ceiling would make the divider stop dead a third of the way across a wide terminal, which reads as a broken drag rather than as a limit.

**Clamping happens at render time and is never written back.**

The stored value is what the user asked for, not what last fitted. A terminal too narrow today clamps what is drawn and leaves the preference alone, so moving between a laptop screen and an external monitor does not silently shrink the setting.

**Columns are stored, not a percentage.**

A percentage transfers between screen sizes more gracefully, but what the user is actually choosing is "enough room for my change titles", and that is a column count. A percentage would re-truncate the same titles on every different screen, which is the problem this change exists to remove.

**The drag target is the divider column widened by one cell on each side.**

A one-cell hit area is accurate and unpleasant to hit. Widening the hit test costs nothing on screen because it changes what is tested, not what is drawn.

**Persist when the drag ends, not while it runs.**

A drag produces an event per column crossed. Writing the settings file on each one would turn a gesture into dozens of file writes.

## Risks / Trade-offs

- This introduces the first stateful mouse interaction in the application. A release event that never arrives would leave the divider stuck to the pointer, so the drag state is cleared by any event that is not a drag, not only by a release.
- Two people sharing a repository will see different sidebars. That is the intent, and it matches where the palette and the language already live.
- Two key combinations are consumed. They are scoped to the sidebar holding focus so they cannot collide with the editor's bindings.
- The width is stored in columns and terminals vary, so the first frame on an unfamiliar terminal may be clamped. The alternative — storing a proportion — was rejected above for a worse failure mode.
