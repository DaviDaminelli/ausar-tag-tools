//! `reindex` (PRD §6.2).

use serde::Deserialize;

use super::Tool;
use super::args::{self, BuildArg};
use crate::output::{Cell, Output, ReportItem, ToolError};
use crate::session::Session;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum What {
    /// Everything.
    #[default]
    All,
    /// Module file lists.
    Index,
    /// Tag headers (references, strings).
    HeaderScan,
    /// Cached results.
    Results,
}

/// Forces a rebuild. Rarely needed: the index updates itself when module files
/// change (size or time). Use it if a file was replaced with the same size and time.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct Reindex {
    /// What to rebuild: all, index, header_scan or results.
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub what: What,
    /// Only modules whose path contains this.
    #[arg(long)]
    pub module: Option<String>,
    /// Only tags of this class (header_scan and results).
    #[arg(long)]
    pub class: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
}

/// Parts that don't exist in this version yet (decisions D20).
const NOT_AVAILABLE: [(What, &str); 1] = [(What::Results, "results")];

impl Tool for Reindex {
    const NAME: &'static str = "reindex";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build_index(s, &self.build)?;
        let mut items = Vec::new();
        if matches!(self.what, What::All | What::Index) {
            let filter = args::module_filter(self.module.as_deref());
            let force = move |path: &str| filter.as_ref().is_none_or(|f| f.matches(path));
            let report = s.reindex(b, &force).map_err(|e| ToolError::new(e.to_string()))?;
            items.push(ReportItem {
                key: "index",
                values: vec![
                    ("read", Cell::Int(report.read as i64)),
                    ("reused", Cell::Int(report.reused as i64)),
                    ("removed", Cell::Int(report.removed as i64)),
                    ("failed", Cell::Int(report.failed as i64)),
                    ("seconds", Cell::Float(report.elapsed.as_secs_f64())),
                ],
            });
        }
        if matches!(self.what, What::All | What::HeaderScan) {
            let filter = args::module_filter(self.module.as_deref());
            let force = move |path: &str| filter.as_ref().is_none_or(|f| f.matches(path));
            let dropped = s.forget_header_scan(b, &force);
            let mut values =
                vec![("dropped modules", Cell::Int(dropped as i64)), ("rebuilt", Cell::Text("on next use".into()))];
            if self.class.is_some() {
                values.push(("note", "class= doesn't narrow the header scan (it works per module)".into()));
            }
            items.push(ReportItem { key: "header_scan", values });
        }
        for (what, key) in NOT_AVAILABLE {
            if self.what == What::All || self.what == what {
                items.push(ReportItem {
                    key,
                    values: vec![("rebuilt", Cell::Bool(false)), ("reason", "not available in this version".into())],
                });
            }
        }
        Ok(Output::Report { build: s.label(&s.builds()[b]), items })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Format;
    use crate::testdeploy::TestDeploy;

    #[test]
    fn reindex_drops_the_header_scan_for_the_next_call() {
        let mut d = TestDeploy::new("reindex-scan");
        crate::references::header_scan(&d.session.builds()[0]);
        let t = Reindex { what: What::HeaderScan, module: None, class: None, build: BuildArg::default() };
        let text = t.run(&mut d.session).unwrap().render(Format::Text);
        assert_eq!(text, "header_scan: dropped modules 1, rebuilt on next use\n");
        let t = Reindex { class: Some("weap".into()), ..t };
        let text = t.run(&mut d.session).unwrap().render(Format::Text);
        assert!(text.contains("note class= doesn't narrow the header scan (it works per module)"), "{text}");
    }
}
