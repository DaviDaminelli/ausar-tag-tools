//! `h5tags`: the command-line front end. Every command is a tool from
//! `h5tags-core` (decisions D7); output goes to stdout, logs to stderr.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, Parser};
use h5tags_core::{Command, Config, Format, Session};
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
    let default_format = if std::io::stdout().is_terminal() { Format::Table } else { Format::Json };
    let format = cli.format.unwrap_or(default_format);
    match cli.command.run(&mut session) {
        Ok(out) => {
            print!("{}", out.render(format));
            ExitCode::SUCCESS
        }
        Err(e) => {
            print!("{}", e.render(format));
            ExitCode::from(1)
        }
    }
}
