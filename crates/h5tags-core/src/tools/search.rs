//! `search_tags` and `browse` (PRD §6.3).

use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::catalog::ClassLookup;
use crate::filters::{Filters, Mode, NameFilter};
use crate::output::{Cell, Column, Output, Summary, Table, ToolError, col};
use crate::query;
use crate::session::Session;

/// Find tags by part of their path, by class, or by module. One row per tag.
/// Next: list_entries(tag=...) for its assets, or browse its folder.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct SearchTags {
    /// Text to find. mode=default: part of the tag path, or a class code/name.
    pub query: String,
    /// Only tags of this class (code or name, e.g. weap or weapon).
    #[arg(long)]
    pub class: Option<String>,
    /// Only tags with a copy in a module whose path contains this.
    #[arg(long)]
    pub module: Option<String>,
    /// default = path contains query, or class equals query; regex = Rust regex on the path; exact = path or display name equals query.
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub mode: Mode,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

const SEARCH_COLUMNS: [Column; 4] = [col::GLOBAL_ID, col::CLASS, col::DISPLAY_NAME, col::COPIES];

impl Tool for SearchTags {
    const NAME: &'static str = "search_tags";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build(s, &self.build)?;
        let class = match self.class.as_deref().map(|c| (c, b.lookup_class(c))) {
            None => None,
            Some((_, ClassLookup::Found(info))) => Some(info.code),
            Some((query, absent)) => {
                return Ok(Output::Table(Table {
                    build: s.label(b),
                    columns: SEARCH_COLUMNS.to_vec(),
                    hints: vec![present::absent_class_hint(b, query, &absent)],
                    ..Table::default()
                }));
            }
        };
        let name = NameFilter::reclaimer(b, args::name_matcher(&self.query, self.mode)?);
        let filters = Filters {
            name: Some(name.clone()),
            class,
            module: args::module_filter(self.module.as_deref()),
            ..Filters::default()
        };
        let hits = query::search_tags(b, &filters);

        // An overview to narrow a broad search with class= in one step.
        let summary: Vec<Summary> =
            present::by_class(query::count_by_class(hits.iter().map(|t| t.class))).into_iter().collect();
        // An empty result says why (D22).
        let mut hints = Vec::new();
        if hits.is_empty() {
            let name_only = query::search_tags(b, &Filters { name: Some(name), ..Filters::default() }).len();
            hints.push(if name_only > 0 {
                present::filtered_out_hint(name_only)
            } else {
                present::no_match_hint(&self.query, self.mode, &query::similar_names(b, &self.query, 5))
            });
        }

        let rows = self
            .page
            .page(&hits, 50)
            .iter()
            .map(|t| {
                let display = b.display_name(t);
                let marks =
                    filters.name.as_ref().map_or_else(Vec::new, |n| n.matcher.find(&t.path, &b.class_name(t.class)));
                vec![
                    t.global_id.to_string().into(),
                    t.class.to_string().into(),
                    Cell::marked(display, marks),
                    Cell::Int(t.copies.len() as i64),
                ]
            })
            .collect();
        Ok(Output::Table(Table {
            build: s.label(b),
            total: hits.len(),
            summary,
            offset: self.page.offset,
            columns: SEARCH_COLUMNS.to_vec(),
            rows,
            path_column: Some(2),
            wide_column: Some(2),
            hints,
            ..Table::default()
        }))
    }
}

/// Explore tag paths like folders: the subfolders of a folder (with how many
/// tags each holds) and the tags directly in it. Next: browse a subfolder, or
/// list_entries(tag=...) for a tag's assets.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct Browse {
    /// Folder, `\`-separated, case-insensitive (empty = the top level).
    #[arg(default_value = "")]
    #[serde(default)]
    pub path: String,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

impl Tool for Browse {
    const NAME: &'static str = "browse";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build(s, &self.build)?;
        let folder = self.path.trim_matches('\\');
        let Some(listing) = query::browse(b, folder) else {
            return Err(ToolError::new(format!("no folder '{folder}'"))
                .hint("browse its parent folder, or search_tags for part of the name"));
        };
        let folder_rows = listing
            .subfolders
            .iter()
            .map(|f| vec![format!("{}\\", f.name).into(), Cell::Empty, Cell::Empty, Cell::Int(f.tag_count as i64)]);
        let tag_rows = listing.tags.iter().map(|t| {
            vec![
                format!("{}.{}", &t.path[listing.prefix_len..], b.class_name(t.class)).into(),
                t.class.to_string().into(),
                t.global_id.to_string().into(),
                Cell::Empty,
            ]
        });
        let all: Vec<Vec<Cell>> = folder_rows.chain(tag_rows).collect();
        let folder_label = if folder.is_empty() { "(top)".to_owned() } else { format!("{folder}\\") };
        Ok(Output::Table(Table {
            build: s.label(b),
            total: all.len(),
            summary: vec![
                Summary { label: "folder", key: "folder", value: folder_label.into() },
                Summary { label: "subfolders", key: "subfolders", value: Cell::Int(listing.subfolders.len() as i64) },
                Summary { label: "tags here", key: "tags_here", value: Cell::Int(listing.tags.len() as i64) },
            ],
            offset: self.page.offset,
            columns: vec![col::NAME, col::CLASS, col::GLOBAL_ID, col::TAGS],
            wide_column: Some(0),
            rows: self.page.page(&all, 100).to_vec(),
            ..Table::default()
        }))
    }
}
