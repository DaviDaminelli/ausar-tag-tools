//! `map_tags` and `which_maps` (PRD §6.6): a map's load manifest.

use h5_module::names::module_layer;
use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::maps::{map_tags, pick_manifest, which_maps};
use crate::output::{Cell, Column, Output, Summary, Table, ToolError, col};
use crate::query;
use crate::session::Session;
use crate::stored::read_manifest;

/// The tags a map loads: those its module's load manifest lists, by class.
/// module= a map's module (part of its path, e.g. mainmenu); for a map with
/// update layers, the latest layer's manifest (each layer's is a full list).
/// IDs that aren't indexed tags are only counted. Next: read_tag on a row.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct MapTags {
    /// The map's module: part of its path (`mainmenu`), or a whole module path for one layer.
    pub module: String,
    /// Only tags of this class (code or name).
    #[arg(long)]
    pub class: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

const TAG_COLUMNS: [Column; 3] = [col::GLOBAL_ID, col::CLASS, col::DISPLAY_NAME];

impl Tool for MapTags {
    const NAME: &'static str = "map_tags";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build(s, &self.build)?;
        let class = self.class.as_deref().map(|c| args::class_code(b, c)).transpose()?;
        let filter = args::module_filter(Some(&self.module)).expect("module= is given");
        let module =
            pick_manifest(b, |path| filter.matches(path)).map_err(|e| present::pick_manifest_error(&self.module, e))?;
        let found = map_tags(b, module, class).map_err(present::read_error)?;
        let mut summary = vec![
            Summary {
                label: "manifest",
                key: "manifest",
                value: b.index.modules[module as usize].rel_path.clone().into(),
            },
            Summary { label: "not in indexed modules", key: "not_indexed", value: Cell::Int(found.not_indexed as i64) },
        ];
        summary.extend(present::by_class(query::count_by_class(found.tags.iter().map(|t| t.class))));
        let rows = self
            .page
            .page(&found.tags, 100)
            .iter()
            .map(|t| vec![t.global_id.to_string().into(), t.class.to_string().into(), b.display_name(t).into()])
            .collect();
        Ok(Output::Table(Table {
            build: s.label(b),
            total: found.tags.len(),
            summary,
            offset: self.page.offset,
            columns: TAG_COLUMNS.to_vec(),
            rows,
            path_column: Some(2),
            wide_column: Some(2),
            ..Table::default()
        }))
    }
}

/// The maps that load a tag: the maps whose load manifest in effect (each
/// map's latest layer) lists it, with each manifest's tag count. Next:
/// map_tags(module=...) for a map's other tags.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct WhichMaps {
    /// The tag: global ID, tag path or display name.
    pub tag: String,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
}

impl Tool for WhichMaps {
    const NAME: &'static str = "which_maps";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let (b, tag) = args::tag(s, &self.tag, &self.build)?;
        let found = which_maps(b, tag.global_id);
        let mut rows = Vec::with_capacity(found.modules.len());
        for &m in &found.modules {
            let path = &b.index.modules[m as usize].rel_path;
            let count = read_manifest(b, m).map_err(present::read_error)?.map_or(0, |ids| ids.len());
            rows.push(vec![
                path.clone().into(),
                present::layer_text(module_layer(path)).into(),
                Cell::Int(count as i64),
            ]);
        }
        let mut hints = Vec::new();
        if found.modules.is_empty() && found.failed.is_empty() {
            hints.push(present::no_map_hint(found.checked));
        }
        if !found.failed.is_empty() {
            hints.push(present::unread_manifests_hint(b, &found.failed));
        }
        Ok(Output::Table(Table {
            build: s.label(b),
            total: rows.len(),
            columns: vec![col::MODULE, col::LAYER, col::TAGS],
            rows,
            hints,
            ..Table::default()
        }))
    }
}

#[cfg(test)]
#[path = "maps_tests.rs"]
mod tests;
