//! `h5tags`: the command-line front end. Every command is a tool from
//! `h5tags-core` (decisions D7); output goes to stdout, logs to stderr.

mod preferences;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, Parser};
use h5tags_core::{Command, Config, Format, Session};
use preferences::Preferences;
use tracing_subscriber::EnvFilter;

/// Reads Halo 5 .module tags.
#[derive(Parser)]
#[command(name = "h5tags", version)]
struct Cli {
    /// Config file (default: h5tags.toml next to the executable).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// One build without a config file: its deploy folder (needs --defs).
    #[arg(long, global = true)]
    deploy: Option<PathBuf>,
    /// With --deploy: Lord Zedd's H5Tags `tags` folder.
    #[arg(long, global = true)]
    defs: Option<PathBuf>,
    /// With --deploy: Lord Zedd's H5Tags `tag_resources` folder.
    #[arg(long, global = true)]
    resource_defs: Option<PathBuf>,
    /// Where the index cache is kept (overrides the config).
    #[arg(long, global = true)]
    cache_dir: Option<PathBuf>,
    /// Output format (default: table in a terminal, json when piped or redirected).
    #[arg(long, global = true, value_enum)]
    format: Option<Format>,
    /// No colors this time. To turn them off for good: `color = false` in the preferences file.
    #[arg(long, global = true)]
    no_color: bool,
    /// Preferences file (default: preferences.toml in your user config folder,
    /// e.g. %APPDATA%\h5tags on Windows, ~/.config/h5tags on Linux).
    #[arg(long, global = true)]
    preferences: Option<PathBuf>,
    /// More log output on stderr (-v, -vv). Or set H5TAGS_LOG.
    #[arg(short, long, global = true, action = ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    command: Command,
}

fn load_config(cli: &Cli) -> Result<Config, String> {
    let mut config = match (&cli.deploy, &cli.config) {
        (Some(_), Some(_)) => return Err("use either --config or --deploy, not both".into()),
        (Some(deploy), None) => {
            let defs = cli.defs.clone().ok_or("--deploy needs --defs <Lord Zedd's H5Tags tags folder>")?;
            Config::single(deploy.clone(), defs, cli.resource_defs.clone(), None)
        }
        (None, path) => {
            let path = match path {
                Some(p) => p.clone(),
                None => std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|d| d.join("h5tags.toml")))
                    .ok_or("can't find the executable's folder; pass --config")?,
            };
            if !path.exists() {
                return Err(format!(
                    "no config file at {}\nhint: pass --config <file>, or --deploy <folder> --defs <folder>",
                    path.display()
                ));
            }
            Config::load(&path).map_err(|e| e.to_string())?
        }
    };
    if let Some(dir) = &cli.cache_dir {
        config.cache_dir = Some(dir.clone());
    }
    Ok(config)
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

/// The `color` preference; a broken file is reported and ignored.
fn color_preference(path: Option<PathBuf>) -> Option<bool> {
    let path = path.or_else(preferences::default_path)?;
    match Preferences::load(&path) {
        Ok(p) => p.color,
        Err(e) => {
            eprintln!("warning: preferences file ignored: {e}");
            None
        }
    }
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
    let level = match cli.verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_env("H5TAGS_LOG").unwrap_or_else(|_| EnvFilter::new(level)))
        .init();

    let config = match load_config(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let mut session = match Session::open(config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    // People get aligned tables; scripts reading a pipe or file get JSON (D23).
    let terminal = std::io::stdout().is_terminal();
    let format = cli.format.unwrap_or(if terminal { Format::Table } else { Format::Json });
    let no_color_env = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let preferences_path = cli.preferences.clone();
    let color = use_color(format, terminal, cli.no_color, || color_preference(preferences_path), no_color_env);
    match cli.command.run(&mut session) {
        Ok(out) => {
            print_out(&out.render_with(format, color), color);
            ExitCode::SUCCESS
        }
        Err(e) => {
            print_out(&e.render_with(format, color), color);
            ExitCode::from(1)
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
    fn preference_beats_no_color_env() {
        assert!(!use_color(Format::Table, true, false, || None, true));
        assert!(use_color(Format::Table, true, false, || Some(true), true));
        assert!(!use_color(Format::Table, true, false, || Some(false), false));
    }
}
