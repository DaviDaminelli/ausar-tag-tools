//! The per-build index (PRD §6.2, first stage): every module's file list and
//! file names, kept per module so that only changed modules are re-read.
//! Reads no entry data: everything here comes from the uncompressed lists at
//! the start of each module.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::check::checks;
use crate::fourcc::FourCc;
use crate::global_id::GlobalId;
pub use crate::module::EntryKind;
use crate::module::{Module, ModuleError, map_file};
use crate::names::{LOAD_MANIFEST, ResourceName, parse_resource_name, split_tag_name};

/// One file entry, as stored in the index.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryRecord {
    pub name: String,
    pub global_id: GlobalId,
    pub class_code: FourCc,
    /// The owning entry, for resources.
    pub parent: Option<u32>,
    /// File indices (in this module) of the resources this entry owns.
    pub resources: Vec<u32>,
    pub asset_checksum: u64,
    pub compressed_size: u32,
    /// Uncompressed size of the data, wherever it is stored.
    pub uncompressed_size: u64,
    /// False if the entry describes data that isn't stored in this module.
    pub stored: bool,
}

impl EntryRecord {
    pub fn kind(&self) -> EntryKind {
        EntryKind::of(self.global_id, self.parent)
    }

    /// For a resource, its parsed `<owner>[<index>:<kind>]` name.
    pub fn resource_name(&self) -> Option<ResourceName<'_>> {
        (self.kind() == EntryKind::Resource).then(|| parse_resource_name(&self.name)).flatten()
    }

    /// The display name of the tag this entry belongs to: its own name for a
    /// tag, the owner's for a resource, and the file name for other files.
    pub fn owner_name(&self) -> &str {
        self.resource_name().map_or(&self.name, |r| r.owner)
    }

    /// The tag path and class name of the tag this entry belongs to, from its
    /// owner name `<path>.<class>`. Other files: the file name and `""`.
    pub fn owner_parts(&self) -> (&str, &str) {
        match self.kind() {
            EntryKind::File => (&self.name, ""),
            _ => {
                let (path, class) = split_tag_name(self.owner_name());
                (path, class.unwrap_or(""))
            }
        }
    }

    /// The tag path this entry belongs to (the owner name without `.class`).
    pub fn tag_path(&self) -> &str {
        self.owner_parts().0
    }
}

/// Size and modification time: a module is re-read when either changes.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileStamp {
    pub size: u64,
    pub modified_secs: u64,
    pub modified_nanos: u32,
}

impl FileStamp {
    pub fn of(meta: &fs::Metadata) -> FileStamp {
        let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).unwrap_or_default();
        FileStamp { size: meta.len(), modified_secs: modified.as_secs(), modified_nanos: modified.subsec_nanos() }
    }
}

/// Failures of one structural cross-check, summarized.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleIssue {
    pub check: String,
    pub count: u32,
    pub first: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleRecord {
    /// Relative to the deploy folder, `\`-separated on every OS (PRD §6.1).
    pub rel_path: String,
    pub stamp: FileStamp,
    pub version: u32,
    pub loaded_tag_count: u32,
    pub entries: Vec<EntryRecord>,
    /// Structural cross-checks that failed (empty for a sound module).
    pub issues: Vec<ModuleIssue>,
    /// Where the last entry's data ends; more than the file size means the
    /// file is incomplete.
    pub data_end: u64,
}

impl ModuleRecord {
    /// Entries whose data lies past the end of the file.
    pub fn entries_past_end(&self) -> u32 {
        self.issues.iter().find(|i| i.check == checks::DATA_BOUNDS).map_or(0, |i| i.count)
    }

    pub fn from_module(rel_path: String, stamp: FileStamp, module: &Module<'_>) -> Result<ModuleRecord, ModuleError> {
        let mut issues: Vec<ModuleIssue> = Vec::new();
        for failure in module.check() {
            match issues.iter_mut().find(|i| i.check == failure.check) {
                Some(issue) => issue.count += 1,
                None => {
                    issues.push(ModuleIssue { check: failure.check.to_owned(), count: 1, first: failure.to_string() })
                }
            }
        }
        let mut entries = Vec::with_capacity(module.entries.len());
        for (i, e) in module.entries.iter().enumerate() {
            let i = i as u32;
            entries.push(EntryRecord {
                name: module.name(i)?.to_owned(),
                global_id: e.global_id,
                class_code: e.class_code,
                parent: e.parent(),
                resources: module.entry_resources(i)?.to_vec(),
                asset_checksum: e.asset_checksum,
                compressed_size: e.total_compressed_size,
                uncompressed_size: e.data_size(),
                stored: !e.is_stored_elsewhere(),
            });
        }
        Ok(ModuleRecord {
            rel_path,
            stamp,
            version: module.header.version.raw(),
            loaded_tag_count: module.header.loaded_tag_count,
            entries,
            issues,
            data_end: module.data_end(),
        })
    }

    pub fn tag_count(&self) -> usize {
        self.entries.iter().filter(|e| e.kind() == EntryKind::Tag).count()
    }

    pub fn has_loadmanifest(&self) -> bool {
        self.entries.iter().any(|e| e.kind() == EntryKind::File && e.name == LOAD_MANIFEST)
    }
}

/// A module that couldn't be indexed, with the reason.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailedModule {
    pub rel_path: String,
    pub error: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Index {
    /// Sorted by path (ordinal, case-insensitive).
    pub modules: Vec<ModuleRecord>,
    pub failed: Vec<FailedModule>,
}

/// What an [`Index::update`] did.
#[derive(Clone, Debug, Default)]
pub struct UpdateReport {
    pub read: usize,
    pub reused: usize,
    pub removed: usize,
    pub failed: usize,
    pub elapsed: Duration,
}

/// A `.module` file found under a deploy folder.
#[derive(Clone, Debug)]
pub struct FoundModule {
    pub rel_path: String,
    pub path: PathBuf,
    pub stamp: FileStamp,
}

/// Sort key for module paths: ordinal, case-insensitive (PRD §6.4.6).
pub fn path_sort_key(p: &str) -> String {
    p.to_lowercase()
}

/// Finds every `.module` file under `deploy`, recursively, sorted by path.
pub fn find_modules(deploy: &Path) -> io::Result<Vec<FoundModule>> {
    let mut found = Vec::new();
    let mut dirs = vec![deploy.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for item in fs::read_dir(&dir)? {
            let item = item?;
            let path = item.path();
            let meta = item.metadata()?;
            if meta.is_dir() {
                dirs.push(path);
            } else if meta.is_file()
                && path.extension().is_some_and(|x| x.eq_ignore_ascii_case("module"))
                && let Ok(rel) = path.strip_prefix(deploy)
            {
                let rel_path = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("\\");
                found.push(FoundModule { rel_path, path: path.clone(), stamp: FileStamp::of(&meta) });
            }
        }
    }
    found.sort_by_cached_key(|m| path_sort_key(&m.rel_path));
    Ok(found)
}

/// Reads one module's lists into a record.
pub fn read_module(found: &FoundModule) -> Result<ModuleRecord, ModuleError> {
    let map = map_file(&found.path)?;
    let module = Module::parse(&map)?;
    ModuleRecord::from_module(found.rel_path.clone(), found.stamp, &module)
}

/// Reads the modules `which` (indices into `found`) in parallel; each read only
/// touches its own file. Returns `(index into found, result)` in any order.
fn read_in_parallel(found: &[FoundModule], which: &[usize]) -> Vec<(usize, Result<ModuleRecord, ModuleError>)> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(which.len()));
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(which.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                while let Some(&i) = which.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let result = read_module(&found[i]);
                    results.lock().unwrap().push((i, result));
                }
            });
        }
    });
    results.into_inner().unwrap()
}

const CACHE_MAGIC: &[u8; 8] = b"H5TAGIDX";
/// Bump when [`Index`] or its records change shape.
const CACHE_FORMAT: u32 = 2;

impl Index {
    /// Brings `previous` up to date with the modules under `deploy`: new and
    /// changed modules are read, removed ones dropped, unchanged ones reused.
    /// Modules whose path matches `force` are read even if unchanged.
    pub fn update(
        deploy: &Path,
        previous: Option<Index>,
        force: &(dyn Fn(&str) -> bool + Sync),
    ) -> io::Result<(Index, UpdateReport)> {
        let start = Instant::now();
        let found = find_modules(deploy)?;
        let mut old: std::collections::HashMap<String, ModuleRecord> =
            previous.map(|p| p.modules).unwrap_or_default().into_iter().map(|m| (m.rel_path.clone(), m)).collect();

        let mut report = UpdateReport::default();
        let mut slots: Vec<Option<Result<ModuleRecord, ModuleError>>> = Vec::with_capacity(found.len());
        let mut to_read = Vec::new();
        for (i, f) in found.iter().enumerate() {
            match old.remove(&f.rel_path) {
                Some(rec) if rec.stamp == f.stamp && !force(&f.rel_path) => {
                    report.reused += 1;
                    slots.push(Some(Ok(rec)));
                }
                _ => {
                    to_read.push(i);
                    slots.push(None);
                }
            }
        }
        report.removed = old.len();
        report.read = to_read.len();

        for (i, result) in read_in_parallel(&found, &to_read) {
            slots[i] = Some(result);
        }

        let mut index = Index::default();
        for (f, slot) in found.iter().zip(slots) {
            match slot.expect("every module was reused or read") {
                Ok(rec) => index.modules.push(rec),
                Err(err) => {
                    tracing::warn!(module = %f.rel_path, error = %err, "module could not be indexed");
                    report.failed += 1;
                    index.failed.push(FailedModule { rel_path: f.rel_path.clone(), error: err.to_string() });
                }
            }
        }
        report.elapsed = start.elapsed();
        Ok((index, report))
    }

    /// Loads a cached index. Returns `None` (and logs why) if it is missing,
    /// unreadable or from another cache format.
    pub fn load(path: &Path) -> Option<Index> {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
            Err(e) => {
                tracing::warn!(cache = %path.display(), error = %e, "index cache unreadable; rebuilding");
                return None;
            }
        };
        let body = bytes
            .strip_prefix(CACHE_MAGIC.as_slice())
            .and_then(|rest| rest.split_first_chunk::<4>())
            .filter(|(format, _)| u32::from_le_bytes(**format) == CACHE_FORMAT)
            .map(|(_, body)| body);
        let Some(body) = body else {
            tracing::info!(cache = %path.display(), "index cache is from another format; rebuilding");
            return None;
        };
        match postcard::from_bytes(body) {
            Ok(index) => Some(index),
            Err(e) => {
                tracing::warn!(cache = %path.display(), error = %e, "index cache is corrupt; rebuilding");
                None
            }
        }
    }

    /// Writes the cache atomically (temporary file, then rename).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut bytes = CACHE_MAGIC.to_vec();
        bytes.extend_from_slice(&CACHE_FORMAT.to_le_bytes());
        bytes.extend(postcard::to_stdvec(self).map_err(io::Error::other)?);
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, path)
    }
}
