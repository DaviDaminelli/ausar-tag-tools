//! `list_classes`, `list_modules`, `list_entries`, `list_resource_kinds` (PRD §6.3).

use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::filters::{Filters, KindFilter, Mode, NameFilter, Size, SizeRange, entry_kind};
use crate::output::{Cell, Output, Summary, Table, ToolError, col, marker};
use crate::query::{self, EntryScope, EntrySort, ModuleStatus};
use crate::session::Session;

/// Every class in the build: code, name, tag count, and whether a definition
/// exists (without one, tags can be listed but not read field by field).
#[derive(Clone, Debug, clap::Args, Deserialize)]
pub struct ListClasses {
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
}

impl Tool for ListClasses {
    const NAME: &'static str = "list_classes";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build(s, &self.build)?;
        let rows: Vec<Vec<Cell>> = b
            .classes
            .values()
            .map(|c| {
                vec![
                    c.code.to_string().into(),
                    c.name.clone().into(),
                    Cell::Int(c.tag_count as i64),
                    Cell::Bool(c.definition.is_some()),
                ]
            })
            .collect();
        Ok(Output::Table(Table {
            build: s.label(b),
            total: rows.len(),
            columns: vec![col::CLASS, col::CLASS_NAME, col::TAGS, col::DEFINITION],
            rows,
            ..Table::default()
        }))
    }
}

/// Every module of the build: path, version, tag count, whether it has a load
/// manifest, and its status (`ok`, or `!!` if it is damaged or incomplete).
#[derive(Clone, Debug, clap::Args, Deserialize)]
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
        Ok(Output::Table(Table {
            build: s.label(b),
            total: rows.len(),
            offset: self.page.offset,
            columns: vec![col::MODULE, col::VERSION, col::TAGS, col::LOAD_MANIFEST, col::STATUS],
            rows: self.page.page(&rows, 100).to_vec(),
            path_column: Some(0),
            hints: (damaged > 0).then(|| present::damaged_modules_hint(damaged)).into_iter().collect(),
            ..Table::default()
        }))
    }
}

/// File entries (tags, resources, other files) with kind and size, from the
/// index only: nothing is decompressed. Filters combine. Use it to see which
/// assets a tag has, or to find assets by kind and size.
#[derive(Clone, Debug, clap::Args, Deserialize)]
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
                let (b, tag) = args::tag(s, text, &self.build)?;
                let copy = query::choose_copy(b, tag, module.as_ref()).ok_or_else(|| {
                    let filter = self.module.as_deref().unwrap_or_default();
                    ToolError::new(format!("{} has no copy in a module matching '{filter}'", b.display_name(tag)))
                        .hint("list_entries without module= lists its modules")
                })?;
                if tag.copies.len() > 1 {
                    hints.push(present::other_copies_hint(tag.copies.len(), &b.module(copy).rel_path));
                }
                (b, EntryScope::Tag(copy))
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
                vec![
                    e.kind().as_str().into(),
                    e.name.clone().into(),
                    entry_kind(e).into(),
                    Cell::Size(e.uncompressed_size),
                    if e.stored { Cell::Size(e.compressed_size as u64) } else { marker::NOT_STORED.into() },
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
            hints,
        }))
    }
}

/// Every resource (asset) kind in the build, with its entry count and total
/// uncompressed size. Next: list_entries(kind=...) to see the entries.
#[derive(Clone, Debug, clap::Args, Deserialize)]
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
