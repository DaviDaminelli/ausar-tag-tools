//! Config, builds and the tools shared by the `h5tags` CLI and MCP server
//! (decisions D1, D7). No MCP code here.

pub mod catalog;
pub mod config;
pub mod filters;
pub mod output;
pub mod query;
pub mod session;
pub mod stored;
#[cfg(test)]
mod testdeploy;
pub mod tools;

pub use config::Config;
pub use output::{Format, Output, ToolError};
pub use session::Session;
pub use tools::Command;
