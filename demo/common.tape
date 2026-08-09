# Shared recording settings. Sourced by every tape; holds no Output directive
# of its own so each tape decides what it writes.
#
# The palette mirrors Theme::DARK from src/tui/theme.rs. Only `background`
# matters while recording with `--theme dark`, since that palette declares its
# own RGB triples — the ANSI slots are here for the `terminal` palette, which
# is precisely the one that renders with whatever the emulator provides.

Require ospc

Set Shell bash
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set FontSize 16
Set Width 1280
Set Height 744
Set Padding 24
Set Framerate 24
Set TypingSpeed 55ms
Set CursorBlink false

Set Theme { "background": "#11131a", "foreground": "#c8d0e0", "cursor": "#7aa2f7", "selection": "#232838", "black": "#15161e", "red": "#f7768e", "green": "#9ece6a", "yellow": "#e0af68", "blue": "#7aa2f7", "magenta": "#bb9af7", "cyan": "#7dcfff", "white": "#a9b1d6", "brightBlack": "#414868", "brightRed": "#f7768e", "brightGreen": "#9ece6a", "brightYellow": "#e0af68", "brightBlue": "#7aa2f7", "brightMagenta": "#bb9af7", "brightCyan": "#7dcfff", "brightWhite": "#c0caf5" }
