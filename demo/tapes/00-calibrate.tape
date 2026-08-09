# Prints the terminal grid the shared settings actually produce.
#
# Cell size is derived from the font metrics by xterm.js, not from a formula
# we control, so the only honest way to size the window is to measure it.
# Target for the dashboard is roughly 110x32.

Source common.tape

Output out/00-calibrate.gif

Sleep 500ms
# Backticks delimit the string: VHS does not take escaped quotes inside one.
Type `echo "grid: $(tput cols) x $(tput lines)"`
Enter
Sleep 2s
