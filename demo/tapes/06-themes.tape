# `:theme` — a bare command cycles. Order is Palette::ALL: terminal, dark,
# light, nord, gruvbox, dracula.
#
# This tape starts on `terminal`, the default, which is the only palette that
# takes its colors from the emulator — here, the theme declared in
# common.tape. Every other palette carries fixed RGB triples and therefore
# looks identical wherever it runs.

Source common.tape

Output out/06-themes.gif
Output out/06-themes.webm

Hide
Type "cd workspace/atlas-api && clear"
Enter
Show

Type "ospc tui"
Enter
Wait+Screen /openspectation/
Sleep 2500ms

Type ":theme"
Sleep 400ms
Enter
Sleep 2200ms

Type ":theme"
Sleep 400ms
Enter
Sleep 2200ms

Type ":theme"
Sleep 400ms
Enter
Sleep 2200ms

Type ":theme"
Sleep 400ms
Enter
Sleep 2200ms

Type ":theme"
Sleep 400ms
Enter
Sleep 2200ms

Type ":theme"
Sleep 400ms
Enter
Sleep 2200ms

Type "q"
Sleep 800ms
