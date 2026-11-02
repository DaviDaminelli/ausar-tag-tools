//! `search_values` (PRD §6.6): tags by the strings they hold.

use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::output::{Output, Summary, Table, ToolError, col};
use crate::query::count_by_class;
use crate::references::{header_scan, search_strings};
use crate::session::Session;

/// Find tags by text they hold: string_id names (sound events, markers,
/// variant names…) and the names of the tags they reference, case-insensitive.
/// Each hit lists the strings that matched. Not field values: use read_tag for
/// those. The first call may build the header scan. Next: read_tag or
/// find_references on a hit.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct SearchValues {
    /// Text to find (part of a string).
    pub term: String,
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

const DEFAULT_LIMIT: usize = 50;
/// Matched strings shown per tag; the rest are counted.
const MAX_STRINGS: usize = 4;

impl Tool for SearchValues {
    const NAME: &'static str = "search_values";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let term = self.term.trim();
        if term.is_empty() {
            return Err(ToolError::new("the term is empty").hint("pass part of a string, e.g. a sound event name"));
        }
        let b = args::build(s, &self.build)?;
        let class = self.class.as_deref().map(|c| args::class_code(b, c)).transpose()?;
        let (scan, built) = header_scan(b);
        let hits = search_strings(b, scan, term, class);
        let mut summary: Vec<Summary> =
            present::by_class(count_by_class(hits.iter().map(|h| h.tag.class))).into_iter().collect();
        summary.extend(present::scan_built(built));
        let page = self.page.page(&hits, DEFAULT_LIMIT);
        let rows = page
            .iter()
            .map(|h| {
                let mut matched = h.strings.iter().take(MAX_STRINGS).cloned().collect::<Vec<_>>().join(", ");
                if h.strings.len() > MAX_STRINGS {
                    matched.push_str(&format!(" (+{} more)", h.strings.len() - MAX_STRINGS));
                }
                vec![
                    h.tag.class.to_string().into(),
                    h.tag.global_id.to_string().into(),
                    b.display_name(h.tag).into(),
                    matched.into(),
                ]
            })
            .collect();
        let hints = hits
            .is_empty()
            .then(|| {
                format!("no tag's string table contains '{term}'; string tables hold string_id names and referenced tag names, not field values")
            })
            .into_iter()
            .collect();
        Ok(Output::Table(Table {
            build: s.label(b),
            total: hits.len(),
            summary,
            offset: self.page.offset,
            columns: vec![col::CLASS, col::GLOBAL_ID, col::DISPLAY_NAME, col::MATCHED],
            rows,
            hints,
            ..Table::default()
        }))
    }
}

#[cfg(test)]
#[path = "search_values_tests.rs"]
mod tests;
