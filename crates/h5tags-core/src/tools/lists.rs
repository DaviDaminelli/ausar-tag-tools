//! `list_classes`, `list_modules`, `list_entries`, `list_resource_kinds` (PRD §6.3).

use h5_module::names::latest_layer;
use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::filters::{Filters, KindFilter, Mode, NameFilter, Size, SizeRange, entry_kind};
use crate::output::{Cell, Output, Summary, Table, ToolError, col};
use crate::query::{self, EntryScope, EntrySort, ModuleStatus};
use crate::session::Session;

/// Every class in the build: code, name, tag count, and whether a definition
/// exists (without one, tags can be listed but not read field by field).
/// Classes with a definition but no tags in the indexed modules are listed
/// with tags 0. query= keeps the classes whose code or name contains it
/// (sound, weap).
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct ListClasses {
    /// Part of a class code or name, any case.
    #[arg(long)]
    pub query: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
}

impl Tool for ListClasses {
    const NAME: &'static str = "list_classes";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build(s, &self.build)?;
        let found = query::classes(b, self.query.as_deref());
        let definition_only = found.iter().filter(|c| c.tags == 0).count();
        let rows: Vec<Vec<Cell>> = found
            .iter()
            .map(|c| vec![c.code.to_string().into(), c.name.into(), Cell::Int(c.tags as i64), Cell::Bool(c.definition)])
            .collect();
        let summary = (definition_only > 0)
            .then_some(Summary {
                label: "definition only",
                key: "definition_only",
                value: Cell::Int(definition_only as i64),
            })
            .into_iter()
            .collect();
        let hints = match &self.query {
            Some(q) if rows.is_empty() => vec![present::no_class_matches(q)],
            _ => Vec::new(),
        };
        Ok(Output::Table(Table {
            build: s.label(b),
            total: rows.len(),
            summary,
            columns: vec![col::CLASS, col::CLASS_NAME, col::TAGS, col::DEFINITION],
            rows,
            hints,
            ..Table::default()
        }))
    }
}

/// Every module of the build: path, version, tag count, whether it has a load
/// manifest, and its status (`ok`, or `!!` if it is damaged or incomplete);
/// above them, the build's formats and its latest game version.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct ListModules {
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

impl Tool for ListModules {
    const NAME: &'static str = "list_modules";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build(s, &self.build)?;
        let statuses: Vec<ModuleStatus> = b.index.modules.iter().map(ModuleStatus::of).collect();
        let damaged = statuses.iter().filter(|st| **st != ModuleStatus::Ok).count() + b.index.failed.len();
        let indexed = b.index.modules.iter().zip(&statuses).map(|(m, status)| {
            vec![
                m.rel_path.clone().into(),
                Cell::Int(m.version as i64),
                Cell::Int(m.tag_count() as i64),
                Cell::Bool(m.has_loadmanifest()),
                present::module_status(status).into(),
            ]
        });
        let failed = b.index.failed.iter().map(|f| {
            vec![
                f.rel_path.clone().into(),
                Cell::Empty,
                Cell::Empty,
                Cell::Empty,
                present::not_indexed(&f.error).into(),
            ]
        });
        let rows: Vec<Vec<Cell>> = indexed.chain(failed).collect();
        // Which build this is (D63): its formats and newest game version.
        let latest = latest_layer(b.index.modules.iter().map(|m| m.rel_path.as_str()));
        let summary = vec![
            Summary {
                label: "formats",
                key: "formats",
                value: present::format_names(b.index.modules.iter().map(|m| m.version)).into(),
            },
            Summary {
                label: "latest game version",
                key: "latest_game_version",
                value: latest.map_or_else(|| "base".to_owned(), present::layer_text).into(),
            },
        ];
        Ok(Output::Table(Table {
            build: s.label(b),
            total: rows.len(),
            summary,
            offset: self.page.offset,
            columns: vec![col::MODULE, col::VERSION, col::TAGS, col::LOAD_MANIFEST, col::STATUS],
            rows: self.page.page(&rows, 100).to_vec(),
            path_column: Some(0),
            wide_column: Some(0),
            hints: (damaged > 0).then(|| present::damaged_modules_hint(damaged)).into_iter().collect(),
            ..Table::default()
        }))
    }
}

/// File entries (tags, resources, other files) with kind and size, from the
/// index only: nothing is decompressed. Filters combine. Use it to see which
/// assets a tag has, or to find assets by kind and size.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct ListEntries {
    /// Only this tag and its resources: global ID, tag path or display name.
    #[arg(long)]
    pub tag: Option<String>,
    /// Name match: the tag path (for a resource, its owner's tag path).
    #[arg(long)]
    pub query: Option<String>,
    /// default = contains; regex = Rust regex; exact = equals.
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub mode: Mode,
    /// Entry kind: a class code or name (tags), a resource kind (e.g. "mesh resource"), or "file".
    #[arg(long)]
    pub kind: Option<String>,
    /// Only entries in modules whose path contains this.
    #[arg(long)]
    pub module: Option<String>,
    /// Minimum uncompressed size, e.g. 500KB, 10MB.
    #[arg(long)]
    pub min_size: Option<Size>,
    /// Maximum uncompressed size, e.g. 500KB, 10MB.
    #[arg(long)]
    pub max_size: Option<Size>,
    /// name, or size (largest first).
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub sort: EntrySort,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

impl Tool for ListEntries {
    const NAME: &'static str = "list_entries";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let module = args::module_filter(self.module.as_deref());
        let mut hints = Vec::new();
        let (b, scope) = match &self.tag {
            None => (args::build(s, &self.build)?, EntryScope::All),
            Some(text) => {
                let t = args::tag_copy(s, text, &self.build, self.module.as_deref())?;
                hints.extend(t.other_copies_hint);
                (t.build, EntryScope::Tag(t.copy))
            }
        };
        let filters = Filters {
            name: self.query.as_deref().map(|q| args::name_matcher(q, self.mode)).transpose()?.map(NameFilter::new),
            kind: self.kind.as_deref().map(|k| KindFilter::new(b, k)),
            module,
            size: SizeRange { min: self.min_size, max: self.max_size },
            ..Filters::default()
        };
        let hits = query::entries(b, scope, &filters, self.sort);

        if hits.iter().any(|h| !h.entry.stored) {
            hints.push(present::not_stored_hint());
        }
        if let (true, Some(kind), Some(text)) = (hits.is_empty(), &filters.kind, &self.kind)
            && !kind.is_known_kind()
            && !query::any_resource_matches(b, &filters)
        {
            hints.push(present::unknown_kind_hint(b, text));
        }

        let rows = self
            .page
            .page(&hits, 100)
            .iter()
            .map(|h| {
                let e = h.entry;
                let marks = filters.name.as_ref().map_or_else(Vec::new, |n| {
                    let (path, class) = e.owner_parts();
                    n.matcher.find(path, class)
                });
                vec![
                    e.kind().as_str().into(),
                    Cell::marked(e.name.clone(), marks),
                    entry_kind(e).into(),
                    Cell::Size(e.uncompressed_size),
                    present::compressed_size(e),
                    h.module.rel_path.clone().into(),
                ]
            })
            .collect();
        Ok(Output::Table(Table {
            build: s.label(b),
            total: hits.len(),
            summary: vec![Summary {
                label: "total size",
                key: "total_size",
                value: Cell::Size(hits.iter().map(|h| h.entry.uncompressed_size).sum()),
            }],
            offset: self.page.offset,
            columns: vec![col::ENTRY_TYPE, col::NAME, col::KIND, col::SIZE, col::COMPRESSED, col::MODULE],
            rows,
            path_column: Some(1),
            wide_column: Some(1),
            hints,
            ..Table::default()
        }))
    }
}

/// Every resource (asset) kind in the build, with its entry count and total
/// uncompressed size. Next: list_entries(kind=...) to see the entries.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct ListResourceKinds {
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
}

impl Tool for ListResourceKinds {
    const NAME: &'static str = "list_resource_kinds";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build(s, &self.build)?;
        let rows: Vec<Vec<Cell>> = query::resource_kinds(b)
            .into_iter()
            .map(|k| vec![k.kind.into(), Cell::Int(k.count as i64), Cell::Size(k.bytes)])
            .collect();
        Ok(Output::Table(Table {
            build: s.label(b),
            total: rows.len(),
            columns: vec![col::KIND, col::COUNT, col::TOTAL_SIZE],
            rows,
            ..Table::default()
        }))
    }
}
