//! `lookup_text` (M8, decisions D92, D93): in-game texts by string_id or by
//! what they say.

use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::filters::Substring;
use crate::output::{Cell, Output, Summary, Table, ToolError, col, marker};
use crate::session::Session;
use crate::text::{Lookup, Scope, TextAt, TextError, search_texts, text_class, text_languages};

/// In-game text, from unic (multilingual_unicode_string_list) tags.
/// string_id=: a name as read_tag shows it (e.g. a weapon's name field);
/// term=: part of a text, case-insensitive. Lists every unic tag that has it:
/// one string_id can have different texts in different unic tags, and the
/// files don't say which one a given tag shows. tag= alone lists that tag's
/// texts. Each unic tag's copy in the latest layer; module= (with tag=) reads
/// another.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct LookupText {
    /// A string_id name, e.g. from read_tag (whole name, case-insensitive).
    #[arg(long)]
    pub string_id: Option<String>,
    /// Part of a text (case-insensitive), in lang.
    #[arg(long)]
    pub term: Option<String>,
    /// A language (english, german, french, …) or all (default english).
    #[arg(long)]
    pub lang: Option<String>,
    /// Only this unic tag: global ID, tag path or display name.
    #[arg(long)]
    pub tag: Option<String>,
    /// With tag=: read the copy in the module whose path contains this.
    #[arg(long)]
    pub module: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

const DEFAULT_LIMIT: usize = 50;
const DEFAULT_LANGUAGE: &str = "english";

/// The text if it isn't blank.
fn given(text: &Option<String>) -> Option<&str> {
    text.as_deref().map(str::trim).filter(|t| !t.is_empty())
}

impl Tool for LookupText {
    const NAME: &'static str = "lookup_text";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let (string_id, term) = (given(&self.string_id), given(&self.term));
        if string_id.is_some() && term.is_some() {
            return Err(ToolError::new("pass string_id= or term=, not both"));
        }
        if string_id.is_none() && term.is_none() && self.tag.is_none() {
            return Err(ToolError::new("pass string_id= or term=").hint(
                "string_id= a name as read_tag shows it; term= part of a text; tag= alone lists one unic tag's texts",
            ));
        }
        if self.module.is_some() && self.tag.is_none() {
            return Err(ToolError::new("module= needs tag=").hint("tag_copies(tag) lists a unic tag's copies"));
        }
        let (b, scope, copies_hint) = match &self.tag {
            Some(tag) => {
                let t = args::tag_copy(s, tag, &self.build, self.module.as_deref())?;
                if t.tag.class != text_class() {
                    return Err(present::text_error(TextError::NotText));
                }
                (t.build, Scope::One(t.tag, t.copy), t.other_copies_hint)
            }
            None => (args::build(s, &self.build)?, Scope::Latest, None),
        };
        let languages = text_languages(b).map_err(present::text_error)?;
        let lang = args::language(self.lang.as_deref().unwrap_or(DEFAULT_LANGUAGE), &languages)?;
        let finder = term.map(Substring::new);
        let accepts = |text: &str| finder.as_ref().is_some_and(|f| f.is_match(text));
        let lookup = match (string_id, term) {
            (Some(id), _) => Lookup::StringId(id),
            (_, Some(_)) => Lookup::Term(&accepts),
            _ => Lookup::Every,
        };
        let found = search_texts(b, scope, &lookup, lang);

        let rows = self
            .page
            .page(&found.hits, DEFAULT_LIMIT)
            .iter()
            .map(|h| {
                let t = found.tag(h);
                let id = found.string_id(h);
                let mut row = vec![
                    t.global_id.to_string().into(),
                    b.display_name(t).into(),
                    match &id.text {
                        Some(name) => name.as_str().into(),
                        None => format!("{} 0x{:08X}", marker::UNRESOLVED, id.hash).into(),
                    },
                ];
                if lang.is_none() {
                    row.push(languages[h.language].as_str().into());
                }
                row.push(match found.text(h) {
                    TextAt::Text(text) => Cell::Escaped(text.to_owned()),
                    TextAt::Missing => marker::NONE.into(),
                    TextAt::Unreadable => marker::NOT_READ.into(),
                });
                row
            })
            .collect();

        let lang_name = lang.map_or("all", |l| languages[l].as_str());
        let differ: Vec<(String, &str, usize)> = found
            .differing()
            .into_iter()
            .map(|(id, l, n)| {
                (id.text.clone().unwrap_or_else(|| format!("0x{:08X}", id.hash)), languages[l].as_str(), n)
            })
            .collect();
        let mut hints: Vec<String> = present::differing_texts(&differ).into_iter().collect();
        hints.extend(present::skipped_tags("unic", &found.skipped));
        if found.unreadable > 0 {
            hints.push(present::unreadable_texts(found.unreadable));
        }
        let layered = found.tags().filter(|t| t.copies.len() > 1).count();
        match copies_hint {
            Some(h) => hints.push(h),
            None if layered > 0 => hints.push(present::older_text_copies(layered)),
            None => {}
        }
        if found.hits.is_empty() {
            hints.push(present::no_text(string_id, term, lang_name));
        }

        let mut columns = vec![col::GLOBAL_ID, col::DISPLAY_NAME, col::STRING_ID];
        if lang.is_none() {
            columns.push(col::LANGUAGE);
        }
        columns.push(col::TEXT);
        Ok(Output::Table(Table {
            build: s.label(b),
            total: found.hits.len(),
            summary: vec![
                Summary { label: "tags", key: "tags", value: Cell::Int(found.tags().count() as i64) },
                Summary { label: "searched", key: "searched", value: Cell::Int(found.searched as i64) },
            ],
            offset: self.page.offset,
            columns,
            rows,
            path_column: Some(1),
            hints,
            ..Table::default()
        }))
    }
}

#[cfg(test)]
#[path = "lookup_text_tests.rs"]
mod tests;
