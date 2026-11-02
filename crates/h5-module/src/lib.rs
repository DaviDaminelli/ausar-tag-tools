//! Reads Halo 5 `.module` files: header, file list, file names, resource list,
//! compression blocks, entry data, and the per-build index.
//!
//! No definitions, no XML, no MCP. Format facts and their sources are in
//! `docs/format-notes.md`.

pub mod cache;
mod check;
mod fourcc;
mod global_id;
pub mod index;
mod le;
mod module;
pub mod murmur3;
pub mod names;
pub mod scan;
pub mod tag;
pub mod text;

#[cfg(test)]
mod tests;
/// Builders for synthetic modules and tags, also for other crates' tests
/// (feature `testutil`; no game files in the repo).
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;

pub use check::{CheckFailure, Item, checks};
pub use fourcc::FourCc;
pub use global_id::GlobalId;
pub use module::{
    BlockEntry, EntryKind, FileEntry, FileFlags, Header, MAGIC, Module, ModuleError, ModuleLists, Section, Version,
    map_file,
};
