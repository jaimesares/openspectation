# `g` — the workspace as numbers. Then `L`, which swaps the interface language
# live: the catalogues are compiled into the binary, so nothing reloads.

Source common.tape

Output out/07-dashboard.gif
Output out/07-dashboard.webm

Hide
Type "cd workspace/atlas-api && clear"
Enter
Show

Type "ospc tui --theme dark"
Enter
Wait+Screen /openspectation/
Sleep 1500ms

Type "g"
Sleep 3500ms

Escape
Sleep 1200ms

# English to Spanish and back, without leaving the dashboard.
Type "L"
Sleep 2500ms
Type "g"
Sleep 3000ms
Escape
Sleep 1000ms
Type "L"
Sleep 2000ms

Type "q"
Sleep 800ms
