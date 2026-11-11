//! `diff_tags` (PRD §6.6): where two tags of one class differ.

use h5_defs::definition::Field;
use h5_meta::{Body, Content, FieldPath, ReadTag, find_sections, parse_path, select, suffix};
use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs, TagCopy};
use super::{Tool, present};
use crate::diff::{Difference, Kind, Side, diff_at, diff_at_by_name, diff_section};
use crate::output::{Cell, Output, Summary, Table, ToolError, col, marker};
use crate::query::choose_copy;
use crate::script::{Form, LineChange, diff_lines, diff_units, read_script, script_field};
use crate::session::Session;
use crate::stored::{load_definition, read_stored_tag};
use crate::tag_view::{BuildLookup, Detail, Lookup, Options, ViewSettings, entry_label, view_selection};
use crate::view::{ViewNode, value_column};

/// Where two tags of the same class differ: only the fields whose values
/// differ, as field path, a, b; tag blocks by entry count, then entry by
/// entry. path= or section= compares only that part. Two script tags (hsc*,
/// luas): their changed script lines instead. Each tag's copy in the latest
/// layer; module_a= / module_b= pick another (the same tag twice compares two
/// updates). The two tags may be in two builds: a='forge:<tag>',
/// b='xbox:<tag>'. The fields that differ are candidates, not causes: check
/// one on a third tag or with query_field first. Next: read_tag on either tag.
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
    /// Read a's copy in the module whose path contains this.
    #[arg(long)]
    pub module_a: Option<String>,
    /// Read b's copy in the module whose path contains this.
    #[arg(long)]
    pub module_b: Option<String>,
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

/// A row's field path (D72): tag a's entry indices, with b's after them where
/// they differ (`resources[1] (b: [2]).value`); an entry only b has, b's
/// alone (`resources (b: [2])`).
fn path_text(d: &Difference, fields: &[Field]) -> String {
    let b = d.path_in(Side::B);
    let last = d.path.steps.len().saturating_sub(1);
    d.path.display_with(fields, |k, select| {
        let in_b = b.steps.get(k).map_or(select, |s| s.select);
        if d.kind == Kind::OnlyIn(Side::B) && k == last {
            return format!(" (b: {})", suffix(in_b));
        }
        match in_b {
            other if other != select => format!("{} (b: {})", suffix(select), suffix(other)),
            _ => suffix(select),
        }
    })
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
    let (path, kind) = (d.path_in(side), d.kind);
    let count = |a: u32, z: u32| if side == Side::A { a } else { z };
    match kind {
        Kind::NotRead => marker::NOT_READ.into(),
        Kind::Count { a, b: z } => format!("block[{}]", count(a, z)).into(),
        Kind::OnlyIn(s) if s != side => marker::NO_ENTRY.into(),
        Kind::OnlyIn(_) => {
            let label = entry_at(read, path).and_then(entry_label);
            label.map_or_else(|| "present".into(), |l| format!("present: {l}").into())
        }
        Kind::FieldOnlyIn(s) if s != side => marker::NO_FIELD.into(),
        // The value the field has here (with `[*]`, in the first entry).
        Kind::Changed | Kind::FieldOnlyIn(_) => {
            let Ok(sels) = select(&read.root, path) else { return marker::NOT_READ.into() };
            let view = view_selection(&sels, shown, settings, lookup);
            view.nodes
                .iter()
                .find_map(|n| match n {
                    // A whole struct or tag block (a field only one definition has): its type, `block[N]`.
                    ViewNode::Field(f) if f.container.is_some() => Some(f.short_type.clone()),
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
        // Each tag in its own build (`xbox:…` picks one, D83).
        let ta = args::tag_copy(s, &self.a, &self.build, self.module_a.as_deref())?;
        let tb = args::tag_copy(s, &self.b, &self.build, self.module_b.as_deref())?;
        let (ba, bb) = (ta.build, tb.build);
        let one_build = std::ptr::eq(ba, bb);
        if ta.tag.class != tb.tag.class {
            return Err(ToolError::new(format!(
                "{} is a {} and {} is a {}",
                ba.display_name(ta.tag),
                ba.class_name(ta.tag.class),
                bb.display_name(tb.tag),
                bb.class_name(tb.tag.class)
            ))
            .hint("diff_tags compares two tags of the same class"));
        }
        let summary = vec![
            Summary { label: "a", key: "a", value: copy_label(&ta, one_build).into() },
            Summary { label: "b", key: "b", value: copy_label(&tb, one_build).into() },
        ];
        let build = if one_build { s.label(ba) } else { None };
        if script_field(ta.tag.class).is_some() && self.path.is_none() && self.section.is_none() {
            return self.script_diff(&ta, &tb, summary, build);
        }
        // Each tag with its own build's definition; two different ones are
        // compared field by field by name (D83).
        let definition = |b: &crate::catalog::Build, t: &crate::catalog::Tag| {
            load_definition(b, t)
                .map_err(present::definition_error)?
                .ok_or_else(|| ToolError::new(present::no_definition(&b.class_name(t.class))))
        };
        let (def_a, def_b) = (definition(ba, ta.tag)?, definition(bb, tb.tag)?);
        let by_name = def_a != def_b;
        let in_b = |e: ToolError| ToolError { message: format!("in {}'s definition: {}", bb.name, e.message), ..e };
        let (at_a, at_b) = match &self.path {
            Some(p) => (
                parse_path(&def_a.fields, p).map_err(present::path_error)?,
                parse_path(&def_b.fields, p).map_err(present::path_error).map_err(in_b)?,
            ),
            None => (FieldPath::default(), FieldPath::default()),
        };
        let section = match self.section.as_deref() {
            Some(_) if by_name => {
                return Err(ToolError::new("section= needs the same definition in both builds")
                    .hint("the builds define this class differently: use path= instead"));
            }
            Some(heading) => {
                let found = find_sections(&def_a.fields, heading);
                let [sec] = found.as_slice() else {
                    return Err(present::section_error(heading, &found, &def_a.fields));
                };
                Some(sec.clone())
            }
            None => None,
        };
        let (sa, sb) = (read_stored_tag(ba, ta.copy), read_stored_tag(bb, tb.copy));
        let (sa, sb) = (sa.map_err(present::read_error)?, sb.map_err(present::read_error)?);
        let ra = h5_meta::read_tag(&def_a, &sa.header, &sa.tag_data);
        let rb = h5_meta::read_tag(&def_b, &sb.header, &sb.tag_data);
        let found = match &section {
            Some(sec) => diff_section(&ra.root, &rb.root, &sec.parent, sec.start + 1..sec.end),
            None => {
                // A part only one tag has (an entry past the other's count) is a difference.
                if let (Err(e), Err(_)) = (select(&ra.root, &at_a), select(&rb.root, &at_b)) {
                    return Err(present::path_error(e));
                }
                if by_name {
                    diff_at_by_name(&ra.root, &rb.root, &at_a, &at_b)
                } else {
                    diff_at(&ra.root, &rb.root, &at_a)
                }
            }
        };
        let (la, lb) = (BuildLookup { build: ba, copy: ta.copy }, BuildLookup { build: bb, copy: tb.copy });
        let settings =
            ViewSettings { depth: 0, detail: Detail::Brief, options: Options::None, show_hidden: true, guid_check };
        // Values rendered only for the rows shown (D26), from the trees already read.
        let rows = self
            .page
            .page(&found, DEFAULT_LIMIT)
            .iter()
            .map(|d| {
                // A field only b's definition has: its path is in b's definition.
                let fields = if d.kind == Kind::FieldOnlyIn(Side::B) { &def_b.fields } else { &def_a.fields };
                let shown = path_text(d, fields);
                vec![
                    shown.clone().into(),
                    side_text(&ra, d, &shown, Side::A, &la, settings),
                    side_text(&rb, d, &shown, Side::B, &lb, settings),
                ]
            })
            .collect();
        let mut hints = Vec::new();
        if by_name {
            hints.push(present::definitions_differ_hint(&ba.class_name(ta.tag.class)));
        }
        if !found.is_empty() {
            hints.push(present::CANDIDATES_HINT.into());
        }
        Ok(Output::Table(Table {
            build,
            total: found.len(),
            summary,
            offset: self.page.offset,
            columns: vec![col::FIELD_PATH, col::A, col::B],
            rows,
            hints,
            ..Table::default()
        }))
    }
}

impl DiffTags {
    /// Two script tags: their changed lines (D89), each with its line number
    /// in a or in b, `- ` removed, `+ ` added, without indentation.
    fn script_diff(
        &self,
        ta: &TagCopy<'_>,
        tb: &TagCopy<'_>,
        mut summary: Vec<Summary>,
        build: Option<String>,
    ) -> Result<Output, ToolError> {
        let sa = read_script(ta.build, ta.tag, ta.copy).map_err(present::script_error)?;
        let sb = read_script(tb.build, tb.tag, tb.copy).map_err(present::script_error)?;
        let (la, lb) = (diff_units(&sa), diff_units(&sb));
        let changes = diff_lines(&la, &lb).map_err(present::script_diff_too_large)?;
        let unit = if matches!(sa.form, Form::Compiled { .. }) { "strings" } else { "lines" };
        summary.push(Summary { label: unit, key: unit, value: format!("{} / {}", la.len(), lb.len()).into() });
        let rows = self
            .page
            .page(&changes, DEFAULT_LIMIT)
            .iter()
            .map(|c| match *c {
                LineChange::Removed(i) => {
                    vec![Cell::Int(i as i64 + 1), Cell::Empty, format!("- {}", la[i].trim()).into()]
                }
                LineChange::Added(j) => {
                    vec![Cell::Empty, Cell::Int(j as i64 + 1), format!("+ {}", lb[j].trim()).into()]
                }
            })
            .collect();
        let hint = if changes.is_empty() { present::SCRIPTS_SAME_HINT } else { present::SCRIPT_DIFF_HINT };
        Ok(Output::Table(Table {
            build,
            total: changes.len(),
            summary,
            offset: self.page.offset,
            columns: vec![col::A, col::B, col::TEXT],
            rows,
            hints: vec![hint.into()],
            ..Table::default()
        }))
    }
}

/// A tag's name above the table: across builds with its build (the tag
/// argument's own syntax), and a copy other than the latest layer's with its
/// module (D89).
fn copy_label(t: &TagCopy<'_>, one_build: bool) -> String {
    let name = t.build.display_name(t.tag);
    let mut label = if one_build { name } else { format!("{}:{name}", t.build.name) };
    if choose_copy(t.build, t.tag, None) != Some(t.copy) {
        label.push_str(&format!(" ({})", t.build.module(t.copy).rel_path));
    }
    label
}

#[cfg(test)]
#[path = "diff_tags_tests.rs"]
mod tests;
