# One pass over the whole workbench, at a brisker pace than the single-feature
# tapes: the change list, the artifact tree, the requirement delta, the task
# list, the four tabs, a command typed on the `:` line, the preferences panel
# and the metrics.
#
# The launch is hidden and there is no `q` at the end, so the recording opens
# on a painted dashboard and never leaves it. Nothing here shows the shell.
#
# No `--theme`: the default is `terminal`, which takes its colors from the
# emulator — here, the palette declared in common.tape.

Source common.tape

Output out/overview.gif
Output out/overview.webm

Hide
Type "cd workspace/atlas-api && clear"
Enter
Type "ospc tui"
Enter
Wait+Screen /openspectation/
Sleep 400ms
Show

Sleep 900ms

# The change list.
Down
Sleep 800ms
Down
Sleep 800ms
Up @350ms 2
Sleep 600ms

# The artifacts of the selected change. `specs` and `auth` are directories and
# summarise the change rather than opening a document, so pass through them.
Tab
Sleep 400ms
Down
Sleep 1100ms
Down
Sleep 1100ms
Down @180ms 3
Sleep 1100ms

# What the change does to the capability.
Type "d"
Sleep 2s

# The task list, and one item ticked off.
Type "t"
Sleep 900ms
Down @110ms 7
Sleep 400ms
Space
Sleep 1400ms

# Back to the rendered document.
Type "m"
Sleep 700ms

# Archive: the changes that already shipped.
Type "2"
Sleep 1000ms
Down @350ms 2
Sleep 700ms

# Specs: one entry per capability.
Type "3"
Sleep 1000ms
Down @350ms 3
Sleep 900ms

# Project: the configuration file.
Type "4"
Sleep 1400ms

# Back to the active changes, and a filter typed on the `:` line. At 60% the
# threshold drops exactly one of the three.
Type "1"
Sleep 700ms
Type ":"
Sleep 400ms
Type@55ms "filter tasks<60%"
Sleep 600ms
Enter
Sleep 2200ms
Escape
Sleep 800ms

# The preferences panel, which the gear in the header also opens. The theme row
# is left where it is — the whole recording is meant to stay on `terminal` —
# and the language is switched instead, which the dashboard then inherits.
Type ","
Sleep 1600ms
Down
Sleep 900ms
Right
Sleep 1800ms
Escape
Sleep 900ms

# Close on the workspace as numbers.
Type "g"
Sleep 3200ms
