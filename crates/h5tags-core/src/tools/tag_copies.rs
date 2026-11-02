//! `tag_copies` (PRD §6.6): every copy of a tag, and which hold the same data.

use serde::Deserialize;

use super::args::{self, BuildArg};
use super::{Tool, present};
use crate::output::{Cell, Output, Summary, Table, ToolError, col};
use crate::session::Session;
use crate::stored::copy_versions;

/// Every copy of a tag (the same global ID stored in several modules): its
/// module, data version, asset checksum and sizes. Copies with the same `data`
/// letter hold the same bytes; read_tag reads the first copy unless module=
/// picks another.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct TagCopies {
    /// The tag: global ID, tag path or display name.
    pub tag: String,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
}

/// `A`, `B`, … for versions 0, 1, … (a tag has a handful of copies).
fn version_letter(v: usize) -> String {
    char::from_u32('A' as u32 + v as u32).map_or_else(|| v.to_string(), String::from)
}

impl Tool for TagCopies {
    const NAME: &'static str = "tag_copies";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let (b, tag) = args::tag(s, &self.tag, &self.build)?;
        let versions = copy_versions(b, tag).map_err(present::read_error)?;
        let rows: Vec<Vec<Cell>> = tag
            .copies
            .iter()
            .zip(&versions)
            .map(|(&copy, &v)| {
                let e = b.entry(copy);
                vec![
                    b.module(copy).rel_path.clone().into(),
                    version_letter(v).into(),
                    format!("{:016X}", e.asset_checksum).into(),
                    Cell::Size(e.uncompressed_size),
                    present::compressed_size(e),
                ]
            })
            .collect();
        let count = versions.iter().max().map_or(0, |m| m + 1);
        Ok(Output::Table(Table {
            build: s.label(b),
            total: rows.len(),
            summary: vec![Summary { label: "versions", key: "versions", value: Cell::Int(count as i64) }],
            columns: vec![col::MODULE, col::DATA, col::ASSET_CHECKSUM, col::SIZE, col::COMPRESSED],
            rows,
            ..Table::default()
        }))
    }
}

#[cfg(test)]
#[path = "tag_copies_tests.rs"]
mod tests;
