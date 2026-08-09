# Demo recordings

GIFs and videos of the real binary, recorded with [VHS](https://github.com/charmbracelet/vhs).
Nothing here is a mockup: every frame is `ospc` running against the fixture
workspace, driven by real keystrokes through a headless terminal.

## Requirements

```bash
sudo dnf5 install vhs        # pulls ttyd; ffmpeg is already a dependency
```

The tapes ask for `JetBrainsMono Nerd Font Mono`, which carries the box drawing
and the status glyphs (`● ◐ ◌ ✔ ✖ █ ░ ▌ ▾`) the dashboard is built from. Any
Nerd Font works; change `Set FontFamily` in `_common.tape` if you swap it.

## Recording

```bash
./record.sh                  # every tape into out/
./record.sh 03-tasks         # one, by stem
./record.sh 00-calibrate     # the grid check
```

`record.sh` builds `--release`, puts `target/release` on `PATH` so `Require ospc`
resolves, and reseeds the fixture before each tape — a recording that toggles a
task or saves an edit would otherwise leak into the next one. It also points
`OPENSPECTATION_SETTINGS` at a scratch file, since a tape that opens the
preferences panel writes one.

Outputs land in `out/`, two per scene: `.gif` for anywhere a GIF is expected,
and `.webm` for the web, where a looping muted video is roughly half the weight
and has none of the GIF banding.

```html
<video autoplay muted loop playsinline>
  <source src="01-tour.webm" type="video/webm">
  <img src="01-tour.gif" alt="Browsing a change in openspectation">
</video>
```

No MP4: VHS asks ffmpeg for `libx264`, which Fedora's `ffmpeg-free` does not
carry, so an `Output …mp4` line fails with `Encoder not found`. WebM covers
every current browser and the GIF covers the rest; add RPM Fusion's `ffmpeg` if
an MP4 is genuinely needed.

## Scenes

| Tape | Shows |
| ---- | ----- |
| `overview` | **The whole workbench in one pass**, on the default `terminal` palette: change list, artifact tree, delta, tasks, the four tabs, a `:` command, the preferences panel and the metrics. Opens on a painted dashboard and never leaves it — no shell, no `q`, nothing of the non-interactive side |
| `00-calibrate` | The terminal grid the shared settings produce. Not a deliverable |
| `01-tour` | Three panes, a change selected, its artifacts opened and read |
| `02-delta` | `d` — added, modified and removed requirements with their lines |
| `03-tasks` | `t` and `space` — toggling a checkbox writes `tasks.md` |
| `04-edit` | `e`, `Ctrl+S`, `Esc` — the modal editor and the dirty marker |
| `05-tabs` | The four tabs, then a `tasks<100%` filter |
| `06-themes` | `:theme` cycling all six palettes |
| `07-dashboard` | `g` for the metrics, `L` for a live language swap |
| `08-cli` | `list --all` and `diff` on stdout — the non-interactive side |

## Sizing

`Set Width` and `Set Height` are pixels, not cells: the grid falls out of the
font metrics, which xterm.js measures rather than us. `00-calibrate` prints what
came out — 1280x744 with the current settings gives **111x32**. Below
`NARROW_WIDTH` (84 columns) the panes collapse into whichever one holds the
focus, which is a feature but not what a demo should be showing.

## Writing a tape

Three things cost a re-record if you get them wrong:

- Comments are `#`. VHS has no `//`.
- `Type` takes no escaped quotes. Use backticks to delimit a string that
  contains double quotes.
- `d`, `t` and `m` move the focus to the viewer themselves (`set_viewer` in
  `src/tui/app.rs`), so a `Tab` after one of them takes the focus *away*. In
  `Tasks` mode `Down` moves the task cursor; everywhere else in the viewer it
  scrolls. A directory node in the artifact tree summarises the change instead
  of opening a document, so pass through `specs` and `<capability>` rather than
  stopping on them.

## Fixture

`seed.sh` writes `workspace/atlas-api/`, a nested workspace shaped the way the
upstream OpenSpec CLI writes one. It is generated rather than committed so the
recordings stay reproducible in two ways: content is fixed, and modification
times are set *relative to now*, which keeps the `UPDATED` column and the
dashboard's recent activity reading the same however long after seeding the
recording is made.

Both `workspace/` and `out/` are ignored by git.
