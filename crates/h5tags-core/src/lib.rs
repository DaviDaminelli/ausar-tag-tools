//! Config, builds and the tools shared by the `h5tags` CLI and MCP server
//! (decisions D1, D7). No MCP code here.

pub mod catalog;
pub mod config;
pub mod def_view;
pub mod field_query;
pub mod filters;
pub mod output;
pub mod query;
pub mod references;
pub mod session;
pub mod stored;
pub mod tag_view;
/// A real deploy folder for tests (feature `testutil`; no game files).
#[cfg(any(test, feature = "testutil"))]
pub mod testdeploy;
pub mod tools;
pub mod view;

pub use config::Config;
pub use output::{Format, Output, ToolError};
pub use session::Session;
pub use tools::Command;
