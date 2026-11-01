//! Every tool, defined once (decisions D7). The parameter struct of a tool is
//! its CLI subcommand (clap) and its MCP arguments (serde); its doc comments
//! are the help text of both.
//!
//! Layers: [`args`] turns raw inputs into domain values (bad input fails
//! there); the domain (`catalog`, `filters`, `query`) does the work on typed
//! values; [`present`] and `output` turn the results into what the agent reads.

pub mod args;
mod header_dump;
mod lists;
pub mod present;
mod read_tag;
mod reindex;
mod search;
#[cfg(test)]
mod tests;

pub use header_dump::HeaderDump;
pub use lists::{ListClasses, ListEntries, ListModules, ListResourceKinds};
pub use read_tag::ReadTag;
pub use reindex::{Reindex, What};
pub use search::{Browse, SearchTags};

use crate::output::{Output, ToolError};
use crate::session::Session;

pub trait Tool {
    /// The MCP tool name. The CLI command is the same with `-` for `_`.
    const NAME: &'static str;
    fn run(&self, session: &mut Session) -> Result<Output, ToolError>;
}

macro_rules! tools {
    ($($variant:ident),* $(,)?) => {
        /// One call of a tool, with its parameters.
        #[derive(Debug, clap::Subcommand)]
        pub enum Command {
            $( $variant($variant), )*
        }

        impl Command {
            pub fn name(&self) -> &'static str {
                match self { $( Command::$variant(_) => <$variant as Tool>::NAME, )* }
            }

            pub fn run(&self, session: &mut Session) -> Result<Output, ToolError> {
                match self { $( Command::$variant(t) => t.run(session), )* }
            }
        }

        /// Every tool name, in order.
        pub const TOOL_NAMES: &[&str] = &[$( <$variant as Tool>::NAME, )*];
    };
}

tools!(SearchTags, Browse, ListClasses, ListModules, ListEntries, ListResourceKinds, ReadTag, Reindex);

#[cfg(test)]
mod registry_tests {
    use clap::CommandFactory;

    use super::*;

    #[derive(clap::Parser)]
    struct Cli {
        #[command(subcommand)]
        command: Command,
    }

    /// D7: CLI name = tool name with `_` replaced by `-`.
    #[test]
    fn cli_names_follow_tool_names() {
        let cli = Cli::command();
        let cli_names: Vec<&str> = cli.get_subcommands().map(|c| c.get_name()).collect();
        let expected: Vec<String> = TOOL_NAMES.iter().map(|n| n.replace('_', "-")).collect();
        assert_eq!(cli_names, expected);
    }

    /// Shared parameters keep their flag names in every tool.
    #[test]
    fn shared_parameters_are_flags() {
        let cli = Cli::command();
        let search = cli.find_subcommand("search-tags").unwrap();
        let flags: Vec<&str> = search.get_arguments().filter_map(|a| a.get_long()).collect();
        for flag in ["build", "offset", "limit", "class", "module", "mode"] {
            assert!(flags.contains(&flag), "search-tags lacks --{flag}: {flags:?}");
        }
    }
}
