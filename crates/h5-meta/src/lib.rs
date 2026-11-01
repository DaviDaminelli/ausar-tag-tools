//! Reads a tag's data with its definition (PRD §6.4): the node tree, and the
//! checks that tell whether the definition fits the tag (§6.4.5, §9 check 1).
//! Domain only: no text or JSON (rendered in `h5tags-core`).

mod read;
mod tree;

pub use read::read_tag;
pub use tree::{Block, Body, Content, DataRef, Divergence, Node, Problem, ReadTag, TagRef, Value};

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
