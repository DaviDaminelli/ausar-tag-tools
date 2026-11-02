//! `definition_status` (PRD §6.5): how well the definitions fit the build's
//! tags, per class, and the definition gap list (D33).

use h5_defs::definition::guid_text;
use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::def_check::{GapKind, class_gaps, class_statuses, definition_check, known_guids};
use crate::output::{Cell, Output, Summary, Table, ToolError, col};
use crate::session::Session;

/// How well the definitions fit this build's tags: per class, whether it has
/// a definition and what reading every tag copy with it found (size and GUID
/// mismatches, layouts). Classes with gaps first. class=: that class's gap
/// list, by field path. The first call checks every tag (cached after).
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct DefinitionStatus {
    /// One class's gaps (code or name).
    #[arg(long)]
    pub class: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

impl Tool for DefinitionStatus {
    const NAME: &'static str = "definition_status";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build(s, &self.build)?;
        let class = self.class.as_deref().map(|c| args::class_code(b, c)).transpose()?;
        let (check, built) = definition_check(b);
        let known = known_guids(b).map_err(present::definition_error)?;
        let statuses = class_statuses(b, check, &known);
        let checked: usize =
            statuses.iter().filter(|st| class.is_none_or(|c| st.class.code == c)).map(|st| st.checked).sum();
        let mut summary =
            vec![Summary { label: "tag copies checked", key: "checked", value: Cell::Int(checked as i64) }];
        let mut hints = Vec::new();
        if !check.failed.is_empty() {
            hints.push(present::unchecked_modules_hint(&check.failed));
        }
        // Cells only for the rows shown (D26).
        let (total, columns, rows) = match class {
            None => {
                let without = statuses.iter().filter(|st| st.class.definition.is_none()).count();
                summary.push(Summary {
                    label: "classes without a definition",
                    key: "without_definition",
                    value: Cell::Int(without as i64),
                });
                let rows: Vec<Vec<Cell>> = self
                    .page
                    .page(&statuses, 100)
                    .iter()
                    .map(|st| {
                        vec![
                            st.class.code.to_string().into(),
                            st.class.name.clone().into(),
                            Cell::Int(st.class.tag_count as i64),
                            Cell::Bool(st.class.definition.is_some()),
                            Cell::Int(st.checked as i64),
                            Cell::Int(st.size_tags as i64),
                            Cell::Int(st.guid_tags as i64),
                            Cell::Int(st.unknown_guids as i64),
                            Cell::Int((st.other_tags + st.unreadable) as i64),
                            Cell::Int(st.layouts as i64),
                        ]
                    })
                    .collect();
                let columns = vec![
                    col::CLASS,
                    col::CLASS_NAME,
                    col::TAGS,
                    col::DEFINITION,
                    col::CHECKED,
                    col::SIZE_MISMATCHES,
                    col::GUID_MISMATCHES,
                    col::UNKNOWN_GUIDS,
                    col::OTHER_PROBLEMS,
                    col::LAYOUTS,
                ];
                (statuses.len(), columns, rows)
            }
            Some(code) => {
                let info = &b.classes[&code];
                if info.definition.is_none() {
                    hints.push(format!(
                        "class {} ({}) has no definition: its tags can't be read field by field or checked",
                        code, info.name
                    ));
                }
                let gaps = class_gaps(check, code, &known);
                let rows: Vec<Vec<Cell>> = self
                    .page
                    .page(&gaps, 100)
                    .iter()
                    .map(|g| {
                        let guid = match g.kind {
                            GapKind::Guid { guid, .. } | GapKind::MainStruct(guid) => guid_text(&guid).into(),
                            _ => Cell::Empty,
                        };
                        let example =
                            b.tag_by_id(g.example).map_or_else(|| g.example.to_string(), |t| b.display_name(t));
                        vec![
                            present::gap_text(&g.kind).into(),
                            g.path.clone().into(),
                            guid,
                            Cell::Int(g.tags as i64),
                            example.into(),
                        ]
                    })
                    .collect();
                (gaps.len(), vec![col::GAP, col::FIELD_PATH, col::GUID, col::TAGS, col::EXAMPLE], rows)
            }
        };
        summary.extend(present::check_built(built));
        Ok(Output::Table(Table {
            build: s.label(b),
            total,
            summary,
            offset: self.page.offset,
            columns,
            rows,
            hints,
            ..Table::default()
        }))
    }
}

#[cfg(test)]
#[path = "definition_status_tests.rs"]
mod tests;
