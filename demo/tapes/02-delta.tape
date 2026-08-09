# `d` — the requirement delta a change declares: what it adds, modifies,
# removes and renames, with the line each one is written on.

Source common.tape

Output out/02-delta.gif
Output out/02-delta.webm

Hide
Type "cd workspace/atlas-api && clear"
Enter
Show

Type "ospc tui --theme dark --change add-oauth-refresh"
Enter
Wait+Screen /openspectation/
Sleep 1800ms

Type "d"
Sleep 3500ms

# `d` already moved the focus to the viewer, and the whole delta fits on one
# screen, so there is nothing to scroll. Walk the other changes instead: the
# mode survives a selection change as long as the new one declares deltas.
Left
Left
Sleep 700ms
Down
Sleep 3s
Down
Sleep 3500ms

# Back to the rendered document.
Type "m"
Sleep 1500ms

Type "q"
Sleep 800ms
