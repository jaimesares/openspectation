//! Terminal lifecycle and the interactive event loop.
//!
//! Raw mode, the alternate screen and mouse capture are all installed here and
//! unconditionally torn down again — including on panic, through a hook chained
//! in front of the previous one.
//!
//! Anything that blocks — a child process, an editor — is queued by the
//! application and executed here, after the frame announcing it has been drawn.
//! That keeps the state machine free of terminal ownership and means the user
//! always sees what the tool is about to do before it stops responding.

pub mod app;
pub mod command;
pub mod diffview;
pub mod markdown;
pub mod theme;
pub mod tree;
pub mod ui;

use std::io::{Stdout, stdout};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use rust_i18n::t;

use crate::domain::openspec;
use crate::error::Result;
use crate::tui::app::{App, Pending, Tone};

type Backend = CrosstermBackend<Stdout>;

/// Editors used when neither `VISUAL` nor `EDITOR` is set.
const EDITOR_FALLBACKS: [&str; 3] = ["nano", "vi", "vim"];

/// Runs the dashboard until the user quits, restoring the terminal afterwards.
pub fn run(mut app: App, tick_rate: Duration) -> Result<()> {
    let mut terminal = setup()?;
    let outcome = event_loop(&mut terminal, &mut app, tick_rate);

    // Restoration must happen even when the loop failed, and its own failure
    // must not mask the original error.
    let restored = restore();
    outcome.and(restored)
}

fn setup() -> Result<Terminal<Backend>> {
    install_panic_hook();

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture, cursor::Hide)?;

    let mut terminal = Terminal::new(CrosstermBackend::new(out))?;
    terminal.clear()?;

    Ok(terminal)
}

fn restore() -> Result<()> {
    let mut out = stdout();
    execute!(out, DisableMouseCapture, LeaveAlternateScreen, cursor::Show)?;
    disable_raw_mode()?;
    Ok(())
}

/// Chains a hook that leaves the alternate screen before the panic is printed,
/// so the backtrace is readable instead of being swallowed by raw mode.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        previous(info);
    }));
}

fn event_loop(terminal: &mut Terminal<Backend>, app: &mut App, tick_rate: Duration) -> Result<()> {
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        // Queued work runs between two frames: the one just drawn told the
        // user what is happening, and the next one will show the result.
        if let Some(pending) = app.take_pending() {
            execute_pending(terminal, app, pending)?;
            continue;
        }

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            match event::read()? {
                // `KeyEventKind` filtering keeps Windows from firing twice.
                Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key)?,
                Event::Mouse(mouse) => app.on_mouse(mouse),
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.on_tick();
            last_tick = Instant::now();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn execute_pending(
    terminal: &mut Terminal<Backend>,
    app: &mut App,
    pending: Pending,
) -> Result<()> {
    match pending {
        Pending::Validate { strict, target } => {
            match openspec::validate(&app.workspace.root, target.as_deref(), strict) {
                Ok(report) => app.report_validation(&report),
                Err(error) => app.report_failure(&error),
            }
        }
        Pending::Archive { skip_specs, target } => {
            match openspec::archive(&app.workspace.root, &target, skip_specs) {
                Ok(lines) => app.report_archive(&lines),
                Err(error) => app.report_failure(&error),
            }
        }
        Pending::ScanHistory => {
            let recovered = app.scan_history();
            app.report_history(recovered);
        }
        Pending::Editor(path) => {
            let outcome = with_suspended_terminal(terminal, || spawn_editor(&path));
            match outcome {
                Ok(true) => app.after_editor(&path),
                Ok(false) => app.say(format!("  {}", t!("tui.log.editor_failed")), Tone::Warning),
                Err(error) => return Err(error),
            }
        }
    }

    Ok(())
}

/// Hands the terminal back to a child process and takes it again afterwards.
fn with_suspended_terminal<T>(
    terminal: &mut Terminal<Backend>,
    action: impl FnOnce() -> T,
) -> Result<T> {
    restore()?;
    let outcome = action();

    enable_raw_mode()?;
    execute!(
        stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        cursor::Hide
    )?;
    // The child left the screen in an unknown state; a full redraw is the only
    // honest way back.
    terminal.clear()?;

    Ok(outcome)
}

/// Runs the user's editor on a file, reporting whether it exited cleanly.
fn spawn_editor(path: &Path) -> bool {
    let Some(program) = editor_command() else {
        return false;
    };

    // `VISUAL="code --wait"` is common enough that the value is split rather
    // than executed as a single program name.
    let mut words = program.split_whitespace();
    let Some(binary) = words.next() else {
        return false;
    };

    Command::new(binary)
        .args(words)
        .arg(path)
        .status()
        .is_ok_and(|status| status.success())
}

fn editor_command() -> Option<String> {
    for variable in ["VISUAL", "EDITOR"] {
        if let Ok(value) = std::env::var(variable)
            && !value.trim().is_empty()
        {
            return Some(value);
        }
    }

    EDITOR_FALLBACKS
        .iter()
        .find(|candidate| which(candidate))
        .map(|candidate| (*candidate).to_string())
}

/// Whether a program exists on `PATH`.
fn which(program: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
}
