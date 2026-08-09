## 1. Sidebar list rendering

- [x] 1.1 Give `change_item` and `spec_item` a `selected: bool` parameter, and pass it from `draw_list` by comparing each visible index against the list state's selection.
- [x] 1.2 Apply the selection background through `ListItem::style` and remove the `highlight_style` call from the list.
- [x] 1.3 Draw the empty part of the progress bar in the selected-track colour when the row is selected and in `border` otherwise.
- [x] 1.4 Give `Theme` a `track_selected` role, since `text_dim` shares slot 8 with `selection` in the `terminal` palette; every palette declares a value that does not collide with its own `selection`.

## 2. Tests

- [x] 2.1 In `src/tui/ui/tests.rs`, assert that the status glyph of a selected change keeps its status colour, taking the guard from `domain::settings::redirect_for_test`.
- [x] 2.2 Assert that the filled and empty parts of a selected change's progress bar differ in colour.
- [x] 2.3 Assert that a selected specification row keeps its path visually subordinate to its title.
- [x] 2.4 Run the colour assertions under the `terminal` palette as well, since that is where the roles collapse.
- [x] 2.5 Assert in `theme.rs` that no palette draws the selected track on the slot its selection uses, comparing ANSI slots rather than `Color` variants.

## 3. Close out

- [x] 3.1 `cargo fmt`, `cargo clippy --all-targets`, `cargo test`.
