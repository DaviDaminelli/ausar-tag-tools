//! `query_field` (PRD §6.6): one field across every tag of a class.

use h5_defs::definition::FieldKind;
use h5_meta::{Select, parse_path, select};
use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::field_query::{Sort, query_field, value_field};
use crate::output::{Output, Summary, Table, ToolError, col};
use crate::query::choose_copy;
use crate::session::Session;
use crate::stored::read_stored_tag;
use crate::tag_view::{BuildLookup, Detail, Options, ViewSettings, view_selection};
use crate::view::{ViewNode, value_column};

/// The same field in every tag of a class: one row per tag (path with [*]:
/// one per entry), from each tag's first copy. filter: '<op> <value>' with
/// == != > >= < <= contains (enums by label, flags: contains <option>, tag
/// references by name). sort: none, asc, desc. Tags that can't be read are
/// listed as skipped. Next: read_tag on a row's tag.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct QueryField {
    /// Class code or name (weap or weapon).
    pub class: String,
    /// The field: `magazines[0].rounds loaded maximum`, `barrels[*].rounds per second`.
    pub path: String,
    /// Keep only rows whose value passes: '>= 30', '== latch', 'contains plasma'.
    #[arg(long)]
    pub filter: Option<String>,
    /// none (tag path order), asc or desc (by value).
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub sort: Sort,
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
        let Some(file) = b.classes.get(&class).and_then(|c| c.definition.clone()) else {
            return Err(ToolError::new(present::no_definition(&b.class_name(class))));
        };
        let def = b.store.definition(&file).map_err(present::definition_error)?;
        let path = parse_path(&def.fields, &self.path).map_err(present::path_error)?;
        let shown = path.display(&def.fields);
        let field = value_field(&def.fields, &path, &shown).map_err(present::query_error)?;
        let filter = self.filter.as_deref().map(args::field_filter).transpose()?;
        let found =
            query_field(b, class, &def, &path, &shown, filter.as_ref(), self.sort).map_err(present::query_error)?;

        // Display text only for the rows shown (D26): their tags are read
        // again (from the tag cache) and their values rendered as read_tag does.
        let several = path.steps.iter().any(|st| st.select == Some(Select::Every));
        let settings = ViewSettings {
            depth: 0,
            detail: Detail::Brief,
            options: Options::None,
            show_hidden: true,
            guid_check: s.guid_check,
        };
        let mut rows = Vec::new();
        for r in self.page.page(&found.rows, DEFAULT_LIMIT) {
            let Some(copy) = choose_copy(b, r.tag, None) else { continue };
            let stored = read_stored_tag(b, copy).map_err(present::read_error)?;
            let read = h5_meta::read_tag(&def, &stored.header, &stored.tag_data);
            let sels = select(&read.root, &path).map_err(present::path_error)?;
            let Some(sel) = sels.get(r.selection) else { continue };
            let lookup = BuildLookup { build: b, copy };
            let view = view_selection(std::slice::from_ref(sel), &shown, settings, &lookup);
            let value = view.nodes.iter().find_map(|n| match n {
                ViewNode::Field(f) => Some(value_column(f)),
                _ => None,
            });
            let mut cells = vec![b.display_name(r.tag).into()];
            if several {
                cells.push(sel.path.clone().into());
            }
            cells.push(value.unwrap_or_default().into());
            rows.push(cells);
        }
        let mut columns = vec![col::DISPLAY_NAME];
        if several {
            columns.push(col::FIELD_PATH);
        }
        columns.push(col::VALUE);
        let angle = matches!(field.kind, FieldKind::Known(t) if t.is_angle());
        // Angles say `rad` in their values; the definition's "degrees" would contradict them.
        let units = field.name.units.as_ref().filter(|_| !angle);
        let summary =
            units.map(|u| Summary { label: "units", key: "units", value: u.clone().into() }).into_iter().collect();
        Ok(Output::Table(Table {
            build: s.label(b),
            total: found.rows.len(),
            summary,
            offset: self.page.offset,
            columns,
            rows,
            hints: present::skipped_hint(b, &found.skipped).into_iter().collect(),
            ..Table::default()
        }))
    }
}

#[cfg(test)]
#[path = "query_field_tests.rs"]
mod tests;
