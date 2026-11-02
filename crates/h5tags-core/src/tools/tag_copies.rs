//! `tag_copies` (PRD §6.6): every copy of a tag, and which hold the same data.

use h5_module::names::module_layer;
use serde::Deserialize;

use super::args::{self, BuildArg};
use super::{Tool, present};
use crate::output::{Cell, Output, Summary, Table, ToolError, col};
use crate::query::choose_copy;
use crate::session::Session;
use crate::stored::copy_versions;

/// Every copy of a tag (the same global ID stored in several modules): its
/// module, layer, data version, asset checksum and sizes. A layer is a game
/// update (base, then e.g. 11587-1, 12719-1: later versions); copies with the
/// same `data` letter hold the same bytes. Every tool reads the copy in the
/// latest layer unless module= picks another: use it to see earlier versions.
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
                let module = &b.module(copy).rel_path;
                vec![
                    module.clone().into(),
                    present::layer_text(module_layer(module)).into(),
                    version_letter(v).into(),
                    format!("{:016X}", e.asset_checksum).into(),
                    Cell::Size(e.uncompressed_size),
                    present::compressed_size(e),
                ]
            })
            .collect();
        let count = versions.iter().max().map_or(0, |m| m + 1);
        let mut summary = vec![Summary { label: "versions", key: "versions", value: Cell::Int(count as i64) }];
        if let Some(copy) = choose_copy(b, tag, None) {
            let value = b.module(copy).rel_path.clone().into();
            summary.push(Summary { label: "read by default", key: "read_by_default", value });
        }
        Ok(Output::Table(Table {
            build: s.label(b),
            total: rows.len(),
            summary,
            columns: vec![col::MODULE, col::LAYER, col::DATA, col::ASSET_CHECKSUM, col::SIZE, col::COMPRESSED],
            rows,
            ..Table::default()
        }))
    }
}

#[cfg(test)]
#[path = "tag_copies_tests.rs"]
mod tests;
