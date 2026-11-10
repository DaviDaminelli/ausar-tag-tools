//! `compare_builds` (PRD §6.6, M6, D82): what differs between two builds.

use serde::Deserialize;

use super::args::{self, BuildArg, PageArgs};
use super::{Tool, present};
use crate::compare::{Row, Status, compare_builds};
use crate::output::{Cell, Output, Summary, Table, ToolError, col};
use crate::query::count_by_class;
use crate::references::header_scan;
use crate::session::Session;

/// One kind of row (`status=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RowStatus {
    /// In both builds, with different tag data.
    Different,
    /// Only in build_a.
    OnlyInA,
    /// Only in build_b.
    OnlyInB,
}

/// What differs between two builds: tags only in one, and tags in both whose
/// tag data differs (each build's copy in the latest layer), matched by global
/// ID. Counts first, then rows by status, class and path. status= one kind of
/// row, class= one class. The first call may build the builds' header scans.
/// Next: read_tag(tag=<global ID>, build=...) in either build.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct CompareBuilds {
    /// The first build (a).
    pub build_a: String,
    /// The second build (b).
    pub build_b: String,
    /// Only tags of this class (code or name).
    #[arg(long)]
    pub class: Option<String>,
    /// Only rows of this kind: different, only_in_a, only_in_b (the counts stay whole).
    #[arg(long, value_enum)]
    pub status: Option<RowStatus>,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

const DEFAULT_LIMIT: usize = 100;

impl Tool for CompareBuilds {
    const NAME: &'static str = "compare_builds";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let a = args::build(s, &BuildArg { build: Some(self.build_a.clone()) })?;
        let b = args::build(s, &BuildArg { build: Some(self.build_b.clone()) })?;
        if std::ptr::eq(a, b) {
            return Err(ToolError::new(format!("build_a and build_b are the same build ({})", a.name))
                .hint("compare_builds compares two builds; list_modules names them"));
        }
        let class = self.class.as_deref().map(|c| args::class_code(a, c)).transpose()?;
        let ((sa, built_a), (sb, built_b)) = (header_scan(a), header_scan(b));
        let found = compare_builds(a, sa, b, sb, class);
        let wanted = |r: &&Row<'_>| match self.status {
            None => true,
            Some(RowStatus::Different) => r.status == Status::Different,
            Some(RowStatus::OnlyInA) => r.status == Status::OnlyInA,
            Some(RowStatus::OnlyInB) => r.status == Status::OnlyInB,
        };
        let shown: Vec<&Row<'_>> = found.rows.iter().filter(wanted).collect();
        // Cells only for the rows shown (D26).
        let rows = self
            .page
            .page(&shown, DEFAULT_LIMIT)
            .iter()
            .map(|r| {
                let (status, build) = match r.status {
                    Status::OnlyInB => ("only in b", b),
                    Status::OnlyInA => ("only in a", a),
                    _ => ("different", a),
                };
                let mut name = build.display_name(r.tag);
                // A shared tag under another path: the same asset, named for each platform.
                if let Some(other) = r.other.filter(|o| o.path_lower != r.tag.path_lower) {
                    name.push_str(&format!("  (b: {})", b.display_name(other)));
                }
                vec![status.into(), r.tag.class.to_string().into(), r.tag.global_id.to_string().into(), name.into()]
            })
            .collect();
        let count = |status| Cell::Int(found.count(status) as i64);
        let mut summary = vec![
            Summary { label: "a", key: "a", value: a.name.clone().into() },
            Summary { label: "b", key: "b", value: b.name.clone().into() },
            Summary { label: "different", key: "different", value: count(Status::Different) },
            Summary { label: "only in a", key: "only_in_a", value: count(Status::OnlyInA) },
            Summary { label: "only in b", key: "only_in_b", value: count(Status::OnlyInB) },
            Summary { label: "same", key: "same", value: count(Status::Same) },
        ];
        if !found.not_compared.is_empty() {
            summary.push(Summary {
                label: "not compared",
                key: "not_compared",
                value: Cell::Int(found.not_compared.len() as i64),
            });
        }
        summary.extend(present::by_class(count_by_class(shown.iter().map(|r| r.tag.class))));
        summary.extend(present::scans_built(built_a, built_b));
        let hints = found
            .not_compared
            .first()
            .map(|t| present::not_compared_hint(found.not_compared.len(), &a.display_name(t)));
        Ok(Output::Table(Table {
            total: shown.len(),
            summary,
            offset: self.page.offset,
            columns: vec![col::STATUS, col::CLASS, col::GLOBAL_ID, col::DISPLAY_NAME],
            rows,
            path_column: Some(3),
            wide_column: Some(3),
            hints: hints.into_iter().collect(),
            ..Table::default()
        }))
    }
}

#[cfg(test)]
#[path = "compare_builds_tests.rs"]
mod tests;
