//! The header scan (PRD §6.2, second stage): for every tag copy, its tag
//! references, its dependency list and its string table, kept per module so
//! only changed modules are scanned again. References come from the reference
//! fields in the tag data: the only exact source for those without a
//! dependency entry (format-notes "What references without a dependency point
//! to"; decisions D50). Built when a tool first needs it.

use std::io;
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::cache::{CacheFile, in_parallel};
use crate::fourcc::FourCc;
use crate::global_id::GlobalId;
use crate::index::{FailedModule, FileStamp, Index, ModuleRecord, Progress, UpdateReport};
use crate::module::{EntryKind, Module, ModuleError, Section, map_file};
use crate::tag::{TagHeader, string_at};
use crate::text::decode_cp1252;

/// A non-null tag reference field, as the tag data holds it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutReference {
    pub target: GlobalId,
    pub class: FourCc,
    /// Its position in the tag's reference list (for its field path).
    pub reference: u32,
    /// Where its listed name is in the tag's string table ([`TagScan::listed_name`]).
    pub name_offset: u32,
    /// Whether the header lists it as a dependency (Q12: not always).
    pub has_dependency: bool,
}

/// One tag copy's scan.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TagScan {
    pub global_id: GlobalId,
    /// Its file entry in the module.
    pub entry: u32,
    /// In the order of the tag's reference list; null ones left out.
    pub references: Vec<OutReference>,
    pub dependencies: Vec<GlobalId>,
    /// The tag's string table (Windows-1252): listed names and string_id strings.
    pub strings: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleScan {
    pub rel_path: String,
    /// The module file as scanned (same as the index's).
    pub stamp: FileStamp,
    pub tags: Vec<TagScan>,
    /// Tags whose header or tag data couldn't be read, with the reason.
    pub problems: Vec<(GlobalId, String)>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeaderScan {
    /// In the index's module order.
    pub modules: Vec<ModuleScan>,
    pub failed: Vec<FailedModule>,
}

/// Bump `format` when the scan's types change shape.
impl TagScan {
    /// `r`'s listed name (the referenced tag's path as this tag stores it).
    pub fn listed_name(&self, r: &OutReference) -> Option<std::borrow::Cow<'_, str>> {
        string_at(&self.strings, r.name_offset).map(decode_cp1252)
    }
}

const CACHE: CacheFile = CacheFile { magic: b"H5TAGSCN", format: 2, what: "header scan" };

impl Progress {
    pub const SCANNING_HEADERS: &'static str = "scanning tag headers";
}

/// The scan of one tag entry: its references, dependencies and strings.
fn scan_tag(module: &Module<'_>, entry: u32, global_id: GlobalId) -> Result<TagScan, String> {
    let header = module.read_section(entry, Section::Header).map_err(|e| e.to_string())?;
    let mut header = TagHeader::parse(&header).map_err(|e| format!("tag header: {e}"))?;
    let data = module.read_section(entry, Section::TagData).map_err(|e| e.to_string())?;
    let references = header
        .tag_references
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let field = header.reference_field(&data, r).filter(|f| !f.is_null())?;
            Some(OutReference {
                target: field.global_id,
                class: field.class_code,
                reference: i as u32,
                name_offset: r.name_offset,
                has_dependency: r.dependency.is_some(),
            })
        })
        .collect();
    Ok(TagScan {
        global_id,
        entry,
        references,
        dependencies: header.dependencies.iter().map(|d| d.global_id).collect(),
        strings: std::mem::take(&mut header.string_table),
    })
}

/// Scans every tag of the indexed module `record`.
fn scan_module(deploy: &Path, record: &ModuleRecord) -> Result<ModuleScan, ModuleError> {
    let map = map_file(&record.path_in(deploy))?;
    let module = Module::parse(&map)?;
    let mut scan =
        ModuleScan { rel_path: record.rel_path.clone(), stamp: record.stamp, tags: Vec::new(), problems: Vec::new() };
    for (e, entry) in record.entries.iter().enumerate().filter(|(_, e)| e.kind() == EntryKind::Tag) {
        match scan_tag(&module, e as u32, entry.global_id) {
            Ok(t) => scan.tags.push(t),
            Err(why) => scan.problems.push((entry.global_id, why)),
        }
    }
    Ok(scan)
}

impl HeaderScan {
    /// Brings `previous` up to date with `index` (of the build in `deploy`):
    /// modules whose file is unchanged are reused, the rest are scanned, in
    /// parallel; modules no longer indexed are dropped. Modules whose path
    /// matches `force` are scanned even if unchanged.
    pub fn update(
        deploy: &Path,
        index: &Index,
        previous: Option<HeaderScan>,
        force: &(dyn Fn(&str) -> bool + Sync),
        progress: &(dyn Fn(Progress) + Sync),
    ) -> (HeaderScan, UpdateReport) {
        let start = Instant::now();
        let mut old: std::collections::HashMap<String, ModuleScan> =
            previous.map(|p| p.modules).unwrap_or_default().into_iter().map(|m| (m.rel_path.clone(), m)).collect();
        let mut report = UpdateReport::default();
        let mut slots: Vec<Option<Result<ModuleScan, ModuleError>>> = Vec::with_capacity(index.modules.len());
        let mut to_scan = Vec::new();
        for (i, m) in index.modules.iter().enumerate() {
            match old.remove(&m.rel_path) {
                Some(s) if s.stamp == m.stamp && !force(&m.rel_path) => {
                    report.reused += 1;
                    slots.push(Some(Ok(s)));
                }
                _ => {
                    to_scan.push(i);
                    slots.push(None);
                }
            }
        }
        report.removed = old.len();
        report.read = to_scan.len();
        let done = |done| progress(Progress { stage: Progress::SCANNING_HEADERS, done, total: to_scan.len() });
        if !to_scan.is_empty() {
            done(0);
        }
        for (i, result) in in_parallel(&to_scan, &|i| scan_module(deploy, &index.modules[i]), &done) {
            slots[i] = Some(result);
        }
        let mut scan = HeaderScan::default();
        for (m, slot) in index.modules.iter().zip(slots) {
            match slot.expect("every module was reused or scanned") {
                Ok(s) => scan.modules.push(s),
                Err(err) => {
                    tracing::warn!(module = %m.rel_path, error = %err, "module could not be scanned");
                    report.failed += 1;
                    scan.failed.push(FailedModule { rel_path: m.rel_path.clone(), error: err.to_string() });
                }
            }
        }
        report.elapsed = start.elapsed();
        (scan, report)
    }

    /// The cached scan, or `None` (logged) if missing or from another format.
    pub fn load(path: &Path) -> Option<HeaderScan> {
        CACHE.load(path)
    }

    /// Writes the cache atomically.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        CACHE.save(path, self)
    }
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
