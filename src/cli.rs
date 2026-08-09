//! Command line surface.
//!
//! The structure is declared with clap's derive API, but the help text is not:
//! clap requires `&'static str` while translations are only known at runtime.
//! [`localized_command`] therefore rebuilds the generated [`clap::Command`],
//! swapping every description for its translated counterpart.

use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Arg, ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};
use rust_i18n::t;

use crate::domain::spec::SpecStatus;
use crate::i18n;
use crate::tui::theme::Palette;

#[derive(Debug, Parser)]
#[command(
    name = "openspectation",
    version,
    propagate_version = true,
    arg_required_else_help = true,
    disable_help_subcommand = true,
    styles = help_styles()
)]
pub struct Cli {
    /// Workspace root. Overridden per invocation with `--path`.
    #[arg(
        short = 'p',
        long,
        global = true,
        value_name = "PATH",
        default_value = ".",
        env = "OPENSPECTATION_PATH"
    )]
    pub path: PathBuf,

    /// Interface language override.
    #[arg(long, global = true, value_name = "LOCALE", env = i18n::LANG_ENV)]
    pub lang: Option<String>,

    /// Disables ANSI colors.
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Init(InitArgs),
    List(ListArgs),
    Tui(TuiArgs),
    Diff(DiffArgs),
    Validate(ValidateArgs),
    Archive(ArchiveArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Human readable project name.
    #[arg(value_name = "NAME")]
    pub name: String,

    /// Short description stored in `openspec.yaml`.
    #[arg(short, long, value_name = "TEXT", default_value = "")]
    pub description: String,

    /// Overwrites files that already exist.
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Also print the change proposals.
    #[arg(short, long)]
    pub all: bool,

    /// Restricts the output to a single status.
    #[arg(short, long, value_name = "STATUS", value_enum)]
    pub status: Option<StatusFilter>,

    /// Sort key.
    #[arg(long, value_name = "FIELD", value_enum, default_value_t = SortKey::Path)]
    pub sort: SortKey,
}

#[derive(Debug, Args)]
pub struct TuiArgs {
    /// Redraw interval in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = 250, value_parser = clap::value_parser!(u64).range(16..=5_000))]
    pub tick_rate: u64,

    /// Opens straight into one change proposal.
    #[arg(short = 'c', long, value_name = "CHANGE")]
    pub change: Option<String>,

    /// Color palette.
    ///
    /// Deliberately without a default: absent means "whatever the settings file
    /// says", and a default value here would be indistinguishable from the user
    /// asking for `terminal`.
    #[arg(long, value_name = "PALETTE", value_enum, env = "OPENSPECTATION_THEME")]
    pub theme: Option<Palette>,
}

#[derive(Debug, Args)]
pub struct DiffArgs {
    /// Change proposal to inspect; defaults to every active one.
    #[arg(value_name = "CHANGE")]
    pub change: Option<String>,
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// Change proposal to validate; defaults to the whole workspace.
    #[arg(value_name = "CHANGE")]
    pub change: Option<String>,

    /// Relaxes the strict rule set.
    #[arg(long)]
    pub loose: bool,
}

#[derive(Debug, Args)]
pub struct ArchiveArgs {
    /// Change proposal to archive.
    #[arg(value_name = "CHANGE")]
    pub change: String,

    /// Skips the confirmation prompt.
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Leaves the specifications untouched.
    #[arg(long)]
    pub skip_specs: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum StatusFilter {
    Draft,
    Review,
    Approved,
    Implemented,
    Deprecated,
    Unknown,
}

impl From<StatusFilter> for SpecStatus {
    fn from(value: StatusFilter) -> Self {
        match value {
            StatusFilter::Draft => Self::Draft,
            StatusFilter::Review => Self::Review,
            StatusFilter::Approved => Self::Approved,
            StatusFilter::Implemented => Self::Implemented,
            StatusFilter::Deprecated => Self::Deprecated,
            StatusFilter::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum SortKey {
    Path,
    Name,
    Title,
    Size,
    Status,
    Updated,
}

/// Builds the clap command with every description translated.
pub fn localized_command() -> clap::Command {
    let locales = i18n::available().join(", ");

    localize_chrome(Cli::command())
        .about(leak(t!("app.about")))
        .after_help(leak(format!(
            "{}\n{}: {}",
            t!("app.after_help"),
            t!("app.locale_label"),
            locales
        )))
        .subcommand_value_name(leak(t!("cli.help.command_value")))
        .subcommand_help_heading(leak(t!("cli.help.commands")))
        .mut_subcommands(localize_chrome)
        .mut_arg("path", |arg| arg.help(leak(t!("cli.arg.path"))))
        .mut_arg("lang", |arg| arg.help(leak(t!("cli.arg.lang"))))
        .mut_arg("no_color", |arg| arg.help(leak(t!("cli.arg.no_color"))))
        .mut_subcommand("init", |command| {
            command
                .about(leak(t!("cli.cmd.init.about")))
                .mut_arg("name", |arg| arg.help(leak(t!("cli.cmd.init.arg.name"))))
                .mut_arg("description", |arg| {
                    arg.help(leak(t!("cli.cmd.init.arg.description")))
                })
                .mut_arg("force", |arg| arg.help(leak(t!("cli.cmd.init.arg.force"))))
        })
        .mut_subcommand("list", |command| {
            command
                .about(leak(t!("cli.cmd.list.about")))
                .mut_arg("all", |arg| arg.help(leak(t!("cli.cmd.list.arg.all"))))
                .mut_arg("status", |arg| {
                    arg.help(leak(t!("cli.cmd.list.arg.status")))
                })
                .mut_arg("sort", |arg| arg.help(leak(t!("cli.cmd.list.arg.sort"))))
        })
        .mut_subcommand("tui", |command| {
            command
                .about(leak(t!("cli.cmd.tui.about")))
                .mut_arg("tick_rate", |arg| {
                    arg.help(leak(t!("cli.cmd.tui.arg.tick_rate")))
                })
                .mut_arg("change", |arg| arg.help(leak(t!("cli.cmd.tui.arg.change"))))
                .mut_arg("theme", |arg| arg.help(leak(t!("cli.cmd.tui.arg.theme"))))
        })
        .mut_subcommand("diff", |command| {
            command
                .about(leak(t!("cli.cmd.diff.about")))
                .mut_arg("change", |arg| {
                    arg.help(leak(t!("cli.cmd.diff.arg.change")))
                })
        })
        .mut_subcommand("validate", |command| {
            command
                .about(leak(t!("cli.cmd.validate.about")))
                .mut_arg("change", |arg| {
                    arg.help(leak(t!("cli.cmd.validate.arg.change")))
                })
                .mut_arg("loose", |arg| {
                    arg.help(leak(t!("cli.cmd.validate.arg.loose")))
                })
        })
        .mut_subcommand("archive", |command| {
            command
                .about(leak(t!("cli.cmd.archive.about")))
                .mut_arg("change", |arg| {
                    arg.help(leak(t!("cli.cmd.archive.arg.change")))
                })
                .mut_arg("yes", |arg| arg.help(leak(t!("cli.cmd.archive.arg.yes"))))
                .mut_arg("skip_specs", |arg| {
                    arg.help(leak(t!("cli.cmd.archive.arg.skip_specs")))
                })
        })
}

/// Translates the parts of the help output clap generates on its own: the
/// usage heading, the section headings and the built-in `--help` / `--version`
/// flags.
///
/// The automatic flags are replaced by explicit ones because clap only
/// materialises them while building, which is too late to translate them.
/// The usage heading has to be baked into the template as a literal, which is
/// why it loses clap's bold styling — a fair trade for a fully translated help.
fn localize_chrome(command: clap::Command) -> clap::Command {
    let template = format!(
        "{{before-help}}{{about-with-newline}}\n{} {{usage}}\n\n{{all-args}}{{after-help}}",
        t!("cli.help.usage")
    );
    let options_heading = leak(t!("cli.help.options"));

    command
        .help_template(leak(template))
        .disable_help_flag(true)
        .disable_version_flag(true)
        .mut_args(|arg| {
            let heading = if arg.is_positional() {
                leak(t!("cli.help.arguments"))
            } else {
                options_heading
            };
            arg.help_heading(heading)
        })
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .help(leak(t!("cli.help.help")))
                .help_heading(options_heading),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help(leak(t!("cli.help.version")))
                .help_heading(options_heading),
        )
}

/// clap stores help strings for the whole process lifetime and requires
/// `&'static str`; translations are produced at runtime, so they are leaked
/// once. The leaked set is bounded by the number of help strings.
fn leak(text: impl Into<String>) -> &'static str {
    Box::leak(text.into().into_boxed_str())
}

fn help_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Magenta.on_default().effects(Effects::BOLD))
        .usage(AnsiColor::Magenta.on_default().effects(Effects::BOLD))
        .literal(AnsiColor::Blue.on_default().effects(Effects::BOLD))
        .placeholder(AnsiColor::Cyan.on_default())
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Yellow.on_default())
        .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn localized_definition_is_valid() {
        localized_command().debug_assert();
    }
}
