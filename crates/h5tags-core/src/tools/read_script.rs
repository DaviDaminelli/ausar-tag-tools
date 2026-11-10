//! `read_script` (M7, decisions D85): a script tag's script.

use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::read_tag::title;
use super::{Tool, present};
use crate::output::{Cell, Output, Summary, Table, ToolError, col};
use crate::script::{Form, read_script};
use crate::session::Session;
use crate::stored::copy_identity;
use crate::tag_view::Detail;
use crate::view::TagView;

/// A script tag's script. hsc* (lua): its Lua source, numbered lines, paged.
/// luas (lua_script): compiled Havok Script, listed as the strings it holds
/// (names, text, paths of other scripts and tags); its code isn't decompiled.
/// The copy in the latest layer; scripts often differ between layers:
/// module= reads another (tag_copies lists them).
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct ReadScript {
    /// The script tag: global ID, tag path or display name.
    pub tag: String,
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
        let size = Summary { label: "size", key: "size", value: Cell::Size(script.size as u64) };
        let limit = self.page.limit.unwrap_or(DEFAULT_LIMIT);
        let table = match &script.form {
            Form::Text { text, not_utf8 } => {
                if *not_utf8 {
                    tag.title_lines.push(present::NOT_UTF8_LINE.into());
                }
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

#[cfg(test)]
#[path = "read_script_tests.rs"]
mod tests;
