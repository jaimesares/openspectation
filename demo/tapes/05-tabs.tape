# The four tabs, and the filter expressions shared by `/` and `:filter`.

Source common.tape

Output out/05-tabs.gif
Output out/05-tabs.webm

Hide
Type "cd workspace/atlas-api && clear"
Enter
Show

Type "ospc tui --theme dark"
Enter
Wait+Screen /openspectation/
Sleep 1800ms

# Archive: the part of the workspace that is finished.
Type "2"
Sleep 2s
Down @600ms 2
Sleep 1200ms

# Specs: one entry per capability.
Type "3"
Sleep 2s
Down @600ms 3
Sleep 1500ms

# Project: the design document and the configuration file.
Type "4"
Sleep 2500ms

# Back to the active changes, then filter them.
Type "1"
Sleep 1500ms
Type "/"
Sleep 700ms
Type@90ms "tasks<100%"
Sleep 2500ms
Escape
Sleep 1500ms

Type "q"
Sleep 800ms
