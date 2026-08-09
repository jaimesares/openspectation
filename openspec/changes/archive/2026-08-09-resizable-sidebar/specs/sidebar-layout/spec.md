## Purpose

The sidebar holds the artifact tree and the change list, and it competes for horizontal room with the document viewer. How that room is divided decides whether change titles and file paths can be read at all, and the answer differs between workspaces and between tasks, so the division is placed under the user's control instead of being fixed.

## ADDED Requirements

### Requirement: The sidebar width is adjustable

The user SHALL be able to set the width of the sidebar. The interface SHALL offer that control by direct manipulation, by keyboard, and by command, so it is reachable whether or not a mouse is in use.

#### Scenario: dragging the divider

- **WHEN** the user presses the primary mouse button on the divider between the sidebar and the viewer and moves the pointer horizontally
- **THEN** the sidebar follows the pointer until the button is released

#### Scenario: adjusting from the keyboard

- **WHEN** the sidebar holds keyboard focus and the user presses the widen or the narrow shortcut
- **THEN** the sidebar changes width by one column in that direction

#### Scenario: setting a width by command

- **WHEN** the user runs the width command with a number
- **THEN** the sidebar takes that width, subject to the limits below

### Requirement: Width limits

The sidebar SHALL never be narrower than 28 columns, and SHALL always leave at least 40 columns to the viewer. A request outside those limits SHALL be clamped rather than refused.

#### Scenario: narrowing past the minimum

- **WHEN** the user drags the divider left beyond the minimum
- **THEN** the sidebar stops at the minimum and further movement in that direction has no effect

#### Scenario: widening past the viewer's reserve

- **WHEN** the user asks for a width that would leave the viewer less than its reserve
- **THEN** the sidebar takes the widest width that still honours the reserve

### Requirement: Automatic width remains available

The sidebar SHALL size itself automatically until the user sets a width, and the user SHALL be able to return it to doing so.

#### Scenario: a user who has never adjusted it

- **WHEN** a user with no stored width opens the interface
- **THEN** the sidebar is sized from the terminal width exactly as it was before this capability existed

#### Scenario: returning to automatic

- **WHEN** the user double-clicks the divider, or runs the width command with the keyword that means automatic
- **THEN** the sidebar resumes sizing itself from the terminal width

### Requirement: The width is remembered

A width the user set SHALL survive closing and reopening the interface, and SHALL be stored with the user's own preferences rather than with the workspace.

#### Scenario: reopening the interface

- **WHEN** the user sets a width, closes the interface, and opens it again on the same terminal
- **THEN** the sidebar has the width that was set

#### Scenario: a terminal too narrow for the stored width

- **WHEN** the stored width cannot be honoured because the terminal is narrower than it was when the width was chosen
- **THEN** the sidebar takes the widest width the current terminal allows, and the stored preference is left unchanged, so returning to a wider terminal restores it

#### Scenario: a workspace opened by someone else

- **WHEN** a second user opens the same workspace from version control
- **THEN** their sidebar width is their own, because no width was committed with the workspace

### Requirement: Narrow terminals are unaffected

Below the width at which the interface can show three columns, it already gives the whole terminal to the focused pane. In that state the sidebar width SHALL have no effect, and the stored preference SHALL NOT be overwritten.

#### Scenario: shrinking below the threshold and back

- **WHEN** the terminal is resized below the three-column threshold and then back above it
- **THEN** the sidebar returns to the width it had before
