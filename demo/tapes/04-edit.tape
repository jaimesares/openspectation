# `e` — the modal text editor. Every printable key is text, the title carries
# a `●` while the buffer is unwritten, `Ctrl+S` saves through a temporary file
# and a rename, `Esc` returns to the preview with the buffer intact.

Source common.tape

Output out/04-edit.gif
Output out/04-edit.webm

Hide
Type "cd workspace/atlas-api && clear"
Enter
Show

Type "ospc tui --theme dark --change add-oauth-refresh"
Enter
Wait+Screen /openspectation/
Sleep 1800ms

Type "e"
Sleep 1800ms

# The open document is design.md, whose line 12 is the blank one after the
# Storage paragraph. Landing on a blank line is what keeps the typed sentence
# from being spliced into the middle of an existing one.
Down @90ms 11
Sleep 1200ms
Type@45ms "Hashing costs nothing here: the token is compared, never read back."
Sleep 2s

# The title is dirty until this.
Ctrl+S
Sleep 2s

Escape
Sleep 2s

Type "q"
Sleep 800ms
