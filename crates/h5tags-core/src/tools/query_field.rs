//! `query_field` (PRD §6.6): one field across every tag of a class.

use std::borrow::Cow;

use h5_defs::definition::FieldKind;
use h5_meta::{Content, FieldPath, Select, Selected, parse_path, select};
use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::field_query::{ClassPath, Sort, also_at, check_also, query_field, value_field};
use crate::inherit::{Inheriting, inherited_field, inheriting_classes};
use crate::output::{Cell, Output, Summary, Table, ToolError, col};
use crate::query::choose_copy;
use crate::query::count_by_class;
use crate::session::Session;
use crate::stored::{load_class_definition, read_stored_tag};
use crate::tag_view::{BuildLookup, Detail, Lookup, Options, ViewSettings, view_selection};
use crate::view::{ViewNode, value_column};

/// The same field in every tag of a class: one row per tag (path with [*]:
/// one per entry, nested too: node graphs[*].nodes[*].screen wrapper), from
/// each tag's copy in the latest layer. filter: '<op> <value>' with
/// == != > >= < <= contains (enums by option label: '== none' is the option
/// named none; flags: contains <option>; tag references by name), or 'is set'
/// / 'is empty' ('== (none)' = a null reference). sort: none, asc, desc. A tag
/// block path without an index gives its entry count. also: more fields per
/// row, one column each (not filtered or sorted). Tags that can't be read are
/// listed as skipped. A parent class (object, item, unit, device) gives the
/// field in every class inheriting it, with a class column. Next: read_tag on
/// a row's tag.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct QueryField {
    /// Class code or name (weap or weapon); a parent class (object) for every class inheriting it.
    pub class: String,
    /// The field: `magazines[0].rounds loaded maximum`, `barrels[*].rounds per second`; `barrels` = entry count.
    pub path: String,
    /// Keep only rows whose value passes: '>= 30', '== latch', 'contains plasma', 'is set'.
    #[arg(long)]
    pub filter: Option<String>,
    /// none (tag path order), asc or desc (by value).
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub sort: Sort,
    /// More fields shown on each row: ["barrels[*].heat generated per round", "magazines"]. A [*] in one must be one path has (the row's entry).
    #[arg(long)]
    #[serde(default)]
    pub also: Vec<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

const DEFAULT_LIMIT: usize = 100;

impl Tool for QueryField {
    const NAME: &'static str = "query_field";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build(s, &self.build)?;
        let class = args::class_code(b, &self.class)?;
        let Some(def) = load_class_definition(b, class).map_err(present::definition_error)? else {
            return Err(ToolError::new(present::no_definition(&b.class_name(class))));
        };
        // D101: a class without tags may be a parent (`object`): it covers
        // every class inheriting it, its definition the reference. A class
        // with tags is queried alone, without looking at other definitions
        // (every parent of both samples has no tags).
        let inheriting =
            if b.classes.contains_key(&class) { Inheriting::default() } else { inheriting_classes(b, class) };
        let parent = !inheriting.classes.is_empty();
        let classes = &inheriting.classes;
        let parent_name = b.class_name(class);
        // One path per class queried, in `classes`' order.
        let class_paths = |text: &str| -> Result<Vec<ClassPath>, ToolError> {
            if parent {
                return inherited_field(b, &def, classes, text)
                    .map_err(|e| present::inherit_error(&parent_name, text, e));
            }
            let path = parse_path(&def.fields, text).map_err(present::path_error)?;
            Ok(vec![ClassPath { class, def: def.clone(), path }])
        };
        let parts = class_paths(&self.path)?;
        let path = parse_path(&def.fields, &self.path).map_err(present::path_error)?;
        let shown = path.display(&def.fields);
        let field = value_field(&def.fields, &path, &shown).map_err(present::query_error)?;
        // D75: the also paths, each checked like path, and against it; one per class (D101).
        let mut also = Vec::with_capacity(self.also.len());
        for text in &self.also {
            let p = parse_path(&def.fields, text).map_err(present::path_error)?;
            let name = p.display(&def.fields);
            value_field(&def.fields, &p, &name).map_err(present::query_error)?;
            check_also(&p, &path, &def.fields).map_err(present::query_error)?;
            let star = p.steps.iter().any(|st| st.select == Some(Select::Every));
            let per_class: Vec<FieldPath> = class_paths(text)?.into_iter().map(|c| c.path).collect();
            also.push((per_class, name, star));
        }
        let filter = self.filter.as_deref().map(args::field_filter).transpose()?;
        let found = query_field(b, &parts, &shown, filter.as_ref(), self.sort).map_err(present::query_error)?;

        // Display text only for the rows shown (D26): their tags are read
        // again (from the tag cache) and their values rendered as read_tag does.
        let several = path.steps.iter().any(|st| st.select == Some(Select::Every));
        // A row's own entries matter only to also paths with a [*].
        let needs_row_path = also.iter().any(|(_, _, star)| *star);
        let settings = ViewSettings {
            depth: 0,
            detail: Detail::Brief,
            options: Options::None,
            show_hidden: true,
            guid_check: s.guid_check,
        };
        let mut rows = Vec::new();
        for r in self.page.page(&found.rows, DEFAULT_LIMIT) {
            // The row's class: its definition and paths (a handful of classes).
            let Some(i) = parts.iter().position(|p| p.class == r.tag.class) else { continue };
            let ClassPath { def, path, .. } = &parts[i];
            let Some(copy) = choose_copy(b, r.tag, None) else { continue };
            let stored = read_stored_tag(b, copy).map_err(present::read_error)?;
            let read = h5_meta::read_tag(def, &stored.header, &stored.tag_data);
            let sels = select(&read.root, path).map_err(present::path_error)?;
            let Some(sel) = sels.get(r.selection) else { continue };
            let lookup = BuildLookup { build: b, copy };
            let mut cells = Vec::with_capacity(3 + also.len());
            if parent {
                cells.push(r.tag.class.to_string().into());
            }
            cells.push(b.display_name(r.tag).into());
            if several {
                cells.push(sel.path.clone().into());
            }
            cells.push(value_cell(sel, &shown, settings, &lookup));
            let row_path = if needs_row_path { parse_path(&def.fields, &sel.path).ok() } else { None };
            for (per_class, name, star) in &also {
                let p = &per_class[i];
                // Copied only where a [*] takes the row's entry.
                let at = match &row_path {
                    Some(row) if *star => Cow::Owned(also_at(p, row)),
                    _ => Cow::Borrowed(p),
                };
                cells.push(match select(&read.root, &at) {
                    Ok(found) => found.first().map_or(Cell::Empty, |a| value_cell(a, name, settings, &lookup)),
                    Err(e) => present::missing_value(&e).into(),
                });
            }
            rows.push(cells);
        }
        let mut columns = if parent { vec![col::CLASS, col::DISPLAY_NAME] } else { vec![col::DISPLAY_NAME] };
        if several {
            columns.push(col::FIELD_PATH);
        }
        columns.push(col::VALUE);
        let angle = matches!(field.kind, FieldKind::Known(t) if t.is_angle());
        // Angles say `rad` in their values; the definition's "degrees" would contradict them.
        let units = field.name.units.as_ref().filter(|_| !angle);
        let mut summary: Vec<Summary> =
            units.map(|u| Summary { label: "units", key: "units", value: u.clone().into() }).into_iter().collect();
        let mut hints: Vec<String> = Vec::new();
        if parent {
            // Most first: the first is the class most rows come from.
            let counts = count_by_class(found.rows.iter().map(|r| r.tag.class));
            if found.rows.len() > self.page.limit.unwrap_or(DEFAULT_LIMIT)
                && let Some(&(most, n)) = counts.first()
            {
                hints.push(present::many_parent_rows(found.rows.len(), most, n));
            }
            summary.push(present::inherited_by(classes));
            summary.extend(present::by_class_up_to(counts, classes.len()));
        }
        hints.extend(present::skipped_hint(b, &found.skipped));
        hints.extend(present::unreadable_definitions(&inheriting.unreadable));
        if !parent && !b.classes.contains_key(&class) {
            hints.push(present::absent_class_hint(b, &self.class, &b.lookup_class(&self.class)));
        }
        Ok(Output::Table(Table {
            build: s.label(b),
            total: found.rows.len(),
            summary,
            offset: self.page.offset,
            columns,
            named_columns: also.into_iter().map(|(_, name, _)| name).collect(),
            rows,
            hints,
            ..Table::default()
        }))
    }
}

/// One selected value's cell, as `read_tag` writes it (brief); a tag block's
/// entry count as a number (D75).
fn value_cell(sel: &Selected<'_, '_>, shown: &str, settings: ViewSettings, lookup: &dyn Lookup) -> Cell {
    if let (Content::Block(b), None) = (&sel.node.content, sel.entry) {
        return Cell::Int(i64::from(b.count));
    }
    let view = view_selection(std::slice::from_ref(sel), shown, settings, lookup);
    let value = view.nodes.iter().find_map(|n| match n {
        ViewNode::Field(f) => Some(value_column(f)),
        _ => None,
    });
    value.unwrap_or_default().into()
}

#[cfg(test)]
#[path = "query_field_tests.rs"]
mod tests;
