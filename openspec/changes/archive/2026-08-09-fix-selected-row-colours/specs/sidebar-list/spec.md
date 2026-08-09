## Purpose

The sidebar list is how a user picks which change or document to work on, and it doubles as the at-a-glance status of the workspace: every row carries a status glyph, and a change also carries a progress bar and a task counter. A row that is selected is the row the user is acting on, so it is the row whose information matters most.

## ADDED Requirements

### Requirement: Selection is a background, not a repaint

A selected row SHALL remain legible in every element it carries. Selection SHALL be conveyed through the row's background and SHALL NOT replace the foreground colour of the row's contents.

#### Scenario: status glyph of a selected change

- **WHEN** a change in the sidebar list is selected
- **THEN** its status glyph is drawn in the colour corresponding to its status, the same colour it carries when the row is not selected

#### Scenario: progress bar of a selected change

- **WHEN** a change with some but not all of its tasks completed is selected
- **THEN** the filled part of its progress bar and the empty part are drawn in different colours, so the completion ratio stays readable

#### Scenario: secondary text of a selected document

- **WHEN** a specification document row is selected
- **THEN** its path and size stay visually subordinate to its title rather than sharing the title's colour

### Requirement: Progress track contrast under selection

The empty part of a progress bar SHALL keep a visible contrast against the background it is drawn on, including the selection background, in every palette.

#### Scenario: a palette that reuses colour roles

- **WHEN** the `terminal` palette is active, which maps several roles onto the same sixteen ANSI slots, and a change row is selected
- **THEN** the empty part of its progress bar is still distinguishable from the selection background
