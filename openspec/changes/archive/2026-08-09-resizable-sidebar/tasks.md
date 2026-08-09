## 1. Preferences

- [x] 1.1 Add an optional sidebar width to `domain::settings`, absent by default, written only once the user has set one.
- [x] 1.2 Test that a missing or unparseable value loads as absent rather than failing the load, matching how the palette and the language already behave.

## 2. Layout

- [x] 2.1 Hold the width on `App` as `Option<u16>`, seeded from the loaded settings.
- [x] 2.2 In `editor::draw`, use the stored width when present — clamped to the minimum and to the viewer's reserve — and keep the existing formula when absent.
- [x] 2.3 Record the divider's rectangle in `Regions`, widened by one cell on each side for hit testing.
- [x] 2.4 Test that an explicit width above the automatic 42-column cap is honoured, and that one below 28 is clamped.

## 3. Mouse

- [x] 3.1 Add drag state to `App`, armed by a press on the divider region and cleared by a release or by any event that is not a drag.
- [x] 3.2 Handle `MouseEventKind::Drag` by setting the width from the pointer's column.
- [x] 3.3 Return the width to automatic on a double-click of the divider.
- [x] 3.4 Persist the width when a drag ends, not on intermediate events.

## 4. Keyboard and command

- [x] 4.1 Add widen and narrow shortcuts, active only while the sidebar holds focus, and verify they do not collide with the editor's bindings.
- [x] 4.2 Add a width command accepting a number or the keyword for automatic.
- [x] 4.3 Add the command's description, its usage hint and any confirmation message to `locales/en.yml` and `locales/es.yml` within this same task.

## 5. Narrow terminals

- [x] 5.1 Confirm that below `NARROW_WIDTH` the stored width has no effect and is not overwritten, with a test that resizes below the threshold and back.

## 6. Close out

- [x] 6.1 `cargo fmt`, `cargo clippy --all-targets`, `cargo test`.
