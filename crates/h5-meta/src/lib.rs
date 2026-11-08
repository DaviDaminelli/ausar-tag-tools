//! Reads a tag's data with its definition (PRD §6.4): the node tree, and the
//! checks that tell whether the definition fits the tag (§6.4.5, §9 check 1).
//! Domain only: no text or JSON (rendered in `h5tags-core`).

mod path;
mod read;
mod tree;

pub use path::{
    FieldPath, PathError, Section, SectionKind, Select, Selected, Step, find_sections, parse_path, select, suffix,
};

pub use read::read_tag;
pub use tree::{
    Block, Body, Content, DataRef, Divergence, EnumOption, FlagBits, Node, Problem, ReadTag, ReadUpTo, ResourceRef,
    StringIdValue, TagRef, Value, target_block,
};

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
