# The non-interactive side: a colorized inventory and the requirement delta
# on stdout, which is what drops into CI.

Source common.tape

Output out/08-cli.gif
Output out/08-cli.webm

Hide
Type "cd workspace/atlas-api && clear"
Enter
Show

Sleep 800ms
Type "ospc list --all"
Sleep 500ms
Enter
Sleep 5s

Type "clear"
Enter
Sleep 300ms

Type "ospc diff add-oauth-refresh"
Sleep 500ms
Enter
Sleep 5s
