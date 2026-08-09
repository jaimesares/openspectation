//! `openspectation tui` — launches the interactive dashboard.

use std::path::Path;
use std::time::Duration;

use crate::cli::TuiArgs;
use crate::domain::settings::Settings;
use crate::domain::workspace::Workspace;
use crate::error::{Error, Result};
use crate::tui::app::App;
use crate::tui::theme::Palette;

pub fn run(path: &Path, args: &TuiArgs) -> Result<()> {
    let workspace = Workspace::open(path)?;
    let mut app = App::new(workspace).with_palette(palette(args, &Settings::load()));

    // An unknown `--change` is a typo worth reporting before taking over the
    // screen, not a silent fall back to whatever happens to be first.
    if let Some(change) = &args.change
        && !app.focus_change(change)
    {
        return Err(Error::UnknownChange(change.clone()));
    }

    crate::tui::run(app, Duration::from_millis(args.tick_rate))
}

/// Resolves the palette: the flag or its environment variable first, then the
/// stored preference, then the default.
///
/// A name the settings file no longer knows is ignored rather than reported —
/// the tool is already drawing by then, and a palette is not worth refusing to
/// start over.
fn palette(args: &TuiArgs, settings: &Settings) -> Palette {
    args.theme
        .or_else(|| settings.theme.as_deref().and_then(Palette::from_name))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(theme: Option<Palette>) -> TuiArgs {
        TuiArgs {
            tick_rate: 250,
            change: None,
            theme,
        }
    }

    #[test]
    fn the_flag_outranks_the_stored_preference() {
        let stored = Settings {
            theme: Some("nord".to_string()),
            language: None,
            sidebar_width: None,
        };
        assert_eq!(
            palette(&args(Some(Palette::Dracula)), &stored),
            Palette::Dracula
        );
        assert_eq!(palette(&args(None), &stored), Palette::Nord);
    }

    #[test]
    fn an_unknown_stored_palette_falls_back_to_the_default() {
        let stored = Settings {
            theme: Some("solarized".to_string()),
            language: None,
            sidebar_width: None,
        };
        assert_eq!(palette(&args(None), &stored), Palette::default());
        assert_eq!(
            palette(&args(None), &Settings::default()),
            Palette::default()
        );
    }
}
