//! `h5tags`: the command-line front end. Every command is a tool from
//! `h5tags-core` (decisions D7); output goes to stdout, logs to stderr.

mod preferences;
mod progress;

use std::cell::LazyCell;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, Parser};
use h5tags_core::config::ConfigArgs;
use h5tags_core::session::ProgressSink;
use h5tags_core::tools::{HeaderDump, Identify};
use h5tags_core::{Command, Format, Output, Session, ToolError};
use preferences::{Key, Preferences};
use progress::SpinnerHandle;
use tracing_subscriber::EnvFilter;

/// Reads Halo 5 .module tags.
#[derive(Parser)]
#[command(name = "ausar-tag-cli", version)]
struct Cli {
    #[command(flatten)]
    config: ConfigArgs,
    /// Output format (default: table in a terminal, json when piped or redirected).
    #[arg(long, global = true, value_enum)]
    format: Option<Format>,
    /// No colors this time. To turn them off for good: `h5tags preferences set color off`.
    #[arg(long, global = true)]
    no_color: bool,
    /// No progress spinner this time. For good: `h5tags preferences set progress off`.
    #[arg(long, global = true)]
    no_progress: bool,
    /// Preferences file (default: preferences.toml in your user config folder,
    /// e.g. %APPDATA%\h5tags on Windows, ~/.config/h5tags on Linux).
    #[arg(long, global = true)]
    preferences: Option<PathBuf>,
    /// More log output on stderr (-v, -vv). Or set H5TAGS_LOG.
    #[arg(short, long, global = true, action = ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    command: CliCommand,
}

/// A tool (the same as over MCP, D7), or a command that only the CLI has.
#[derive(clap::Subcommand)]
enum CliCommand {
    #[command(flatten)]
    Tool(Command),
    /// Every list of a tag's header, as stored (developer command, CLI only).
    HeaderDump(HeaderDump),
    Identify(Identify),
    /// Show or change your display preferences (CLI only; the AI never sees them).
    Preferences {
        #[command(subcommand)]
        action: Option<PreferenceAction>,
    },
}

#[derive(clap::Subcommand)]
enum PreferenceAction {
    /// Set a preference: `set color off`, `set progress on` (also true/false, yes/no).
    Set {
        #[arg(value_enum)]
        key: Key,
        #[arg(action = ArgAction::Set, value_parser = clap::builder::BoolishValueParser::new())]
        value: bool,
    },
    /// Remove a preference: back to its default.
    Unset {
        #[arg(value_enum)]
        key: Key,
    },
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

/// `h5tags preferences [set|unset]`: needs no config and loads no game files.
fn run_preferences(action: Option<&PreferenceAction>, file: Option<PathBuf>) -> ExitCode {
    let Some(path) = file.or_else(preferences::default_path) else {
        eprintln!("error: can't find your user config folder; pass --preferences <file>");
        return ExitCode::from(2);
    };
    let loaded = Preferences::load(&path);
    let Some(action) = action else {
        let state = match (&loaded, path.exists()) {
            (Err(e), _) => format!("unreadable, defaults are used: {e}"),
            (Ok(_), false) => "not created yet".to_owned(),
            (Ok(_), true) => "exists".to_owned(),
        };
        println!("file: {} ({state})", path.display());
        let prefs = loaded.unwrap_or_default();
        for key in Key::ALL {
            let source = if prefs.get(key).is_some() { "set in the file" } else { "default" };
            println!("{}: {} ({source})", key.name(), on_off(prefs.value(key)));
        }
        return ExitCode::SUCCESS;
    };
    let mut prefs = match loaded {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}\nhint: fix or delete the file, then try again");
            return ExitCode::from(2);
        }
    };
    let message = match *action {
        PreferenceAction::Set { key, value } => {
            prefs.set(key, Some(value));
            format!("{}: {}", key.name(), on_off(value))
        }
        PreferenceAction::Unset { key } => {
            prefs.set(key, None);
            format!("{}: back to the default ({})", key.name(), on_off(key.default_value()))
        }
    };
    match prefs.save(&path) {
        Ok(()) => {
            println!("{message}; saved to {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: can't save the preferences: {e}");
            ExitCode::from(2)
        }
    }
}

/// Colors only for `table` output to a terminal (D24). Precedence: `--no-color`,
/// then the preferences file, then the `NO_COLOR` convention, then on. The
/// preferences are only read (`preference()`) when colors are possible (D25).
fn use_color(
    format: Format,
    terminal: bool,
    no_color_flag: bool,
    preference: impl FnOnce() -> Option<bool>,
    no_color_env: bool,
) -> bool {
    if format != Format::Table || !terminal || no_color_flag {
        return false;
    }
    preference().unwrap_or(!no_color_env)
}

/// The progress spinner only on a terminal (stderr), and not with `-v` (log
/// lines would make it flicker). Precedence: `--no-progress`, then the
/// preferences file, then on (D27).
fn use_progress(
    stderr_terminal: bool,
    no_progress_flag: bool,
    verbose: bool,
    preference: impl FnOnce() -> Option<bool>,
) -> bool {
    if !stderr_terminal || no_progress_flag || verbose {
        return false;
    }
    preference().unwrap_or(Key::Progress.default_value())
}

/// The preferences file; a broken file is reported and ignored.
fn read_preferences(path: Option<PathBuf>) -> Preferences {
    let Some(path) = path.or_else(preferences::default_path) else { return Preferences::default() };
    Preferences::load(&path).unwrap_or_else(|e| {
        eprintln!("warning: preferences file ignored: {e}");
        Preferences::default()
    })
}

/// What a tool call ended with.
enum Outcome {
    Done(Result<h5tags_core::Output, h5tags_core::ToolError>),
    /// The config or the builds couldn't be loaded.
    Failed(String),
}

/// Loads the builds and runs the command `name`, reporting progress to `spinner`.
fn run_tool(
    cli: &Cli,
    name: &str,
    run: impl FnOnce(&mut Session) -> Result<Output, ToolError>,
    spinner: Option<SpinnerHandle>,
) -> Outcome {
    let config = match cli.config.load() {
        Ok(c) => c,
        Err(e) => return Outcome::Failed(e),
    };
    let sink: ProgressSink = match spinner.clone() {
        Some(s) => Box::new(move |build, p| s.set_message(progress::progress_message(build, p))),
        None => Box::new(|_, _| {}),
    };
    let mut session = match Session::open_with_progress(config, sink) {
        Ok(s) => s,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    if let Some(s) = &spinner {
        s.set_message(format!("running {name}"));
    }
    Outcome::Done(run(&mut session))
}

/// Writes `text`, translating colors for consoles that need it (older Windows).
fn print_out(text: &str, color: bool) {
    let mut out: Box<dyn Write> = if color {
        Box::new(anstream::AutoStream::new(std::io::stdout(), anstream::ColorChoice::Always))
    } else {
        Box::new(std::io::stdout())
    };
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // A tool (D7) or a CLI-only command: its name, and how to run it on the
    // loaded builds; `identify` runs before any config exists (D63).
    type Run<'a> = Box<dyn FnOnce(&mut Session) -> Result<Output, ToolError> + 'a>;
    let (name, run): (&str, Option<Run>) = match &cli.command {
        CliCommand::Preferences { action } => return run_preferences(action.as_ref(), cli.preferences.clone()),
        CliCommand::Tool(tool) => (tool.name(), Some(Box::new(|s| tool.run(s)))),
        CliCommand::HeaderDump(dump) => ("header-dump", Some(Box::new(|s| dump.run(s)))),
        CliCommand::Identify(_) => ("identify", None),
    };

    // Read once, and only if a setting can matter (D25).
    let prefs = LazyCell::new(|| read_preferences(cli.preferences.clone()));
    let stdout_terminal = std::io::stdout().is_terminal();
    let show_progress =
        use_progress(std::io::stderr().is_terminal(), cli.no_progress, cli.verbose > 0, || prefs.get(Key::Progress));
    let spinner = show_progress.then(|| progress::Spinner::start("loading"));

    let level = match cli.verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let log_spinner = spinner.as_ref().map(progress::Spinner::handle);
    tracing_subscriber::fmt()
        .with_writer(move || progress::LogWriter(log_spinner.clone()))
        .with_env_filter(EnvFilter::try_from_env("H5TAGS_LOG").unwrap_or_else(|_| EnvFilter::new(level)))
        .init();

    let outcome = match (run, &cli.command) {
        (Some(run), _) => run_tool(&cli, name, run, spinner.as_ref().map(progress::Spinner::handle)),
        (None, CliCommand::Identify(identify)) => Outcome::Done(identify.run()),
        (None, _) => unreachable!("only identify runs without the builds"),
    };
    if let Some(s) = spinner {
        s.finish(); // the line is gone before anything is printed
    }

    // People get aligned tables; scripts reading a pipe or file get JSON (D23).
    let format = cli.format.unwrap_or(if stdout_terminal { Format::Table } else { Format::Json });
    let no_color_env = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let color = use_color(format, stdout_terminal, cli.no_color, || prefs.get(Key::Color), no_color_env);
    match outcome {
        Outcome::Done(Ok(out)) => {
            print_out(&out.render_with(format, color), color);
            ExitCode::SUCCESS
        }
        Outcome::Done(Err(e)) => {
            print_out(&e.render_with(format, color), color);
            ExitCode::from(1)
        }
        Outcome::Failed(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_never() -> Option<bool> {
        panic!("the preferences file must not be read here")
    }

    #[test]
    fn colors_only_for_tables_in_a_terminal() {
        assert!(use_color(Format::Table, true, false, || None, false));
        // Piped, other formats, or --no-color: no colors, and the file isn't read.
        assert!(!use_color(Format::Table, false, false, read_never, false));
        assert!(!use_color(Format::Text, true, false, read_never, false));
        assert!(!use_color(Format::Json, true, false, read_never, false));
        assert!(!use_color(Format::Table, true, true, read_never, false));
    }

    #[test]
    fn command_line_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn preferences_command_parses() {
        let parse = |args: &[&str]| Cli::try_parse_from(std::iter::once("h5tags").chain(args.iter().copied()));
        let action = |args: &[&str]| match parse(args).map(|c| c.command) {
            Ok(CliCommand::Preferences { action }) => action,
            _ => panic!("not a preferences command: {args:?}"),
        };
        assert!(matches!(
            action(&["preferences", "set", "color", "off"]),
            Some(PreferenceAction::Set { key: Key::Color, value: false })
        ));
        assert!(matches!(
            action(&["preferences", "set", "progress", "yes"]),
            Some(PreferenceAction::Set { key: Key::Progress, value: true })
        ));
        assert!(matches!(
            action(&["preferences", "unset", "progress"]),
            Some(PreferenceAction::Unset { key: Key::Progress })
        ));
        assert!(action(&["preferences"]).is_none());
        assert!(parse(&["preferences", "set", "color", "maybe"]).is_err());
        assert!(parse(&["preferences", "set", "colour", "off"]).is_err());
        // Tools are unchanged.
        assert!(matches!(parse(&["search-tags", "magnum"]).unwrap().command, CliCommand::Tool(_)));
    }

    #[test]
    fn header_dump_is_a_cli_command() {
        let cli = Cli::try_parse_from(["h5tags", "header-dump", "magnum", "--module", "11587"]).unwrap();
        match cli.command {
            CliCommand::HeaderDump(d) => assert_eq!((d.tag.as_str(), d.module.as_deref()), ("magnum", Some("11587"))),
            _ => panic!("not header-dump"),
        }
        // Not a tool: the AI can't call it (PRD §6.7).
        assert!(!h5tags_core::tools::TOOL_NAMES.contains(&"header_dump"));
    }

    #[test]
    fn progress_only_on_a_terminal_without_verbose_logs() {
        assert!(use_progress(true, false, false, || None));
        assert!(!use_progress(true, false, false, || Some(false)));
        // Not a terminal, --no-progress or -v: off, and the file isn't read.
        assert!(!use_progress(false, false, false, read_never));
        assert!(!use_progress(true, true, false, read_never));
        assert!(!use_progress(true, false, true, read_never));
    }

    #[test]
    fn preference_beats_no_color_env() {
        assert!(!use_color(Format::Table, true, false, || None, true));
        assert!(use_color(Format::Table, true, false, || Some(true), true));
        assert!(!use_color(Format::Table, true, false, || Some(false), false));
    }
}
