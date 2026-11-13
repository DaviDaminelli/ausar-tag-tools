//! `tag_copies` (PRD §6.6): every copy of a tag, and which hold the same data;
//! with `path=`, one field's value in each copy (M9, D102).

use h5_meta::{Select, parse_path, select};
use h5_module::names::module_layer;
use serde::Deserialize;

use super::args::{self, BuildArg};
use super::{Tool, present};
use crate::catalog::{Build, Tag};
use crate::field_query::value_field;
use crate::output::{Cell, Output, Summary, Table, ToolError, col, marker};
use crate::query::{choose_copy, layer_order};
use crate::session::Session;
use crate::stored::{copy_versions, load_definition, read_stored_tag};
use crate::tag_view::{BuildLookup, GuidCheck};

/// Every copy of a tag (the same global ID stored in several modules), in
/// layer order: its module, layer, data version, asset checksum and sizes. A
/// layer is a game update (base, then e.g. 11587-1, 12719-1: later versions);
/// copies with the same `data` letter hold the same bytes. Every tool reads
/// the copy in the latest layer unless module= picks another. path=: that
/// field's value in each copy instead (how it changed across updates).
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct TagCopies {
    /// The tag: global ID, tag path or display name.
    pub tag: String,
    /// A field (as query_field takes it): its value in each copy; [*] gives every entry.
    #[arg(long)]
    pub path: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
}

/// `A`, `B`, … for versions 0, 1, … (a tag has a handful of copies).
fn version_letter(v: usize) -> String {
    char::from_u32('A' as u32 + v as u32).map_or_else(|| v.to_string(), String::from)
}

/// One version's values for `path=`: each selection's field path (with `[*]`) and value.
type Values = Vec<(Option<String>, Cell)>;

/// A tag's copies as `tag_copies` lists them.
struct Listed<'b> {
    b: &'b Build,
    tag: &'b Tag,
    /// Each copy's data version (position in `tag.copies`).
    versions: Vec<usize>,
    /// How many versions.
    count: usize,
    /// Positions in `tag.copies`, in layer order.
    order: Vec<usize>,
}

impl Listed<'_> {
    /// Module, layer and data letter of copy `i`: the first cells of every row.
    fn cells(&self, i: usize) -> Vec<Cell> {
        let module = &self.b.module(self.tag.copies[i]).rel_path;
        vec![
            module.clone().into(),
            present::layer_text(module_layer(module)).into(),
            version_letter(self.versions[i]).into(),
        ]
    }

    /// One row per copy: the cells above, the asset checksum and sizes.
    fn copies_table(&self) -> Table {
        let rows = self
            .order
            .iter()
            .map(|&i| {
                let e = self.b.entry(self.tag.copies[i]);
                let mut row = self.cells(i);
                row.extend([
                    format!("{:016X}", e.asset_checksum).into(),
                    Cell::Size(e.uncompressed_size),
                    present::compressed_size(e),
                ]);
                row
            })
            .collect();
        let columns = vec![col::MODULE, col::LAYER, col::DATA, col::ASSET_CHECKSUM, col::SIZE, col::COMPRESSED];
        Table { columns, rows, ..Table::default() }
    }

    /// `path=`: one row per value per copy (several with `[*]`). Copies with
    /// the same data letter hold the same bytes: each version is read once,
    /// its rows reused (D26).
    fn field_table(&self, text: &str, guid_check: GuidCheck) -> Result<Table, ToolError> {
        let (b, tag) = (self.b, self.tag);
        let def = load_definition(b, tag)
            .map_err(present::definition_error)?
            .ok_or_else(|| ToolError::new(present::no_definition(&b.class_name(tag.class))))?;
        let path = parse_path(&def.fields, text).map_err(present::path_error)?;
        let shown = path.display(&def.fields);
        let field = value_field(&def.fields, &path, &shown).map_err(present::query_error)?;
        let several = path.steps.iter().any(|st| st.select == Some(Select::Every));
        let settings = present::value_settings(guid_check);
        let mut read: Vec<Option<Values>> = vec![None; self.count];
        let mut unread: Vec<String> = Vec::new();
        let mut rows = Vec::new();
        for &i in &self.order {
            let copy = tag.copies[i];
            let values = read[self.versions[i]].get_or_insert_with(|| match read_stored_tag(b, copy) {
                Err(e) => {
                    unread.push(format!("{} ({e})", b.module(copy).rel_path));
                    vec![(None, marker::NOT_READ.into())]
                }
                Ok(stored) => {
                    let tree = h5_meta::read_tag(&def, &stored.header, &stored.tag_data);
                    let lookup = BuildLookup { build: b, copy };
                    match select(&tree.root, &path) {
                        Ok(sels) if sels.is_empty() => vec![(None, marker::EMPTY.into())],
                        Ok(sels) => sels
                            .iter()
                            .map(|sel| {
                                let at = several.then(|| sel.path.clone());
                                (at, present::value_cell(sel, &shown, settings, &lookup))
                            })
                            .collect(),
                        Err(e) => vec![(None, present::missing_value(&e).into())],
                    }
                }
            });
            for (at, value) in values.iter() {
                let mut row = self.cells(i);
                if several {
                    row.push(at.clone().map_or(Cell::Empty, Cell::from));
                }
                row.push(value.clone());
                rows.push(row);
            }
        }
        let mut columns = vec![col::MODULE, col::LAYER, col::DATA];
        if several {
            columns.push(col::FIELD_PATH);
        }
        columns.push(col::VALUE);
        let hints = present::unread_copies(&unread).into_iter().collect();
        Ok(Table { summary: present::units(field).into_iter().collect(), columns, rows, hints, ..Table::default() })
    }
}

impl Tool for TagCopies {
    const NAME: &'static str = "tag_copies";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let (b, tag) = args::tag(s, &self.tag, &self.build)?;
        let versions = copy_versions(b, tag).map_err(present::read_error)?;
        let count = versions.iter().max().map_or(0, |m| m + 1);
        let listed = Listed { b, tag, versions, count, order: layer_order(b, tag) };
        let table = match &self.path {
            None => listed.copies_table(),
            Some(text) => listed.field_table(text, s.guid_check)?,
        };
        let mut summary = vec![Summary { label: "versions", key: "versions", value: Cell::Int(count as i64) }];
        if let Some(copy) = choose_copy(b, tag, None) {
            let value = b.module(copy).rel_path.clone().into();
            summary.push(Summary { label: "read by default", key: "read_by_default", value });
        }
        summary.extend(table.summary);
        Ok(Output::Table(Table { build: s.label(b), total: table.rows.len(), summary, ..table }))
    }
}

#[cfg(test)]
#[path = "tag_copies_tests.rs"]
mod tests;
