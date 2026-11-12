//! `read_script` (M7, decisions D85): a script tag's script.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::read_tag::title;
use super::{Tool, present};
use crate::catalog::Build;
use crate::output::{Cell, Output, Summary, Table, ToolError, col, marker};
use crate::script::{Form, TagName, read_script, tag_names};
use crate::session::Session;
use crate::stored::copy_identity;
use crate::tag_view::Detail;
use crate::view::TagView;

/// A script tag's script. hsc* (lua): its Lua source, numbered lines, paged.
/// luas (lua_script): compiled Havok Script, listed as the strings it holds
/// (names, text, paths of other scripts and tags); its code isn't decompiled.
/// A line under the tag line counts the tags the script names by path;
/// names=true lists them. The copy in the latest layer; scripts often differ
/// between layers: module= reads another (tag_copies lists them).
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct ReadScript {
    /// The script tag: global ID, tag path or display name.
    pub tag: String,
    /// List the tags the script names by path (each path once, with its first
    /// line), instead of the script.
    #[arg(long)]
    #[serde(default)]
    pub names: bool,
    /// Read the copy in the module whose path contains this.
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

impl Tool for ReadScript {
    const NAME: &'static str = "read_script";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let t = args::tag_copy(s, &self.tag, &self.build, self.module.as_deref())?;
        let script = read_script(t.build, t.tag, t.copy).map_err(present::script_error)?;
        let identity = copy_identity(t.build, t.tag).map_err(present::read_error)?;
        let mut tag = TagView::default();
        title(s.builds(), &t, identity, Detail::Normal, &mut tag);
        if let Form::Text { not_utf8: true, .. } = script.form {
            tag.title_lines.push(present::NOT_UTF8_LINE.into());
        }
        let limit = self.page.limit.unwrap_or(DEFAULT_LIMIT);
        let all = tag_names(t.build, &script);
        let paths = distinct_paths(&all);
        let indexed = paths.iter().filter(|(n, _)| !n.tags.is_empty()).count();
        if self.names {
            return Ok(Output::Tagged {
                tag: Box::new(tag),
                table: names_table(t.build, &paths, all.len(), indexed, &self.page, limit),
            });
        }
        if !paths.is_empty() {
            tag.title_lines.push(present::tag_paths_line(paths.len(), all.len(), indexed));
        }
        let size = Summary { label: "size", key: "size", value: Cell::Size(script.size as u64) };
        let table = match &script.form {
            Form::Text { text, .. } => {
                // Only the lines shown are kept (D26).
                let rows = text
                    .lines()
                    .enumerate()
                    .skip(self.page.offset)
                    .take(limit)
                    .map(|(i, line)| vec![Cell::Int(i as i64 + 1), line.into()])
                    .collect();
                Table {
                    total: text.lines().count(),
                    summary: vec![Summary { label: "form", key: "form", value: "source text".into() }, size],
                    offset: self.page.offset,
                    columns: vec![col::LINE, col::TEXT],
                    rows,
                    ..Table::default()
                }
            }
            Form::Compiled { header, strings } => {
                let hex: Vec<String> = header.iter().map(|b| format!("{b:02x}")).collect();
                Table {
                    total: strings.len(),
                    summary: vec![
                        Summary { label: "form", key: "form", value: "compiled".into() },
                        size,
                        Summary { label: "header", key: "header", value: hex.join(" ").into() },
                    ],
                    offset: self.page.offset,
                    columns: vec![col::TEXT],
                    rows: self.page.page(strings, limit).iter().map(|x| vec![x.as_str().into()]).collect(),
                    hints: vec![present::COMPILED_HINT.into()],
                    ..Table::default()
                }
            }
        };
        Ok(Output::Tagged { tag: Box::new(tag), table })
    }
}

/// Each path once, where it first appears, with how many strings name it
/// (`\\`, `/` and case aside, D91). Keys borrow the names: nothing is
/// allocated per string.
fn distinct_paths<'n, 'b, 's>(all: &'n [TagName<'b, 's>]) -> Vec<(&'n TagName<'b, 's>, usize)> {
    let mut at: HashMap<&str, usize> = HashMap::with_capacity(all.len());
    let mut paths: Vec<(&TagName, usize)> = Vec::new();
    for n in all {
        match at.entry(n.path.as_str()) {
            Entry::Occupied(e) => paths[*e.get()].1 += 1,
            Entry::Vacant(e) => {
                e.insert(paths.len());
                paths.push((n, 1));
            }
        }
    }
    paths
}

/// names=true: one row per tag a path finds (a path in several classes, one
/// row each), or one row for a path no indexed tag has. Only the page's rows
/// are built (D26).
fn names_table(
    b: &Build,
    paths: &[(&TagName, usize)],
    strings: usize,
    indexed: usize,
    page: &PageArgs,
    limit: usize,
) -> Table {
    let line = |n: &TagName| n.line.map_or(Cell::Empty, |l| Cell::Int(l as i64));
    let rows = paths
        .iter()
        .flat_map(|&(n, count)| {
            let found = n.tags.iter().map(move |t| {
                vec![
                    line(n),
                    Cell::Int(count as i64),
                    n.text.into(),
                    t.class.to_string().into(),
                    t.global_id.to_string().into(),
                    b.display_name(t).into(),
                ]
            });
            let missing = n.tags.is_empty().then(|| {
                let not_indexed = marker::NOT_IN_INDEX.into();
                vec![line(n), Cell::Int(count as i64), n.text.into(), Cell::Empty, Cell::Empty, not_indexed]
            });
            found.chain(missing)
        })
        .skip(page.offset)
        .take(limit)
        .collect();
    let int = |n: usize| Cell::Int(n as i64);
    Table {
        total: paths.iter().map(|(n, _)| n.tags.len().max(1)).sum(),
        summary: vec![
            Summary { label: "indexed", key: "indexed", value: int(indexed) },
            Summary { label: "not indexed", key: "not_indexed", value: int(paths.len() - indexed) },
            Summary { label: "strings", key: "strings", value: int(strings) },
        ],
        offset: page.offset,
        columns: vec![col::LINE, col::COUNT, col::TEXT, col::CLASS, col::GLOBAL_ID, col::DISPLAY_NAME],
        rows,
        ..Table::default()
    }
}

#[cfg(test)]
#[path = "read_script_tests.rs"]
mod tests;
