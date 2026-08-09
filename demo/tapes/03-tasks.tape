# `t` — the interactive task list. `space` writes the checkbox back to
# tasks.md and the progress bar in the sidebar moves with it.

Source common.tape

Output out/03-tasks.gif
Output out/03-tasks.webm

Hide
Type "cd workspace/atlas-api && clear"
Enter
Show

Type "ospc tui --theme dark --change add-oauth-refresh"
Enter
Wait+Screen /openspectation/
Sleep 1500ms

Type "t"
Sleep 2s

# The viewer already holds the focus, and in this mode Down moves the task
# cursor rather than scrolling.
#
# The three stops are 2.5, 2.6 and 3.2, the unfinished ones: ticking work off
# drives the sidebar bar from 60% to 80%, where reopening a done task would
# send it backwards.
Down @180ms 7
Sleep 1200ms

Space
Sleep 1600ms
Down
Sleep 700ms
Space
Sleep 1800ms
Down @250ms 2
Sleep 800ms
Space
Sleep 2500ms

Type "q"
Sleep 800ms
