//! `openspectation` — a localized CLI and TUI to manage OpenSpec workspaces.
//!
//! The crate is organised in four layers:
//!
//! * [`cli`] turns process arguments into a typed [`cli::Cli`] value and rebuilds
//!   clap's help output with translated strings.
//! * [`domain`] owns everything filesystem related: workspace discovery, spec
//!   parsing and the derived statistics.
//! * [`commands`] implements the user facing behaviour of each subcommand.
//! * [`tui`] renders the interactive dashboard on top of Ratatui.
//!
//! It is a library so the two binaries — `openspectation` and its short alias
//! `ospc` — can share one compilation instead of building the same sources
//! twice.

pub mod cli;
pub mod commands;
pub mod domain;
pub mod error;
pub mod i18n;
pub mod paint;
pub mod tui;

use std::process::ExitCode;

use clap::FromArgMatches;

use crate::cli::Cli;
use crate::paint::Painter;

// Embeds `locales/*.yml` into the binary and generates the `t!` backend.
rust_i18n::i18n!("locales", fallback = "en");

/// Parses the command line and runs the requested subcommand.
pub fn run() -> ExitCode {
    restore_sigpipe();

    // The locale has to be known before clap builds its help text, so the raw
    // arguments are scanned once for `--lang` before the real parse happens.
    i18n::activate(i18n::prescan_locale().as_deref());

    let matches = match cli::localized_command().try_get_matches() {
        Ok(matches) => matches,
        Err(report) => {
            let _ = report.print();
            return ExitCode::from(u8::try_from(report.exit_code()).unwrap_or(2));
        }
    };

    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(report) => {
            let _ = report.print();
            return ExitCode::from(u8::try_from(report.exit_code()).unwrap_or(2));
        }
    };

    // `--lang` may also arrive through the environment, which the pre-scan does
    // not cover in every shape, so the locale is reconciled after parsing.
    i18n::activate(cli.lang.as_deref());

    let painter = Painter::detect(cli.no_color);

    match commands::dispatch(&cli, painter) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.render(painter));
            ExitCode::from(error.exit_code())
        }
    }
}

/// Restores the default disposition of `SIGPIPE`.
///
/// The Rust runtime ignores it, which turns `openspectation list | head` into a
/// panic on a broken pipe instead of the quiet exit every other CLI performs.
#[cfg(unix)]
fn restore_sigpipe() {
    const SIGPIPE: i32 = 13;
    const SIG_DFL: usize = 0;

    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }

    // Safe: installing the default handler for a standard signal touches no
    // Rust state and cannot fail in a way that matters here.
    unsafe {
        signal(SIGPIPE, SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_sigpipe() {}
