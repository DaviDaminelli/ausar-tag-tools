//! `find_references` (PRD §6.6): what a tag references, and what references
//! it, from the header scan (decisions D50, D51).

use h5_module::{FourCc, GlobalId};
use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::catalog::{Build, Copy, Tag};
use crate::output::{Cell, Column, Output, Part, Summary, Table, ToolError, col, marker};
use crate::query::count_by_class;
use crate::references::{Referrer, ScanIndex, header_scan, path_of, paths_to, reference_paths, referrers, walk_out};
use crate::session::Session;
use crate::stored::read_stored_tag;

/// Which way to look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Tags this tag references.
    #[default]
    Out,
    /// Tags that reference this tag.
    In,
    /// Both, as two parts.
    Both,
}

fn default_max_depth() -> u32 {
    3
}

/// What a tag references (direction=out, with each field path), what
/// references it (in), or both; a change to the tag reaches every tag listed
/// under in. recursive=true follows references out up to
/// max_depth levels; with class=, only tags of that class are listed (the walk
/// still follows every class). References are what tag fields hold: objects a
/// map variant places aren't seen. The first call may build the header scan.
/// Next: read_tag or outline on a listed tag.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct FindReferences {
    /// The tag: global ID, tag path or display name.
    pub tag: String,
    /// out, in or both.
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub direction: Direction,
    /// Follow references out, level by level (direction=out only).
    #[arg(long)]
    #[serde(default)]
    pub recursive: bool,
    /// Levels followed with recursive=true.
    #[arg(long, default_value_t = 3)]
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
    /// Only referenced / referencing tags of this class (code or name).
    #[arg(long)]
    pub class: Option<String>,
    /// For direction=out: the copy in the module whose path contains this.
    #[arg(long)]
    pub module: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

const DEFAULT_LIMIT: usize = 100;

/// A referenced tag's name: its display name, or the name the referencing
/// tag stores, marked, when it isn't indexed (PRD §8).
fn target_name(b: &Build, tag: Option<&Tag>, listed: &str) -> String {
    match tag {
        Some(t) => b.display_name(t),
        None => format!("{listed} {}", marker::NOT_IN_INDEX),
    }
}

/// Field paths named in one `in` row; the `count` column has them all.
const MAX_FIELD_PATHS: usize = 3;

/// `a, b, c (+2 more)`.
fn field_paths_cell(paths: &[String]) -> String {
    let mut text = paths[..paths.len().min(MAX_FIELD_PATHS)].join(", ");
    if paths.len() > MAX_FIELD_PATHS {
        text.push_str(&format!(" (+{} more)", paths.len() - MAX_FIELD_PATHS));
    }
    text
}

/// Copies the scan couldn't read: references from them are missing (said, not hidden).
fn problems_hint(b: &Build, scan: &ScanIndex) -> Option<String> {
    let mut problems = scan.problems();
    let (_, id, why) = problems.next()?;
    let n = 1 + problems.count();
    let name = b.tag_by_id(id).map_or_else(|| id.to_string(), |t| b.display_name(t));
    let copies = if n == 1 {
        "1 tag copy couldn't be scanned".to_owned()
    } else {
        format!("{n} tag copies couldn't be scanned")
    };
    Some(format!("{copies}; references from them are missing here (e.g. {name}: {why})"))
}

impl FindReferences {
    fn table(&self, columns: &[Column], rows: Vec<(FourCc, Vec<Cell>)>) -> Table {
        let summary: Vec<Summary> = present::by_class(count_by_class(rows.iter().map(|r| r.0))).into_iter().collect();
        let all: Vec<Vec<Cell>> = rows.into_iter().map(|(_, r)| r).collect();
        Table {
            total: all.len(),
            summary,
            offset: self.page.offset,
            columns: columns.to_vec(),
            rows: self.page.page(&all, DEFAULT_LIMIT).to_vec(),
            ..Table::default()
        }
    }

    fn out(
        &self,
        b: &Build,
        tag: &Tag,
        copy: Copy,
        scan: &ScanIndex,
        class: Option<FourCc>,
    ) -> Result<Table, ToolError> {
        let t = scan.outgoing(copy).ok_or_else(|| {
            let why = scan.problems().find(|p| p.1 == tag.global_id).map_or("", |p| p.2).to_owned();
            ToolError::new(format!("{} couldn't be scanned: {why}", b.display_name(tag)))
        })?;
        let stored = read_stored_tag(b, copy).map_err(present::read_error)?;
        let paths = reference_paths(b, tag, &stored).map_err(present::definition_error)?;
        let mut found: Vec<(FourCc, String, Vec<Cell>)> = t
            .references
            .iter()
            .filter(|r| class.is_none_or(|c| c == r.class))
            .map(|r| {
                let path = paths.as_ref().and_then(|p| path_of(p, &stored, r)).unwrap_or_default().to_owned();
                let target = b.tag_by_id(r.target);
                let name = target_name(b, target, &t.listed_name(r).unwrap_or_default());
                (r.class, name, vec![path.into(), r.class.to_string().into(), r.target.to_string().into()])
            })
            .collect();
        // By class, then name.
        found.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        let rows = found
            .into_iter()
            .map(|(class, name, mut cells)| {
                cells.push(name.into());
                (class, cells)
            })
            .collect();
        let mut table = self.table(&[col::FIELD_PATH, col::CLASS, col::GLOBAL_ID, col::DISPLAY_NAME], rows);
        if paths.is_none() {
            table.hints.push(present::no_definition(&b.class_name(tag.class)));
        }
        Ok(table)
    }

    fn incoming(&self, b: &Build, target: GlobalId, scan: &ScanIndex, class: Option<FourCc>) -> Table {
        let found: Vec<Referrer<'_>> =
            referrers(b, scan, target).into_iter().filter(|r| class.is_none_or(|c| c == r.tag.class)).collect();
        // Field paths only for the rows shown (D26): each is a read of the referrer.
        let mut no_definition: Vec<FourCc> = Vec::new();
        let rows = self
            .page
            .page(&found, DEFAULT_LIMIT)
            .iter()
            .map(|r| {
                let paths = match paths_to(b, scan, r.tag, r.copy, target) {
                    Ok(Some(paths)) => field_paths_cell(&paths),
                    Ok(None) => {
                        if !no_definition.contains(&r.tag.class) {
                            no_definition.push(r.tag.class);
                        }
                        String::new()
                    }
                    Err(_) => marker::NOT_READ.to_owned(),
                };
                vec![
                    r.tag.class.to_string().into(),
                    r.tag.global_id.to_string().into(),
                    b.display_name(r.tag).into(),
                    format!("{} of {}", r.copies, r.tag.copies.len()).into(),
                    Cell::Int(r.fields as i64),
                    paths.into(),
                ]
            })
            .collect();
        let summary = present::by_class(count_by_class(found.iter().map(|r| r.tag.class))).into_iter().collect();
        let mut hints: Vec<String> = no_definition.iter().map(|c| present::no_definition(&b.class_name(*c))).collect();
        hints.extend(problems_hint(b, scan));
        Table {
            total: found.len(),
            summary,
            offset: self.page.offset,
            columns: vec![col::CLASS, col::GLOBAL_ID, col::DISPLAY_NAME, col::COPIES, col::COUNT, col::FIELD_PATH],
            rows,
            hints,
            ..Table::default()
        }
    }

    fn walk(&self, b: &Build, tag: &Tag, scan: &ScanIndex, class: Option<FourCc>) -> Table {
        let rows: Vec<(FourCc, Vec<Cell>)> = walk_out(b, scan, tag, self.max_depth)
            .into_iter()
            .filter(|s| class.is_none_or(|c| c == s.class))
            .map(|s| {
                let mut name = target_name(b, s.tag, &s.listed_name);
                if s.repeat {
                    name.push(' ');
                    name.push_str(marker::SEE_ABOVE);
                }
                let cells = vec![
                    Cell::Int(s.depth as i64),
                    s.class.to_string().into(),
                    s.target.to_string().into(),
                    name.into(),
                    b.display_name(s.via).into(),
                ];
                (s.class, cells)
            })
            .collect();
        let mut table = self.table(&[col::DEPTH, col::CLASS, col::GLOBAL_ID, col::DISPLAY_NAME, col::VIA], rows);
        table.hints.extend(problems_hint(b, scan));
        table
    }
}

impl Tool for FindReferences {
    const NAME: &'static str = "find_references";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        if self.recursive && self.direction != Direction::Out {
            return Err(ToolError::new("recursive=true works with direction=out only")
                .hint("for what references a tag, use direction=in"));
        }
        let t = args::tag_copy(s, &self.tag, &self.build, self.module.as_deref())?;
        let (b, tag, copy) = (t.build, t.tag, t.copy);
        let class = self.class.as_deref().map(|c| args::class_code(b, c)).transpose()?;
        let (scan, built) = header_scan(b);
        let label = s.label(b);
        let with_built = |mut table: Table| {
            table.summary.extend(present::scan_built(built));
            table
        };
        Ok(match self.direction {
            Direction::Out if self.recursive => {
                Output::Table(Table { build: label, ..with_built(self.walk(b, tag, scan, class)) })
            }
            Direction::Out => Output::Table(Table { build: label, ..with_built(self.out(b, tag, copy, scan, class)?) }),
            Direction::In => {
                Output::Table(Table { build: label, ..with_built(self.incoming(b, tag.global_id, scan, class)) })
            }
            Direction::Both => Output::Parts {
                build: label,
                parts: vec![
                    Part {
                        title: "references out",
                        key: "out",
                        table: with_built(self.out(b, tag, copy, scan, class)?),
                    },
                    Part { title: "references in", key: "in", table: self.incoming(b, tag.global_id, scan, class) },
                ],
            },
        })
    }
}

#[cfg(test)]
#[path = "find_references_tests.rs"]
mod tests;
