//! `identify` (D63, CLI only): what a folder of modules is, before any
//! config exists. Needs no session.

use std::path::PathBuf;

use h5_module::identify::{Folder, identify};
use h5_module::tag::TAG_VERSION;

use super::present;
use crate::identify::{format_label, suggested_name};
use crate::output::{Cell, Output, ReportItem, ToolError};

/// What a folder of .module files is: its module and tag header formats,
/// whether they're supported, the game versions in its module names, and a
/// suggested build name for the config. Needs no config.
#[derive(Clone, Debug, clap::Args)]
pub struct Identify {
    /// The folder (searched recursively, any layout).
    pub folder: PathBuf,
}

/// `27 (3 modules, supported, forge); 53 (1 module, not supported)`; `label`
/// names a supported format (module formats only: both builds' tags are 27).
fn formats_text(
    formats: impl IntoIterator<Item = (u32, usize)>,
    supported: impl Fn(u32) -> bool,
    label: impl Fn(u32) -> Option<&'static str>,
) -> String {
    let parts: Vec<String> = formats
        .into_iter()
        .map(|(v, n)| {
            let modules = if n == 1 { "1 module".to_owned() } else { format!("{n} modules") };
            let state = if supported(v) { "supported" } else { "not supported" };
            match label(v).filter(|_| supported(v)) {
                Some(label) => format!("{v} ({modules}, {state}, {label})"),
                None => format!("{v} ({modules}, {state})"),
            }
        })
        .collect();
    if parts.is_empty() { "(none)".into() } else { parts.join("; ") }
}

impl Identify {
    pub fn run(&self) -> Result<Output, ToolError> {
        let folder = identify(&self.folder).map_err(|e| {
            ToolError::new(format!("can't read {}: {e}", self.folder.display()))
                .hint("give the folder that holds the .module files (any folder above them works)")
        })?;
        Ok(Output::Report { build: None, items: report(&self.folder, &folder) })
    }
}

fn report(path: &std::path::Path, f: &Folder) -> Vec<ReportItem> {
    let module_supported = |v| f.modules.iter().any(|m| m.format.as_ref().is_ok_and(|x| x.version == v && x.supported));
    let mut items = vec![
        ReportItem {
            key: "folder",
            values: vec![("path", path.display().to_string().into()), ("modules", Cell::Int(f.modules.len() as i64))],
        },
        ReportItem {
            key: "module formats",
            values: vec![("found", formats_text(f.module_formats(), module_supported, format_label).into())],
        },
        ReportItem {
            key: "tag header formats",
            values: vec![("found", formats_text(f.tag_formats(), |v| v == TAG_VERSION, |_| None).into())],
        },
        ReportItem {
            key: "game versions",
            values: vec![
                ("base", Cell::Bool(f.modules.iter().any(|m| m.layer.version == 0 && m.format.is_ok()))),
                ("latest", f.latest_layer().map_or_else(|| "(none)".to_owned(), present::layer_text).into()),
            ],
        },
    ];
    let not_modules: Vec<&h5_module::identify::ModuleFacts> = f.modules.iter().filter(|m| m.format.is_err()).collect();
    if let Some(first) = not_modules.first() {
        let why = first.format.as_ref().err().cloned().unwrap_or_default();
        items.push(ReportItem {
            key: "not modules",
            values: vec![
                ("files", Cell::Int(not_modules.len() as i64)),
                ("first", format!("{} ({why})", first.rel_path).into()),
            ],
        });
    }
    let unsupported = f.modules.iter().filter(|m| m.format.as_ref().is_ok_and(|x| !x.supported)).count();
    if unsupported > 0 {
        items.push(ReportItem {
            key: "not supported yet",
            values: vec![
                ("modules", Cell::Int(unsupported as i64)),
                ("note", "this format can't be read yet; a sample module would let it be added".into()),
            ],
        });
    }
    if f.modules.is_empty() {
        items.push(ReportItem { key: "note", values: vec![("found", "no .module files under this folder".into())] });
    }
    items.push(ReportItem { key: "suggested build name", values: vec![("name", suggested_name(f).into())] });
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Format;
    use crate::testdeploy::TestDeploy;

    /// D63: a folder's formats, versions and suggested name, with no config.
    #[test]
    fn identifies_a_folder() {
        let d = TestDeploy::new("identify-tool");
        let deploy = d.dir.0.join("deploy");
        let text = Identify { folder: deploy.clone() }.run().unwrap().render(Format::Text);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], format!("folder: path {}, modules 1", deploy.display()));
        assert_eq!(lines[1], "module formats: found 27 (1 module, supported, forge)");
        assert_eq!(lines[2], "tag header formats: found 27 (1 module, supported)");
        assert_eq!(lines[3], "game versions: base yes, latest (none)");
        assert_eq!(lines[4], "suggested build name: name forge");
        assert!(Identify { folder: d.dir.0.join("nowhere") }.run().is_err());
    }
}
