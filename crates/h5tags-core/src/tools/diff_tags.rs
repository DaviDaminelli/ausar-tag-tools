//! `diff_tags` (PRD §6.6): where two tags of one class differ.

use h5_meta::{Body, Content, FieldPath, ReadTag, find_sections, parse_path, select};
use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::diff::{Difference, Kind, Side, diff_at, diff_section};
use crate::output::{Cell, Output, Summary, Table, ToolError, col, marker};
use crate::session::Session;
use crate::stored::{load_definition, read_stored_tag};
use crate::tag_view::{BuildLookup, Detail, Lookup, Options, ViewSettings, entry_label, view_selection};
use crate::view::{ViewNode, value_column};

/// Where two tags of the same class differ: only the fields whose values
/// differ, as field path, a, b; tag blocks by entry count, then entry by
/// entry. path= or section= compares only that part. Each tag's copy in the
/// latest layer (tag_copies lists the others). The fields that differ are
/// candidates, not causes: check one on a third tag or with query_field
/// first. Next: read_tag on either tag for the rest.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct DiffTags {
    /// The first tag (global ID, tag path or display name).
    pub a: String,
    /// The second tag, of the same class.
    pub b: String,
    /// Only this part: `barrels`, `barrels[0].firing`.
    #[arg(long)]
    pub path: Option<String>,
    /// Only this section: a class section, explanation or group heading (as outline shows them).
    #[arg(long)]
    pub section: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

const DEFAULT_LIMIT: usize = 100;

/// The tag block entry at `path` (`barrels[1]`) in a read tag.
fn entry_at<'r, 'd>(read: &'r ReadTag<'d>, path: &FieldPath) -> Option<&'r Body<'d>> {
    let sel = select(&read.root, path).ok()?.into_iter().next()?;
    match (&sel.node.content, sel.entry) {
        (Content::Block(b), Some(i)) => b.entries.get(i as usize),
        _ => None,
    }
}

/// One side's cell for a difference: its value as `read_tag` writes it (brief).
fn side_text(
    read: &ReadTag<'_>,
    d: &Difference,
    shown: &str,
    side: Side,
    lookup: &dyn Lookup,
    settings: ViewSettings,
) -> Cell {
    let (path, kind) = (&d.path, d.kind);
    let count = |a: u32, z: u32| if side == Side::A { a } else { z };
    match kind {
        Kind::NotRead => marker::NOT_READ.into(),
        Kind::Count { a, b: z } => format!("block[{}]", count(a, z)).into(),
        Kind::OnlyIn(s) if s != side => marker::NO_ENTRY.into(),
        Kind::OnlyIn(_) => {
            let label = entry_at(read, path).and_then(entry_label);
            label.map_or_else(|| "present".into(), |l| format!("present: {l}").into())
        }
        Kind::Changed => {
            let Ok(sels) = select(&read.root, path) else { return marker::NOT_READ.into() };
            let view = view_selection(&sels, shown, settings, lookup);
            view.nodes
                .iter()
                .find_map(|n| match n {
                    ViewNode::Field(f) => Some(value_column(f)),
                    _ => None,
                })
                .unwrap_or_default()
                .into()
        }
    }
}

impl Tool for DiffTags {
    const NAME: &'static str = "diff_tags";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        if self.path.is_some() && self.section.is_some() {
            return Err(present::path_and_section());
        }
        let guid_check = s.guid_check;
        let ta = args::tag_copy(s, &self.a, &self.build, None)?;
        let tb = args::tag_copy(s, &self.b, &self.build, None)?;
        if !std::ptr::eq(ta.build, tb.build) {
            return Err(
                ToolError::new("the two tags are in different builds").hint("comparing builds comes later (M6)")
            );
        }
        let b = ta.build;
        if ta.tag.class != tb.tag.class {
            return Err(ToolError::new(format!(
                "{} is a {} and {} is a {}",
                b.display_name(ta.tag),
                b.class_name(ta.tag.class),
                b.display_name(tb.tag),
                b.class_name(tb.tag.class)
            ))
            .hint("diff_tags compares two tags of the same class"));
        }
        let Some(def) = load_definition(b, ta.tag).map_err(present::definition_error)? else {
            return Err(ToolError::new(present::no_definition(&b.class_name(ta.tag.class))));
        };
        let at = match &self.path {
            Some(p) => parse_path(&def.fields, p).map_err(present::path_error)?,
            None => FieldPath::default(),
        };
        let section = match self.section.as_deref() {
            Some(heading) => {
                let found = find_sections(&def.fields, heading);
                let [sec] = found.as_slice() else {
                    return Err(present::section_error(heading, &found, &def.fields));
                };
                Some(sec.clone())
            }
            None => None,
        };
        let (sa, sb) = (read_stored_tag(b, ta.copy), read_stored_tag(b, tb.copy));
        let (sa, sb) = (sa.map_err(present::read_error)?, sb.map_err(present::read_error)?);
        let (ra, rb) =
            (h5_meta::read_tag(&def, &sa.header, &sa.tag_data), h5_meta::read_tag(&def, &sb.header, &sb.tag_data));
        let found = match &section {
            Some(sec) => diff_section(&ra.root, &rb.root, &sec.parent, sec.start + 1..sec.end),
            None => {
                // A part only one tag has (an entry past the other's count) is a difference.
                if let (Err(e), Err(_)) = (select(&ra.root, &at), select(&rb.root, &at)) {
                    return Err(present::path_error(e));
                }
                diff_at(&ra.root, &rb.root, &at)
            }
        };
        let (la, lb) = (BuildLookup { build: b, copy: ta.copy }, BuildLookup { build: b, copy: tb.copy });
        let settings =
            ViewSettings { depth: 0, detail: Detail::Brief, options: Options::None, show_hidden: true, guid_check };
        // Values rendered only for the rows shown (D26), from the trees already read.
        let rows = self
            .page
            .page(&found, DEFAULT_LIMIT)
            .iter()
            .map(|d| {
                let shown = d.path.display(&def.fields);
                vec![
                    shown.clone().into(),
                    side_text(&ra, d, &shown, Side::A, &la, settings),
                    side_text(&rb, d, &shown, Side::B, &lb, settings),
                ]
            })
            .collect();
        let summary = vec![
            Summary { label: "a", key: "a", value: b.display_name(ta.tag).into() },
            Summary { label: "b", key: "b", value: b.display_name(tb.tag).into() },
        ];
        Ok(Output::Table(Table {
            build: s.label(b),
            total: found.len(),
            summary,
            offset: self.page.offset,
            columns: vec![col::FIELD_PATH, col::A, col::B],
            rows,
            hints: if found.is_empty() { Vec::new() } else { vec![present::CANDIDATES_HINT.into()] },
            ..Table::default()
        }))
    }
}

#[cfg(test)]
#[path = "diff_tags_tests.rs"]
mod tests;
