//! `search_scripts` (M7, decisions D85, D88): the script lines holding a text.

use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::filters::Substring;
use crate::output::{Cell, Output, Summary, Table, ToolError, col};
use crate::script::{script_field, search_scripts};
use crate::session::Session;

/// Find text in every script, case-insensitive: hsc* source lines (with
/// their line number) and the strings of compiled luas scripts (functions,
/// events, text, paths). Each script's copy in the latest layer. Next:
/// read_script(tag, offset=<line - 1>) for the lines around a hit.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct SearchScripts {
    /// Text to find (part of a line).
    pub term: String,
    /// Only this script class: hsc* (lua) or luas (lua_script).
    #[arg(long)]
    pub class: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

const DEFAULT_LIMIT: usize = 50;

impl Tool for SearchScripts {
    const NAME: &'static str = "search_scripts";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let term = self.term.trim();
        if term.is_empty() {
            return Err(ToolError::new("the term is empty").hint("pass part of a line, e.g. a function or event name"));
        }
        let b = args::build(s, &self.build)?;
        let class = self.class.as_deref().map(|c| args::class_code(b, c)).transpose()?;
        if let Some(c) = class.filter(|&c| script_field(c).is_none()) {
            return Err(present::not_a_script_class(c));
        }
        let finder = Substring::new(term);
        let found = search_scripts(b, class, &|text| finder.is_match(text));
        let scripts = found.hits.chunk_by(|x, y| std::ptr::eq(x.tag, y.tag)).count();
        let rows = self
            .page
            .page(&found.hits, DEFAULT_LIMIT)
            .iter()
            .map(|h| {
                vec![
                    h.tag.class.to_string().into(),
                    h.tag.global_id.to_string().into(),
                    b.display_name(h.tag).into(),
                    h.line.map_or(Cell::Empty, |l| Cell::Int(l as i64)),
                    h.text.as_str().into(),
                ]
            })
            .collect();
        let mut hints: Vec<String> = present::skipped_scripts(&found.skipped).into_iter().collect();
        if found.hits.is_empty() {
            hints.push(present::no_script_holds(term));
        }
        Ok(Output::Table(Table {
            build: s.label(b),
            total: found.hits.len(),
            summary: vec![
                Summary { label: "scripts", key: "scripts", value: Cell::Int(scripts as i64) },
                Summary { label: "searched", key: "searched", value: Cell::Int(found.searched as i64) },
            ],
            offset: self.page.offset,
            columns: vec![col::CLASS, col::GLOBAL_ID, col::DISPLAY_NAME, col::LINE, col::TEXT],
            rows,
            path_column: Some(2),
            hints,
            ..Table::default()
        }))
    }
}

#[cfg(test)]
#[path = "search_scripts_tests.rs"]
mod tests;
