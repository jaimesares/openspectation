//! The `:` command line.
//!
//! Every command has a keyboard equivalent; the line exists for the arguments
//! a keystroke cannot carry — a change name, a filter expression, a locale.

use rust_i18n::t;

use crate::tui::app::Tab;

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Validate {
        strict: bool,
        target: Option<String>,
    },
    Archive {
        skip_specs: bool,
        target: Option<String>,
    },
    New(String),
    Filter(String),
    /// Edits in place; a path hands the file to `$EDITOR` instead.
    Edit(Option<String>),
    /// Always hands the file to `$EDITOR`.
    External(Option<String>),
    /// Saves the buffer being edited.
    Write,
    Diff,
    Tasks,
    Dashboard,
    /// Opens the timeline of what happened in the workspace.
    Activity,
    Editor,
    /// Selects a sidebar tab.
    Open(Tab),
    Reload,
    Language(Option<String>),
    /// Switches the palette; without a name it cycles to the next one.
    Theme(Option<String>),
    /// Sets the sidebar width in columns; `None` hands it back to the layout.
    Width(Option<u16>),
    Clear,
    Help,
    /// Opens the preferences panel.
    Settings,
    Quit,
    /// Recognised as a command, but the arguments were wrong.
    Invalid(String),
    /// Not a command at all.
    Unknown(String),
}

/// Reads a command line, with or without its leading colon.
pub fn parse(raw: &str) -> Option<Command> {
    let trimmed = raw.trim().trim_start_matches(':').trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut words = trimmed.split_whitespace();
    let name = words.next()?.to_lowercase();
    let arguments: Vec<&str> = words.collect();

    let (flags, positional): (Vec<&str>, Vec<&str>) = arguments
        .iter()
        .partition(|word| word.starts_with("--") || word.starts_with('-'));
    let has = |flag: &str| flags.contains(&flag);
    let first = positional.first().map(|value| (*value).to_string());

    let command = match name.as_str() {
        "validate" | "v" => Command::Validate {
            // Validation is only useful when it is strict; the flag exists so
            // the command reads the same as the CLI, not to make it optional.
            strict: !has("--loose"),
            target: first,
        },
        "archive" | "a" => Command::Archive {
            skip_specs: has("--skip-specs"),
            target: first,
        },
        "new" | "n" => match first {
            Some(id) => Command::New(id),
            None => Command::Invalid(t!("command.needs_name", command = "new").to_string()),
        },
        // The rest of the line is the expression, spaces and all.
        "filter" | "f" => Command::Filter(after(trimmed)),
        "edit" | "e" => Command::Edit(first),
        "external" | "vi" => Command::External(first),
        "w" | "write" | "save" => Command::Write,
        "diff" | "d" => Command::Diff,
        "tasks" | "t" => Command::Tasks,
        "dashboard" | "g" => Command::Dashboard,
        "activity" | "history" | "timeline" => Command::Activity,
        "editor" => Command::Editor,
        "specs" => Command::Open(Tab::Specs),
        "changes" => Command::Open(Tab::Changes),
        "archived" | "archive-tab" => Command::Open(Tab::Archive),
        "project" => Command::Open(Tab::Project),
        "reload" | "r" => Command::Reload,
        "lang" | "language" => Command::Language(first),
        "theme" | "colors" => Command::Theme(first),
        "width" | "sidebar" => width(first.as_deref()),
        "clear" => Command::Clear,
        "settings" | "config" | "preferences" => Command::Settings,
        "help" | "h" | "?" => Command::Help,
        "q" | "quit" | "exit" => Command::Quit,
        other => Command::Unknown(other.to_string()),
    };

    Some(command)
}

/// A column count, or the word that means the sidebar sizes itself.
///
/// The keyword is English like every other command word: the line is a set of
/// verbs, not prose, and a translated `auto` would make the command depend on
/// which language the interface happens to be in.
fn width(argument: Option<&str>) -> Command {
    let unusable = || Command::Invalid(t!("command.bad_width").to_string());
    let Some(value) = argument else {
        return unusable();
    };

    if value.eq_ignore_ascii_case("auto") || value.eq_ignore_ascii_case("automatic") {
        return Command::Width(None);
    }
    value
        .parse::<u16>()
        .map_or_else(|_| unusable(), |columns| Command::Width(Some(columns)))
}

/// Everything after the first word, verbatim.
fn after(line: &str) -> String {
    line.split_once(char::is_whitespace)
        .map(|(_, rest)| rest.trim().to_string())
        .unwrap_or_default()
}

/// Command names offered by the help overlay, in presentation order.
pub const CATALOGUE: [&str; 18] = [
    "validate [--loose] [change]",
    "archive [--skip-specs] [change]",
    "new <change-id>",
    "filter <expression>",
    "edit  ·  w",
    "external [path]",
    "diff",
    "tasks",
    "dashboard",
    "activity",
    "changes | archived | specs | project",
    "lang [code]",
    "theme [name]",
    "width <columns> | auto",
    "settings",
    "reload",
    "clear",
    "quit",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_colon_is_not_a_command() {
        assert_eq!(parse(":"), None);
        assert_eq!(parse("   "), None);
    }

    #[test]
    fn validation_is_strict_unless_opted_out() {
        assert_eq!(
            parse(":validate"),
            Some(Command::Validate {
                strict: true,
                target: None
            })
        );
        assert_eq!(
            parse(":validate --loose add-history"),
            Some(Command::Validate {
                strict: false,
                target: Some("add-history".to_string())
            })
        );
    }

    #[test]
    fn archive_reads_its_flag_and_target() {
        assert_eq!(
            parse("archive --skip-specs add-history"),
            Some(Command::Archive {
                skip_specs: true,
                target: Some("add-history".to_string())
            })
        );
    }

    #[test]
    fn a_filter_keeps_its_whole_expression() {
        assert_eq!(
            parse(":filter tasks < 100%"),
            Some(Command::Filter("tasks < 100%".to_string()))
        );
        assert_eq!(parse(":filter"), Some(Command::Filter(String::new())));
    }

    #[test]
    fn new_without_a_name_is_rejected() {
        assert!(matches!(parse(":new"), Some(Command::Invalid(_))));
        assert_eq!(
            parse(":new add-oauth"),
            Some(Command::New("add-oauth".to_string()))
        );
    }

    #[test]
    fn archive_and_archived_are_different_commands() {
        assert_eq!(parse(":archived"), Some(Command::Open(Tab::Archive)));
        assert_eq!(
            parse(":archive"),
            Some(Command::Archive {
                skip_specs: false,
                target: None
            })
        );
    }

    #[test]
    fn a_bare_theme_cycles_while_a_named_one_selects() {
        assert_eq!(parse(":theme"), Some(Command::Theme(None)));
        assert_eq!(
            parse(":theme nord"),
            Some(Command::Theme(Some("nord".to_string())))
        );
    }

    #[test]
    fn an_unrecognised_word_is_reported_as_such() {
        assert_eq!(parse(":wat"), Some(Command::Unknown("wat".to_string())));
    }

    #[test]
    fn a_width_is_a_number_of_columns_or_the_word_auto() {
        assert_eq!(parse(":width 60"), Some(Command::Width(Some(60))));
        assert_eq!(parse(":sidebar auto"), Some(Command::Width(None)));
        assert_eq!(parse(":width AUTO"), Some(Command::Width(None)));
        assert!(matches!(parse(":width wide"), Some(Command::Invalid(_))));
        assert!(matches!(parse(":width"), Some(Command::Invalid(_))));
    }

    #[test]
    fn settings_answers_to_the_words_people_reach_for() {
        for line in [":settings", ":config", ":preferences"] {
            assert_eq!(parse(line), Some(Command::Settings), "{line}");
        }
    }
}
