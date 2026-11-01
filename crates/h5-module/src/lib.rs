//! Reads Halo 5 `.module` files: header, file list, file names, resource list,
//! compression blocks, entry data, and the per-build index.
//!
//! No definitions, no XML, no MCP. Format facts and their sources are in
//! `docs/format-notes.md`.

mod check;
mod fourcc;
mod global_id;
pub mod index;
mod le;
mod module;
pub mod murmur3;
pub mod names;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod testutil;

pub use check::{CheckFailure, checks};
pub use fourcc::FourCc;
pub use global_id::GlobalId;
pub use module::{BlockEntry, EntryKind, FileEntry, FileFlags, Header, MAGIC, Module, ModuleError, Version, map_file};
