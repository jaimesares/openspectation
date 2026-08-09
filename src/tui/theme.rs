//! Color palettes and reusable styles for the interactive dashboard.
//!
//! [`Theme::TERMINAL`] is the default: it sticks to the 16 ANSI slots and
//! `Reset`, so the dashboard inherits whichever color scheme the emulator is
//! configured with — including a transparent background. Every other palette
//! declares explicit RGB triples instead and therefore looks identical
//! everywhere, at the cost of ignoring the terminal's own colors.
//!
//! [`Palette`] is the named index over them, shared by `--theme` and the
//! `:theme` command line.

use clap::ValueEnum;
use ratatui::style::{Color, Modifier, Style};

use crate::domain::spec::SpecStatus;

/// The palettes a user can pick by name.
///
/// The variants carry no doc comments on purpose: clap would print them
/// verbatim under `Possible values`, in English, next to help text this crate
/// otherwise translates. The names alone read the same in every language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum Palette {
    #[default]
    Terminal,
    Dark,
    Light,
    Nord,
    Gruvbox,
    Dracula,
}

impl Palette {
    /// Every palette, in presentation order.
    pub const ALL: [Self; 6] = [
        Self::Terminal,
        Self::Dark,
        Self::Light,
        Self::Nord,
        Self::Gruvbox,
        Self::Dracula,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Nord => "nord",
            Self::Gruvbox => "gruvbox",
            Self::Dracula => "dracula",
        }
    }

    /// Looks a palette up by name, case insensitively.
    pub fn from_name(name: &str) -> Option<Self> {
        let wanted = name.trim().to_lowercase();
        Self::ALL
            .into_iter()
            .find(|palette| palette.name() == wanted)
    }

    /// The next palette, wrapping around — what a bare `:theme` asks for.
    pub const fn next(self) -> Self {
        let index = self as usize;
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// Every name, comma separated, for help and error messages.
    pub fn names() -> String {
        Self::ALL.map(Self::name).join(", ")
    }

    pub const fn theme(self) -> Theme {
        match self {
            Self::Terminal => Theme::TERMINAL,
            Self::Dark => Theme::DARK,
            Self::Light => Theme::LIGHT,
            Self::Nord => Theme::NORD,
            Self::Gruvbox => Theme::GRUVBOX,
            Self::Dracula => Theme::DRACULA,
        }
    }
}

impl From<Palette> for Theme {
    fn from(value: Palette) -> Self {
        value.theme()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub background: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub border: Color,
    pub border_focus: Color,
    pub text: Color,
    pub text_strong: Color,
    pub text_dim: Color,
    pub primary: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub info: Color,
    pub selection: Color,
    /// Empty rail of a progress bar drawn on the selection band.
    ///
    /// Its own role rather than `text_dim`, because `text_dim` collapses onto
    /// `selection` in the sixteen-slot palette: `DarkGray` and `Indexed(8)`
    /// are the same ANSI slot, so the rail would render invisible there.
    pub track_selected: Color,
}

impl Theme {
    /// Default dark theme.
    pub const DARK: Self = Self {
        background: Color::Rgb(17, 19, 26),
        surface: Color::Rgb(23, 26, 35),
        surface_alt: Color::Rgb(30, 34, 46),
        border: Color::Rgb(42, 47, 61),
        border_focus: Color::Rgb(122, 162, 247),
        text: Color::Rgb(200, 208, 224),
        text_strong: Color::Rgb(230, 236, 248),
        text_dim: Color::Rgb(107, 116, 136),
        primary: Color::Rgb(122, 162, 247),
        accent: Color::Rgb(187, 154, 247),
        success: Color::Rgb(158, 206, 106),
        warning: Color::Rgb(224, 175, 104),
        danger: Color::Rgb(247, 118, 142),
        info: Color::Rgb(125, 207, 255),
        selection: Color::Rgb(35, 40, 56),
        track_selected: Color::Rgb(107, 116, 136),
    };

    /// Palette driven by the terminal's own color scheme.
    ///
    /// `Reset` yields the emulator's default foreground and background, which
    /// also preserves a transparent one. Text that only needs emphasis is left
    /// on `Reset` too, since the styles carry `BOLD`: a fixed white would
    /// disappear on a light background.
    pub const TERMINAL: Self = Self {
        background: Color::Reset,
        surface: Color::Reset,
        surface_alt: Color::Indexed(8),
        border: Color::DarkGray,
        border_focus: Color::Blue,
        text: Color::Reset,
        text_strong: Color::Reset,
        text_dim: Color::DarkGray,
        primary: Color::Blue,
        accent: Color::Magenta,
        success: Color::Green,
        warning: Color::Yellow,
        danger: Color::Red,
        info: Color::Cyan,
        selection: Color::Indexed(8),
        track_selected: Color::Gray,
    };

    /// Light palette for a bright terminal background.
    ///
    /// Every foreground clears WCAG AA against `background`, which a dark
    /// palette shown on a white terminal never does.
    pub const LIGHT: Self = Self {
        background: Color::Rgb(255, 255, 255),
        surface: Color::Rgb(247, 248, 250),
        surface_alt: Color::Rgb(237, 239, 243),
        border: Color::Rgb(214, 218, 226),
        border_focus: Color::Rgb(23, 105, 215),
        text: Color::Rgb(44, 50, 62),
        text_strong: Color::Rgb(17, 20, 28),
        text_dim: Color::Rgb(110, 118, 134),
        primary: Color::Rgb(23, 105, 215),
        accent: Color::Rgb(137, 60, 190),
        success: Color::Rgb(26, 127, 55),
        warning: Color::Rgb(154, 103, 0),
        danger: Color::Rgb(191, 38, 53),
        info: Color::Rgb(2, 116, 143),
        selection: Color::Rgb(223, 232, 246),
        track_selected: Color::Rgb(110, 118, 134),
    };

    /// Nord.
    pub const NORD: Self = Self {
        background: Color::Rgb(46, 52, 64),
        surface: Color::Rgb(59, 66, 82),
        surface_alt: Color::Rgb(67, 76, 94),
        border: Color::Rgb(76, 86, 106),
        border_focus: Color::Rgb(136, 192, 208),
        text: Color::Rgb(216, 222, 233),
        text_strong: Color::Rgb(236, 239, 244),
        text_dim: Color::Rgb(118, 129, 150),
        primary: Color::Rgb(129, 161, 193),
        accent: Color::Rgb(180, 142, 173),
        success: Color::Rgb(163, 190, 140),
        warning: Color::Rgb(235, 203, 139),
        danger: Color::Rgb(191, 97, 106),
        info: Color::Rgb(143, 188, 187),
        selection: Color::Rgb(67, 76, 94),
        track_selected: Color::Rgb(118, 129, 150),
    };

    /// Gruvbox, dark medium.
    pub const GRUVBOX: Self = Self {
        background: Color::Rgb(40, 40, 40),
        surface: Color::Rgb(50, 48, 47),
        surface_alt: Color::Rgb(60, 56, 54),
        border: Color::Rgb(102, 92, 84),
        border_focus: Color::Rgb(131, 165, 152),
        text: Color::Rgb(235, 219, 178),
        text_strong: Color::Rgb(251, 241, 199),
        text_dim: Color::Rgb(168, 153, 132),
        primary: Color::Rgb(131, 165, 152),
        accent: Color::Rgb(211, 134, 155),
        success: Color::Rgb(184, 187, 38),
        warning: Color::Rgb(250, 189, 47),
        danger: Color::Rgb(251, 73, 52),
        info: Color::Rgb(142, 192, 124),
        selection: Color::Rgb(80, 73, 69),
        track_selected: Color::Rgb(168, 153, 132),
    };

    /// Dracula.
    pub const DRACULA: Self = Self {
        background: Color::Rgb(33, 34, 44),
        surface: Color::Rgb(40, 42, 54),
        surface_alt: Color::Rgb(52, 55, 70),
        border: Color::Rgb(68, 71, 90),
        border_focus: Color::Rgb(189, 147, 249),
        text: Color::Rgb(248, 248, 242),
        text_strong: Color::Rgb(255, 255, 255),
        text_dim: Color::Rgb(98, 114, 164),
        primary: Color::Rgb(189, 147, 249),
        accent: Color::Rgb(255, 121, 198),
        success: Color::Rgb(80, 250, 123),
        warning: Color::Rgb(255, 184, 108),
        danger: Color::Rgb(255, 85, 85),
        info: Color::Rgb(139, 233, 253),
        selection: Color::Rgb(68, 71, 90),
        track_selected: Color::Rgb(98, 114, 164),
    };

    pub const fn base(&self) -> Style {
        Style::new().fg(self.text).bg(self.background)
    }

    pub const fn panel(&self) -> Style {
        Style::new().fg(self.text).bg(self.surface)
    }

    pub const fn dim(&self) -> Style {
        Style::new().fg(self.text_dim)
    }

    pub const fn strong(&self) -> Style {
        Style::new()
            .fg(self.text_strong)
            .add_modifier(Modifier::BOLD)
    }

    pub const fn accented(&self) -> Style {
        Style::new().fg(self.accent)
    }

    pub const fn primary_style(&self) -> Style {
        Style::new().fg(self.primary)
    }

    /// Border style for a panel, highlighted when it holds the focus.
    pub const fn border_style(&self, focused: bool) -> Style {
        if focused {
            Style::new().fg(self.border_focus)
        } else {
            Style::new().fg(self.border)
        }
    }

    /// Title style for a panel, highlighted when it holds the focus.
    pub const fn title_style(&self, focused: bool) -> Style {
        if focused {
            Style::new()
                .fg(self.border_focus)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(self.text_dim).add_modifier(Modifier::BOLD)
        }
    }

    pub const fn selected(&self) -> Style {
        Style::new()
            .fg(self.text_strong)
            .bg(self.selection)
            .add_modifier(Modifier::BOLD)
    }

    pub const fn status(&self, status: SpecStatus) -> Color {
        match status {
            SpecStatus::Draft => self.info,
            SpecStatus::Review => self.warning,
            SpecStatus::Approved => self.primary,
            SpecStatus::Implemented => self.success,
            SpecStatus::Deprecated => self.danger,
            SpecStatus::Unknown => self.text_dim,
        }
    }

    /// Color used for a completion ratio in `0.0..=1.0`.
    pub fn progress(&self, ratio: f64) -> Color {
        if ratio >= 0.999 {
            self.success
        } else if ratio >= 0.5 {
            self.primary
        } else if ratio > 0.0 {
            self.warning
        } else {
            self.text_dim
        }
    }

    // ---- Markdown ---------------------------------------------------------

    pub const fn heading(&self, level: u8) -> Style {
        let color = match level {
            1 => self.accent,
            2 => self.primary,
            3 => self.info,
            _ => self.text_strong,
        };
        Style::new().fg(color).add_modifier(Modifier::BOLD)
    }

    pub const fn code(&self) -> Style {
        Style::new().fg(self.info).bg(self.surface_alt)
    }

    pub const fn quote(&self) -> Style {
        Style::new()
            .fg(self.text_dim)
            .add_modifier(Modifier::ITALIC)
    }

    pub const fn link(&self) -> Style {
        Style::new()
            .fg(self.info)
            .add_modifier(Modifier::UNDERLINED)
    }

    pub const fn bullet(&self) -> Style {
        Style::new().fg(self.accent)
    }

    pub const fn rule(&self) -> Style {
        Style::new().fg(self.border)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::TERMINAL
    }
}

/// The ANSI slot a color ends up on, so two names for one slot compare equal.
///
/// `DarkGray` and `Indexed(8)` are different variants and the same color on
/// screen. Anything that is not one of the sixteen named slots is returned
/// unchanged, since it has no slot to collide on.
#[cfg(test)]
pub(crate) fn ansi_slot(color: Color) -> Color {
    let index = match color {
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        other => return other,
    };
    Color::Indexed(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every color a palette declares, for the invariants below.
    fn colors(theme: &Theme) -> [Color; 16] {
        [
            theme.background,
            theme.surface,
            theme.surface_alt,
            theme.border,
            theme.border_focus,
            theme.text,
            theme.text_strong,
            theme.text_dim,
            theme.primary,
            theme.accent,
            theme.success,
            theme.warning,
            theme.danger,
            theme.info,
            theme.selection,
            theme.track_selected,
        ]
    }

    #[test]
    fn the_terminal_palette_never_hardcodes_a_color() {
        assert!(
            !colors(&Theme::TERMINAL)
                .iter()
                .any(|color| matches!(color, Color::Rgb(..))),
            "an RGB triple would ignore the terminal color scheme"
        );
    }

    #[test]
    fn every_other_palette_is_fully_explicit() {
        for palette in Palette::ALL {
            if palette == Palette::Terminal {
                continue;
            }
            assert!(
                colors(&palette.theme())
                    .iter()
                    .all(|color| matches!(color, Color::Rgb(..))),
                "{} mixes an ANSI slot into a fixed palette",
                palette.name()
            );
        }
    }

    #[test]
    fn names_round_trip_and_cycle_through_every_palette() {
        let mut seen = Vec::new();
        let mut palette = Palette::default();

        for _ in Palette::ALL {
            assert_eq!(Palette::from_name(palette.name()), Some(palette));
            seen.push(palette);
            palette = palette.next();
        }

        assert_eq!(palette, Palette::default(), "the cycle has to wrap around");
        assert_eq!(seen.len(), Palette::ALL.len());
    }

    /// The empty rail of a progress bar is drawn on the selection band, so a
    /// palette that puts both on one slot renders it invisible — which is what
    /// `text_dim` did in `terminal`, where it shares slot 8 with `selection`.
    #[test]
    fn the_selected_track_never_shares_a_slot_with_the_selection() {
        for palette in Palette::ALL {
            let theme = palette.theme();
            assert_ne!(
                ansi_slot(theme.track_selected),
                ansi_slot(theme.selection),
                "{} draws the selected progress track in the selection colour",
                palette.name()
            );
        }
    }

    #[test]
    fn an_unknown_name_is_rejected() {
        assert_eq!(Palette::from_name("  NORD "), Some(Palette::Nord));
        assert_eq!(Palette::from_name("solarized"), None);
    }

    #[test]
    fn the_name_list_covers_every_palette() {
        // It is what `:theme` prints when it rejects a name, so a palette
        // missing from it is a palette the user cannot discover.
        let names = Palette::names();
        for palette in Palette::ALL {
            assert!(names.contains(palette.name()), "{} missing", palette.name());
        }
        assert_eq!(names.split(", ").count(), Palette::ALL.len());
    }

    #[test]
    fn the_default_theme_is_the_terminal_one() {
        assert_eq!(Palette::default(), Palette::Terminal);
        assert_eq!(colors(&Theme::default()), colors(&Theme::TERMINAL));
    }

    #[test]
    fn converting_a_palette_yields_its_own_theme() {
        for palette in Palette::ALL {
            assert_eq!(
                colors(&Theme::from(palette)),
                colors(&palette.theme()),
                "{} converts to a different theme",
                palette.name()
            );
        }
    }

    /// Every style helper, so one wired to nothing cannot pass unnoticed.
    fn all_styles(theme: &Theme) -> Vec<(&'static str, Style)> {
        vec![
            ("base", theme.base()),
            ("panel", theme.panel()),
            ("dim", theme.dim()),
            ("strong", theme.strong()),
            ("accented", theme.accented()),
            ("primary_style", theme.primary_style()),
            ("border_style", theme.border_style(false)),
            ("border_focused", theme.border_style(true)),
            ("title_style", theme.title_style(false)),
            ("title_focused", theme.title_style(true)),
            ("selected", theme.selected()),
            ("heading", theme.heading(1)),
            ("code", theme.code()),
            ("quote", theme.quote()),
            ("link", theme.link()),
            ("bullet", theme.bullet()),
            ("rule", theme.rule()),
        ]
    }

    #[test]
    fn every_style_sets_a_foreground_in_every_palette() {
        for palette in Palette::ALL {
            for (name, style) in all_styles(&palette.theme()) {
                assert!(
                    style.fg.is_some(),
                    "{name} has no foreground in {}",
                    palette.name()
                );
            }
        }
    }

    #[test]
    fn focus_visibly_changes_a_panel_border_and_title() {
        for palette in Palette::ALL {
            let theme = palette.theme();
            assert_ne!(
                theme.border_style(true).fg,
                theme.border_style(false).fg,
                "{} cannot show which panel has the focus",
                palette.name()
            );
            assert_ne!(theme.title_style(true).fg, theme.title_style(false).fg);
            // Both weights of the title stay bold; only the colour carries focus.
            assert!(
                theme
                    .title_style(true)
                    .add_modifier
                    .contains(Modifier::BOLD)
            );
            assert!(
                theme
                    .title_style(false)
                    .add_modifier
                    .contains(Modifier::BOLD)
            );
        }
    }

    #[test]
    fn the_selection_band_paints_a_background() {
        for palette in Palette::ALL {
            let theme = palette.theme();
            assert_eq!(theme.selected().bg, Some(theme.selection));
            assert!(theme.selected().add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn each_status_gets_its_own_role_colour() {
        let theme = Theme::DARK;
        assert_eq!(theme.status(SpecStatus::Draft), theme.info);
        assert_eq!(theme.status(SpecStatus::Review), theme.warning);
        assert_eq!(theme.status(SpecStatus::Approved), theme.primary);
        assert_eq!(theme.status(SpecStatus::Implemented), theme.success);
        assert_eq!(theme.status(SpecStatus::Deprecated), theme.danger);
        assert_eq!(theme.status(SpecStatus::Unknown), theme.text_dim);
    }

    #[test]
    fn progress_only_turns_green_when_the_work_is_finished() {
        let theme = Theme::DARK;
        assert_eq!(theme.progress(0.0), theme.text_dim);
        assert_eq!(theme.progress(0.1), theme.warning);
        assert_eq!(theme.progress(0.5), theme.primary);
        assert_eq!(theme.progress(1.0), theme.success);
        // 99% is not done, and must not read as done.
        assert_eq!(theme.progress(0.99), theme.primary);
    }

    #[test]
    fn heading_levels_are_told_apart_down_to_the_third() {
        let theme = Theme::DARK;
        assert_eq!(theme.heading(1).fg, Some(theme.accent));
        assert_eq!(theme.heading(2).fg, Some(theme.primary));
        assert_eq!(theme.heading(3).fg, Some(theme.info));
        // Deeper levels share one colour rather than inventing more.
        assert_eq!(theme.heading(4).fg, Some(theme.text_strong));
        assert_eq!(theme.heading(6).fg, theme.heading(4).fg);
        for level in 1..=6 {
            assert!(theme.heading(level).add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn markdown_styles_carry_the_modifier_that_names_them() {
        let theme = Theme::DARK;
        assert!(theme.quote().add_modifier.contains(Modifier::ITALIC));
        assert!(theme.link().add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(theme.code().bg, Some(theme.surface_alt));
    }

    #[test]
    fn a_color_outside_the_sixteen_slots_is_returned_unchanged() {
        let rgb = Color::Rgb(1, 2, 3);
        assert_eq!(ansi_slot(rgb), rgb);
        assert_eq!(ansi_slot(Color::DarkGray), Color::Indexed(8));
        assert_eq!(ansi_slot(Color::Indexed(8)), Color::Indexed(8));
    }
}
