//! ANSI styling for the non-interactive commands.
//!
//! Colors are produced with `crossterm` so the TUI and the plain commands share
//! a single terminal dependency. [`Painter`] is a copyable capability handle:
//! when colors are disabled every helper degrades to the raw text, which keeps
//! call sites free of conditionals.

use std::ffi::OsString;
use std::fmt::Display;
use std::io::IsTerminal;

use crossterm::style::{Attribute, Color, ContentStyle, Stylize};

/// Palette shared with the TUI theme so both surfaces look consistent.
pub mod palette {
    use crossterm::style::Color;

    pub const PRIMARY: Color = Color::Rgb {
        r: 122,
        g: 162,
        b: 247,
    };
    pub const ACCENT: Color = Color::Rgb {
        r: 187,
        g: 154,
        b: 247,
    };
    pub const SUCCESS: Color = Color::Rgb {
        r: 158,
        g: 206,
        b: 106,
    };
    pub const WARNING: Color = Color::Rgb {
        r: 224,
        g: 175,
        b: 104,
    };
    pub const DANGER: Color = Color::Rgb {
        r: 247,
        g: 118,
        b: 142,
    };
    pub const INFO: Color = Color::Rgb {
        r: 125,
        g: 207,
        b: 255,
    };
    pub const MUTED: Color = Color::Rgb {
        r: 107,
        g: 116,
        b: 136,
    };
    pub const TEXT: Color = Color::Rgb {
        r: 200,
        g: 208,
        b: 224,
    };
}

#[derive(Debug, Clone, Copy)]
pub struct Painter {
    enabled: bool,
}

impl Painter {
    /// Decides whether ANSI sequences are safe to emit.
    ///
    /// Honours `--no-color`, the `NO_COLOR` convention, `CLICOLOR_FORCE` and
    /// finally whether stdout is attached to a terminal.
    pub fn detect(disabled_by_flag: bool) -> Self {
        Self::decide(
            disabled_by_flag,
            std::env::var_os("NO_COLOR").is_some(),
            std::env::var_os("CLICOLOR_FORCE"),
            std::io::stdout().is_terminal(),
        )
    }

    /// The decision itself, with the environment passed in. Kept separate from
    /// [`Painter::detect`] because mutating process environment variables to
    /// exercise the precedence is a data race under a parallel test runner.
    fn decide(
        disabled_by_flag: bool,
        no_color: bool,
        clicolor_force: Option<OsString>,
        stdout_is_terminal: bool,
    ) -> Self {
        if disabled_by_flag || no_color {
            return Self { enabled: false };
        }
        if clicolor_force.is_some_and(|value| value != "0") {
            return Self { enabled: true };
        }
        Self {
            enabled: stdout_is_terminal,
        }
    }

    fn render(self, text: impl Display, style: ContentStyle) -> String {
        let text = text.to_string();
        if self.enabled {
            style.apply(text).to_string()
        } else {
            text
        }
    }

    fn fg(color: Color) -> ContentStyle {
        ContentStyle::new().with(color)
    }

    fn fg_bold(color: Color) -> ContentStyle {
        ContentStyle::new().with(color).attribute(Attribute::Bold)
    }

    pub fn plain(self, text: impl Display) -> String {
        text.to_string()
    }

    pub fn bold(self, text: impl Display) -> String {
        self.render(text, ContentStyle::new().attribute(Attribute::Bold))
    }

    pub fn dim(self, text: impl Display) -> String {
        self.render(text, Self::fg(palette::MUTED))
    }

    pub fn text(self, text: impl Display) -> String {
        self.render(text, Self::fg(palette::TEXT))
    }

    pub fn primary(self, text: impl Display) -> String {
        self.render(text, Self::fg(palette::PRIMARY))
    }

    pub fn accent(self, text: impl Display) -> String {
        self.render(text, Self::fg(palette::ACCENT))
    }

    pub fn accent_bold(self, text: impl Display) -> String {
        self.render(text, Self::fg_bold(palette::ACCENT))
    }

    pub fn success(self, text: impl Display) -> String {
        self.render(text, Self::fg(palette::SUCCESS))
    }

    pub fn warning(self, text: impl Display) -> String {
        self.render(text, Self::fg(palette::WARNING))
    }

    pub fn danger_bold(self, text: impl Display) -> String {
        self.render(text, Self::fg_bold(palette::DANGER))
    }

    pub fn info(self, text: impl Display) -> String {
        self.render(text, Self::fg(palette::INFO))
    }

    pub fn info_bold(self, text: impl Display) -> String {
        self.render(text, Self::fg_bold(palette::INFO))
    }

    /// Paints text with an arbitrary palette color.
    pub fn colored(self, text: impl Display, color: Color) -> String {
        self.render(text, Self::fg(color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_no_color_flag_suppresses_every_escape_sequence() {
        // `--no-color` wins regardless of the environment or of stdout being a tty.
        let painter = Painter::detect(true);
        assert_eq!(painter.danger_bold("boom"), "boom");
        assert_eq!(painter.colored("plain", palette::SUCCESS), "plain");
        assert_eq!(painter.dim("quiet"), "quiet");
    }

    const ON: Painter = Painter { enabled: true };
    const OFF: Painter = Painter { enabled: false };

    /// A styling helper addressed by name, so the tables below can iterate them.
    type Helper = (&'static str, fn(Painter, &str) -> String);

    /// Every helper, so a new one cannot be added without deciding whether it
    /// honours the disabled state.
    fn all_helpers() -> Vec<Helper> {
        vec![
            ("bold", |p, t| p.bold(t)),
            ("dim", |p, t| p.dim(t)),
            ("text", |p, t| p.text(t)),
            ("primary", |p, t| p.primary(t)),
            ("accent", |p, t| p.accent(t)),
            ("accent_bold", |p, t| p.accent_bold(t)),
            ("success", |p, t| p.success(t)),
            ("warning", |p, t| p.warning(t)),
            ("danger_bold", |p, t| p.danger_bold(t)),
            ("info", |p, t| p.info(t)),
            ("info_bold", |p, t| p.info_bold(t)),
            ("colored", |p, t| p.colored(t, palette::ACCENT)),
        ]
    }

    #[test]
    fn a_disabled_painter_returns_the_text_untouched() {
        for (name, helper) in all_helpers() {
            assert_eq!(
                helper(OFF, "hello"),
                "hello",
                "{name} styled while disabled"
            );
        }
    }

    #[test]
    fn an_enabled_painter_wraps_the_text_in_escape_sequences() {
        // crossterm closes a plain foreground with `[39m` and anything carrying
        // an attribute with the full `[0m`.
        for (name, helper) in all_helpers() {
            let painted = helper(ON, "hello");
            assert!(painted.contains("hello"), "{name} lost its text");
            assert!(painted.starts_with('\u{1b}'), "{name} emitted no escape");
            assert!(
                painted.ends_with("\u{1b}[0m") || painted.ends_with("\u{1b}[39m"),
                "{name} left the style open: {painted:?}"
            );
        }
    }

    #[test]
    fn plain_never_styles_even_when_colors_are_enabled() {
        // The escape hatch used when a value must survive being piped.
        assert_eq!(ON.plain("hello"), "hello");
        assert_eq!(OFF.plain("hello"), "hello");
        assert_eq!(ON.plain(42), "42");
    }

    #[test]
    fn bold_helpers_carry_the_bold_attribute_on_top_of_their_color() {
        for painted in [
            ON.bold("x"),
            ON.accent_bold("x"),
            ON.danger_bold("x"),
            ON.info_bold("x"),
        ] {
            assert!(painted.contains("\u{1b}[1m"), "missing bold in {painted:?}");
        }
        assert!(!ON.dim("x").contains("\u{1b}[1m"));
    }

    #[test]
    fn the_flag_beats_every_environment_signal() {
        assert!(!Painter::decide(true, false, Some("1".into()), true).enabled);
        assert!(!Painter::decide(true, true, None, true).enabled);
    }

    #[test]
    fn no_color_disables_colors_even_on_a_terminal() {
        assert!(!Painter::decide(false, true, None, true).enabled);
        // NO_COLOR is checked first, so it also beats CLICOLOR_FORCE.
        assert!(!Painter::decide(false, true, Some("1".into()), true).enabled);
    }

    #[test]
    fn clicolor_force_enables_colors_off_a_terminal_unless_it_is_zero() {
        assert!(Painter::decide(false, false, Some("1".into()), false).enabled);
        assert!(!Painter::decide(false, false, Some("0".into()), false).enabled);
        assert!(Painter::decide(false, false, Some("".into()), false).enabled);
    }

    #[test]
    fn without_any_signal_the_decision_follows_stdout() {
        assert!(Painter::decide(false, false, None, true).enabled);
        assert!(!Painter::decide(false, false, None, false).enabled);
    }
}
