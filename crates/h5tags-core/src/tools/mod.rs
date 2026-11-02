//! Every tool, defined once (decisions D7). The parameter struct of a tool is
//! its CLI subcommand (clap) and its MCP arguments (serde); its doc comments
//! are the help text of both.
//!
//! Layers: [`args`] turns raw inputs into domain values (bad input fails
//! there); the domain (`catalog`, `filters`, `query`) does the work on typed
//! values; [`present`] and `output` turn the results into what the agent reads.

pub mod args;
mod definitions;
mod find_references;
mod header_dump;
mod lists;
pub mod present;
mod query_field;
mod read_tag;
mod reindex;
mod search;
mod search_values;
mod tag_copies;
#[cfg(test)]
mod tests;

pub use definitions::{GetDefinition, Outline, Scope, SearchDefinitions};
pub use find_references::{Direction, FindReferences};
pub use header_dump::HeaderDump;
pub use lists::{ListClasses, ListEntries, ListModules, ListResourceKinds};
pub use query_field::QueryField;
pub use read_tag::ReadTag;
pub use reindex::{Reindex, What};
pub use search::{Browse, SearchTags};
pub use search_values::SearchValues;
pub use tag_copies::TagCopies;

use serde_json::Value;

use crate::output::{Output, ToolError};
use crate::session::Session;
pub use args::ToolSpec;
use args::{from_json, spec};

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

            /// The call of tool `name` with MCP arguments `args` (a JSON object).
            pub fn from_json(name: &str, args: Value) -> Result<Command, ToolError> {
                match name {
                    $( <$variant as Tool>::NAME => Ok(Command::$variant(from_json(name, args)?)), )*
                    _ => Err(ToolError::new(format!("no tool '{name}'"))
                        .hint(format!("tools: {}", TOOL_NAMES.join(", ")))),
                }
            }
        }

        /// Every tool name, in order.
        pub const TOOL_NAMES: &[&str] = &[$( <$variant as Tool>::NAME, )*];

        /// Every tool as MCP lists it, in order.
        pub fn tool_specs() -> Vec<ToolSpec> {
            vec![$( spec::<$variant>(), )*]
        }
    };
}

tools!(
    SearchTags,
    Browse,
    ListClasses,
    ListModules,
    ListEntries,
    ListResourceKinds,
    ReadTag,
    GetDefinition,
    Outline,
    SearchDefinitions,
    FindReferences,
    TagCopies,
    SearchValues,
    QueryField,
    Reindex
);

#[cfg(test)]
mod registry_tests {
    use clap::CommandFactory;
    use serde_json::json;

    use super::*;
    use crate::tag_view::Detail;

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

    /// D7: every CLI argument of a tool is a property of its MCP schema, and
    /// back; every tool has a description.
    #[test]
    fn mcp_schemas_match_the_cli() {
        let cli = Cli::command();
        let specs = tool_specs();
        assert_eq!(specs.iter().map(|s| s.name).collect::<Vec<_>>(), TOOL_NAMES);
        for spec in &specs {
            assert!(spec.description.len() > 20, "{}: {:?}", spec.name, spec.description);
            let sub = cli.find_subcommand(spec.name.replace('_', "-")).unwrap();
            let mut args: Vec<String> =
                sub.get_arguments().filter(|a| a.get_id() != "help").map(|a| a.get_id().to_string()).collect();
            let mut props: Vec<String> = spec.input_schema["properties"].as_object().unwrap().keys().cloned().collect();
            args.sort();
            props.sort();
            assert_eq!(args, props, "{}", spec.name);
        }
    }

    #[test]
    fn mcp_arguments_become_the_cli_command() {
        let c =
            Command::from_json("read_tag", json!({ "tag": "0x00008593", "path": "barrels[0]", "depth": 0 })).unwrap();
        let Command::ReadTag(t) = c else { panic!("{c:?}") };
        assert_eq!(
            (t.tag.as_str(), t.path.as_deref(), t.depth, t.detail),
            ("0x00008593", Some("barrels[0]"), 0, Detail::Normal)
        );
        let err = Command::from_json("read_tag", json!({ "tag": "x", "paths": "a" })).unwrap_err();
        assert_eq!(err.message, "read_tag has no parameter 'paths'");
        assert!(err.hint.unwrap().contains("path"));
        let err = Command::from_json("read_tag", json!({ "path": "a" })).unwrap_err();
        assert!(err.message.contains("tag"), "{}", err.message);
        assert!(Command::from_json("read_tags", json!({})).unwrap_err().message.contains("no tool 'read_tags'"));
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
