# The workbench: three panes, a change selected, its artifacts opened.

Source common.tape

Output out/01-tour.gif
Output out/01-tour.webm

Hide
Type "cd workspace/atlas-api && clear"
Enter
Show

Type "ospc tui --theme dark"
Enter
Wait+Screen /openspectation/
Sleep 2s

# Down the change list.
Down
Sleep 1600ms
Down
Sleep 1600ms
Up @500ms 2
Sleep 1200ms

# Into the artifact tree. A directory node summarises the change rather than
# opening a document, so pass through `specs` and `auth` without stopping.
Tab
Sleep 700ms
Down
Sleep 2400ms
Down
Sleep 2400ms
Down @280ms 3
Sleep 2400ms

# Read the delta file.
Tab
Sleep 500ms
Down @110ms 14
Sleep 1600ms

Type "q"
Sleep 600ms
