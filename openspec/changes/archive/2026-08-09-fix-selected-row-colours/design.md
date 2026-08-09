## Context

The sidebar list is a ratatui `List` and its selection is expressed through `highlight_style`. That style is applied *after* the item's content has been drawn: in `ratatui-widgets-0.3.2/src/list/rendering.rs` the content renders at line 104 and `buf.set_style(row_area, self.highlight_style)` runs at line 107. `Buffer::set_style` overwrites the foreground of every cell whose style names one, and `Theme::selected()` names both a foreground and a background. Every span in the selected row therefore loses its own foreground.

What that costs on screen: the status glyph stops encoding status, and the filled and empty halves of the progress bar collapse to a single colour, which reads as no bar at all. The affected rows are exactly the rows the user is working on.

The same widget applies the item's *own* style at line 91, before the content is drawn. That ordering is the whole fix.

## Goals / Non-Goals

**Goals:**

- Selection changes the background and nothing else.
- Every colour a row encodes survives being selected.
- Both row shapes are covered — changes and specification documents.

**Non-Goals:**

- Redesigning the row layout or the selection colour itself.
- The artifact tree and the task list, which already build their lines directly and are not affected.

## Decisions

**Move the selection from `highlight_style` to `ListItem::style`.**

`item_style` is applied to the row area before the content renders, so spans paint over it and keep their own foreground, while the cells no span covers keep the background — the highlight still spans the full row width. `highlight_style` is dropped entirely.

Alternatives rejected:

- `highlight_style(Style::new().bg(...))` with no foreground. Smaller, and it does fix the reported symptom, but it leaves selection expressed in two places and offers no hook for the track-contrast decision below.
- Abandoning `List` and building lines directly, as `tree_line` and `task_line` already do. Full control, but it discards `ListState`, the scroll-offset handling and the click routing that depend on it.

**Pass a `selected` flag into the item builders.**

`change_item` and `spec_item` take `selected: bool`. It is needed to choose the item style, and it is what makes the next decision possible.

**A selected row draws its progress track in a colour role of its own.**

`Theme::border` and `Theme::selection` are both near-background roles. In the `terminal` palette, which uses only the sixteen ANSI slots and `Reset`, several roles land on the same slot, so a track drawn in `border` on a `selection` background disappears.

`text_dim` was the first answer and does not work either: in `terminal` it is `Color::DarkGray`, which crossterm emits as `38;5;8` — the slot `selection` already occupies as `Color::Indexed(8)`. The two are distinct variants and one colour on screen, so the rail renders invisible in exactly the palette the requirement calls out.

`Theme` therefore carries a `track_selected` role. The five RGB palettes set it to their own `text_dim` value, which is what the first answer intended; `terminal` sets it to `Color::Gray`, slot 7 against the selection's slot 8. A test walks every palette and fails if the two land on the same slot, so the invariant survives the next palette added.

Comparing `Color` variants is not enough to see that collision, so the test normalises the sixteen named colours to their `Indexed` slot first (`theme::ansi_slot`, test-only).

Alternatives rejected:

- Moving `terminal`'s `selection` to a slot `text_dim` clears. One line and no new field, but it restyles every selected row in the default palette to fix a progress bar.
- Narrowing the requirement to palettes that keep the roles apart, on the grounds that the percentage and the `n/m` counter sit beside the bar. That trades the bar away in the default palette.

## Risks / Trade-offs

- The bold on a selected title used to come from `highlight_style`. Nothing is lost: the title span is already built with `Theme::strong()`, which carries `BOLD` whether or not the row is selected.
- Asserting on colour in a rendering test is more brittle than asserting on text. It is worth it here: this bug is invisible to any assertion that only reads characters, which is how it shipped in the first place.
