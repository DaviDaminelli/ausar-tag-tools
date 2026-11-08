//! The definition check behind `definition_status` (PRD §6.5, §6.9, D60):
//! every tag copy of every class with a definition is read with it once, and
//! where the definition doesn't fit is recorded per tag. Kept per module,
//! cached next to the index, reused while the module and the definition files
//! it used are unchanged. Domain only.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use h5_defs::definition::{Definition, Field};
use h5_meta::{Body, Content, Problem};
use h5_module::cache::{CacheFile, in_parallel};
use h5_module::index::{EntryKind, FileStamp, UpdateReport};
use h5_module::tag::{StructKind, TagHeader};
use h5_module::{FourCc, GlobalId, Module, ModuleError, Section, map_file};
use serde::{Deserialize, Serialize};

use crate::catalog::{Build, ClassInfo};
use crate::stored::DefinitionLoadError;

/// What reading one tag copy with its class's definition found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TagCheck {
    pub global_id: GlobalId,
    pub class: FourCc,
    /// The main struct's GUID in the tag header: one per class layout.
    pub main_guid: Option<[u8; 16]>,
    /// Field paths (`[*]` for entries) where the definition's size isn't the
    /// tag's; empty = the main struct. Each path once.
    pub size_mismatches: Vec<String>,
    /// (field path, the tag's GUID) where it isn't the definition's. Each once.
    pub guid_mismatches: Vec<(String, [u8; 16])>,
    /// Other places reading stopped, and places the definition and the
    /// header's lists disagree (PRD §9 check 1).
    pub other_problems: u32,
    /// Why the tag couldn't be read at all.
    pub unreadable: Option<String>,
}

/// The check of one module's tags.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModuleCheck {
    pub rel_path: String,
    pub stamp: FileStamp,
    /// The definition files used, as they were: a change has the module checked again.
    pub definitions: Vec<(PathBuf, FileStamp)>,
    pub tags: Vec<TagCheck>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DefinitionCheck {
    /// In the index's module order.
    pub modules: Vec<ModuleCheck>,
    /// (module path, why) for modules that couldn't be read.
    pub failed: Vec<(String, String)>,
}

/// Bump `format` when the check's types change shape, or what a read finds
/// (2: main structs of another size are read in part, D70).
const CACHE: CacheFile = CacheFile { magic: b"H5TAGCHK", format: 2, what: "definition check" };

/// The definition files of the build's classes, as they are now.
fn current_definitions(b: &Build) -> HashMap<&Path, FileStamp> {
    b.classes
        .values()
        .filter_map(|c| c.definition.as_deref())
        .filter_map(|p| Some((p, FileStamp::of(&std::fs::metadata(p).ok()?))))
        .collect()
}

/// Notes a body's or tag block's problem and GUID mismatch at `path`.
fn note(problem: Option<&Problem>, guid: Option<[u8; 16]>, path: &str, t: &mut TagCheck) {
    match problem {
        Some(Problem::SizeMismatch { .. }) if !t.size_mismatches.iter().any(|p| p == path) => {
            t.size_mismatches.push(path.to_owned());
        }
        // Past the last confirmed anchor: part of the main struct's size mismatch (D70).
        Some(Problem::SizeMismatch { .. } | Problem::Unconfirmed { .. }) | None => {}
        Some(_) => t.other_problems += 1,
    }
    if let Some(g) = guid
        && !t.guid_mismatches.iter().any(|(p, x)| p == path && *x == g)
    {
        t.guid_mismatches.push((path.to_owned(), g));
    }
}

/// Walks a read tag with one path buffer (D26: copied only into what is found).
fn walk(body: &Body<'_>, path: &mut String, t: &mut TagCheck) {
    note(body.problem.as_ref(), body.guid_mismatch, path, t);
    for n in &body.children {
        let len = path.len();
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(n.name.unwrap_or_default());
        match &n.content {
            Content::Struct(b) | Content::Array(b) => walk(b, path, t),
            Content::Block(k) => {
                note(k.problem.as_ref(), k.guid_mismatch, path, t);
                path.push_str("[*]");
                for e in &k.entries {
                    walk(e, path, t);
                }
            }
            _ => {}
        }
        path.truncate(len);
    }
}

/// Reads one tag copy with `definition` and records what doesn't fit.
fn check_tag(module: &Module<'_>, entry: u32, t: &mut TagCheck, definition: &Definition) {
    let read = || -> Result<(TagHeader, Vec<u8>), String> {
        let header = module.read_section(entry, Section::Header).map_err(|e| e.to_string())?;
        let header = TagHeader::parse(&header).map_err(|e| format!("tag header: {e}"))?;
        Ok((header, module.read_section(entry, Section::TagData).map_err(|e| e.to_string())?))
    };
    let (header, data) = match read() {
        Ok(read) => read,
        Err(why) => return t.unreadable = Some(why),
    };
    t.main_guid = header.tag_structs.iter().find(|s| s.kind == StructKind::Main).map(|s| s.guid);
    let tag = h5_meta::read_tag(definition, &header, &data);
    t.other_problems += tag.divergences.len() as u32;
    walk(&tag.root, &mut String::new(), t);
}

/// Tags per unit of work: a large module is checked on every core.
const CHUNK: usize = 1024;

/// One unit of work: module `module`'s tag entries `entries` (file entry numbers).
struct Chunk {
    module: usize,
    entries: Vec<u32>,
}

/// Checks the tags of `chunk` whose class has a definition. Each class's
/// definition is looked up once per chunk, not per tag.
fn check_chunk(b: &Build, chunk: &Chunk) -> Result<Vec<TagCheck>, ModuleError> {
    let record = &b.index.modules[chunk.module];
    let map = map_file(&b.module_file(record))?;
    let module = Module::parse(&map)?;
    let mut definitions: HashMap<FourCc, Result<Arc<Definition>, String>> = HashMap::new();
    let mut out = Vec::with_capacity(chunk.entries.len());
    for &e in &chunk.entries {
        let entry = &record.entries[e as usize];
        let Some(path) = b.classes.get(&entry.class_code).and_then(|c| c.definition.as_deref()) else { continue };
        let definition = definitions
            .entry(entry.class_code)
            .or_insert_with(|| b.store.definition(path).map_err(|err| err.to_string()));
        let mut t = TagCheck {
            global_id: entry.global_id,
            class: entry.class_code,
            main_guid: None,
            size_mismatches: Vec::new(),
            guid_mismatches: Vec::new(),
            other_problems: 0,
            unreadable: None,
        };
        match definition {
            Ok(def) => check_tag(&module, e, &mut t, def),
            Err(why) => t.unreadable = Some(why.clone()),
        }
        out.push(t);
    }
    Ok(out)
}

/// Module `m`'s tag entries whose class has a definition, in chunks.
fn chunks_of(b: &Build, m: usize) -> Vec<Chunk> {
    let entries: Vec<u32> = b.index.modules[m]
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.kind() == EntryKind::Tag && b.classes.get(&e.class_code).is_some_and(|c| c.definition.is_some())
        })
        .map(|(i, _)| i as u32)
        .collect();
    entries.chunks(CHUNK).map(|c| Chunk { module: m, entries: c.to_vec() }).collect()
}

/// The definition files module `m`'s classes use, as they are now.
fn definitions_used(b: &Build, m: usize, current: &HashMap<&Path, FileStamp>) -> Vec<(PathBuf, FileStamp)> {
    let mut used: Vec<(PathBuf, FileStamp)> = Vec::new();
    for e in b.index.modules[m].entries.iter().filter(|e| e.kind() == EntryKind::Tag) {
        let Some(path) = b.classes.get(&e.class_code).and_then(|c| c.definition.as_deref()) else { continue };
        if !used.iter().any(|(p, _)| p == path)
            && let Some(&stamp) = current.get(path)
        {
            used.push((path.to_owned(), stamp));
        }
    }
    used
}

impl DefinitionCheck {
    /// Brings `previous` up to date with `b`: modules whose file and used
    /// definition files are unchanged are reused, the rest checked, in
    /// parallel; modules whose path matches `force` are checked anyway.
    pub fn update(
        b: &Build,
        previous: Option<DefinitionCheck>,
        force: &(dyn Fn(&str) -> bool + Sync),
    ) -> (DefinitionCheck, UpdateReport) {
        let start = Instant::now();
        let current = current_definitions(b);
        let mut old: HashMap<String, ModuleCheck> =
            previous.map(|p| p.modules).unwrap_or_default().into_iter().map(|m| (m.rel_path.clone(), m)).collect();
        let mut report = UpdateReport::default();
        let mut slots: Vec<Option<Result<ModuleCheck, ModuleError>>> = Vec::with_capacity(b.index.modules.len());
        let mut to_check = Vec::new();
        for (i, m) in b.index.modules.iter().enumerate() {
            let unchanged = |c: &ModuleCheck| {
                c.stamp == m.stamp && c.definitions.iter().all(|(p, s)| current.get(p.as_path()) == Some(s))
            };
            match old.remove(&m.rel_path) {
                Some(c) if unchanged(&c) && !force(&m.rel_path) => {
                    report.reused += 1;
                    slots.push(Some(Ok(c)));
                }
                _ => {
                    to_check.push(i);
                    slots.push(None);
                }
            }
        }
        report.removed = old.len();
        report.read = to_check.len();
        let chunks: Vec<Chunk> = to_check.iter().flat_map(|&m| chunks_of(b, m)).collect();
        let numbers: Vec<usize> = (0..chunks.len()).collect();
        // Chunks come back in any order: put by number, then joined per module in order.
        let mut results: Vec<Option<Result<Vec<TagCheck>, ModuleError>>> = (0..chunks.len()).map(|_| None).collect();
        for (i, result) in in_parallel(&numbers, &|i| check_chunk(b, &chunks[i]), &|_| {}) {
            results[i] = Some(result);
        }
        let mut results = chunks.iter().map(|c| c.module).zip(results).peekable();
        for &m in &to_check {
            let record = &b.index.modules[m];
            let definitions = definitions_used(b, m, &current);
            let mut module = Ok(ModuleCheck {
                rel_path: record.rel_path.clone(),
                stamp: record.stamp,
                definitions,
                tags: Vec::new(),
            });
            while let Some((_, result)) = results.next_if(|(cm, _)| *cm == m) {
                match (&mut module, result.expect("every chunk was checked")) {
                    (Ok(c), Ok(tags)) => c.tags.extend(tags),
                    (Ok(_), Err(e)) => module = Err(e),
                    (Err(_), _) => {}
                }
            }
            slots[m] = Some(module);
        }
        let mut check = DefinitionCheck::default();
        for (m, slot) in b.index.modules.iter().zip(slots) {
            match slot.expect("every module was reused or checked") {
                Ok(c) => check.modules.push(c),
                Err(err) => {
                    tracing::warn!(module = %m.rel_path, error = %err, "module could not be checked");
                    report.failed += 1;
                    check.failed.push((m.rel_path.clone(), err.to_string()));
                }
            }
        }
        report.elapsed = start.elapsed();
        (check, report)
    }

    pub fn load(path: &Path) -> Option<DefinitionCheck> {
        CACHE.load(path)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        CACHE.save(path, self)
    }

    fn tags(&self) -> impl Iterator<Item = &TagCheck> {
        self.modules.iter().flat_map(|m| &m.tags)
    }
}

/// A build's definition check: where it is cached, and the check once loaded or built.
#[derive(Default)]
pub struct CheckSlot {
    pub(crate) cache: Option<PathBuf>,
    check: OnceLock<DefinitionCheck>,
}

/// What building the check took, when a call had to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CheckBuilt {
    /// Tag copies checked in all (reused modules included).
    pub tags: usize,
    pub elapsed: Duration,
}

/// The definition check of `b`: loaded (and brought up to date) or built on
/// the first call, then kept; `CheckBuilt` when this call had to check modules.
pub fn definition_check(b: &Build) -> (&DefinitionCheck, Option<CheckBuilt>) {
    let mut built = None;
    let check = b.check.check.get_or_init(|| {
        let previous = b.check.cache.as_deref().and_then(DefinitionCheck::load);
        let (check, report) = DefinitionCheck::update(b, previous, &|_| false);
        if report.read > 0 || report.removed > 0 {
            built = Some(CheckBuilt { tags: check.tags().count(), elapsed: report.elapsed });
            if let Some(path) = &b.check.cache
                && let Err(e) = check.save(path)
            {
                tracing::warn!(cache = %path.display(), error = %e, "couldn't save the definition check");
            }
        }
        tracing::info!(build = %b.name, ?report, "definition check ready");
        check
    });
    (check, built)
}

/// `reindex(what=results)` (PRD §6.9): drops the check, and the cached checks
/// of the modules whose path matches `force`. Returns how many were dropped.
pub fn forget_definition_check(b: &mut Build, force: &dyn Fn(&str) -> bool) -> usize {
    b.check.check = OnceLock::new();
    let Some(path) = b.check.cache.clone() else { return 0 };
    let Some(mut cached) = DefinitionCheck::load(&path) else { return 0 };
    let before = cached.modules.len();
    cached.modules.retain(|m| !force(&m.rel_path));
    let dropped = before - cached.modules.len();
    if let Err(e) = cached.save(&path) {
        tracing::warn!(cache = %path.display(), error = %e, "couldn't save the definition check");
    }
    dropped
}

/// Every struct GUID the definitions of the build's classes name.
pub fn known_guids(b: &Build) -> Result<HashSet<[u8; 16]>, DefinitionLoadError> {
    fn add(fields: &[Field], out: &mut HashSet<[u8; 16]>) {
        for f in fields {
            out.extend(f.guid());
            add(&f.children, out);
        }
    }
    let mut out = HashSet::new();
    for path in b.classes.values().filter_map(|c| c.definition.as_deref()) {
        add(&b.store.definition(path)?.fields, &mut out);
    }
    Ok(out)
}

/// One class in `definition_status`: what the check found in its tags.
#[derive(Clone, Debug)]
pub struct ClassStatus<'b> {
    pub class: &'b ClassInfo,
    /// Tag copies checked (0 without a definition).
    pub checked: usize,
    pub unreadable: usize,
    /// Tag copies with a size mismatch, a GUID mismatch, another problem.
    pub size_tags: usize,
    pub guid_tags: usize,
    pub other_tags: usize,
    /// Distinct mismatching GUIDs that no definition names.
    pub unknown_guids: usize,
    /// Distinct main struct GUIDs: more than one = the class's layout varies.
    pub layouts: usize,
}

impl ClassStatus<'_> {
    pub fn has_gaps(&self) -> bool {
        self.class.definition.is_none() || self.unreadable + self.size_tags + self.guid_tags + self.other_tags > 0
    }
}

/// Every indexed class, those with gaps first, then by class code.
pub fn class_statuses<'b>(b: &'b Build, check: &DefinitionCheck, known: &HashSet<[u8; 16]>) -> Vec<ClassStatus<'b>> {
    #[derive(Default)]
    struct Acc {
        counts: [usize; 5],
        layouts: HashSet<[u8; 16]>,
        unknown: HashSet<[u8; 16]>,
    }
    let mut acc: HashMap<FourCc, Acc> = HashMap::new();
    for t in check.tags() {
        let a = acc.entry(t.class).or_default();
        a.counts[0] += 1;
        a.counts[1] += usize::from(t.unreadable.is_some());
        a.counts[2] += usize::from(!t.size_mismatches.is_empty());
        a.counts[3] += usize::from(!t.guid_mismatches.is_empty());
        a.counts[4] += usize::from(t.other_problems > 0);
        a.layouts.extend(t.main_guid);
        a.unknown.extend(t.guid_mismatches.iter().map(|(_, g)| *g).filter(|g| !known.contains(g)));
    }
    let mut out: Vec<ClassStatus<'b>> = b
        .classes
        .values()
        .map(|class| {
            let a = acc.remove(&class.code).unwrap_or_default();
            let [checked, unreadable, size_tags, guid_tags, other_tags] = a.counts;
            ClassStatus {
                class,
                checked,
                unreadable,
                size_tags,
                guid_tags,
                other_tags,
                unknown_guids: a.unknown.len(),
                layouts: a.layouts.len(),
            }
        })
        .collect();
    out.sort_by_key(|s| !s.has_gaps());
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GapKind {
    /// The tag couldn't be read; the reason.
    Unreadable(String),
    Size,
    /// The tag's GUID, and whether a definition names it.
    Guid {
        guid: [u8; 16],
        known: bool,
    },
    /// A main struct GUID of the class (a layout fingerprint, listed for every class).
    MainStruct([u8; 16]),
}

/// One kind of gap at one field path, over a class's tag copies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gap {
    pub kind: GapKind,
    /// Empty for the main struct.
    pub path: String,
    pub tags: usize,
    /// The first tag copy found with it.
    pub example: GlobalId,
}

/// `class`'s gaps: unreadable tags, size mismatches, GUID mismatches (each
/// by field path), then its main struct GUIDs, most used first.
pub fn class_gaps<'c>(check: &'c DefinitionCheck, class: FourCc, known: &HashSet<[u8; 16]>) -> Vec<Gap> {
    // (kind order, path, reason, GUID) → (tags, example), borrowed from the
    // check: text is copied once per gap, not per tag (D26).
    type Key<'c> = (u8, &'c str, &'c str, [u8; 16]);
    let mut found: BTreeMap<Key<'c>, (usize, GlobalId)> = BTreeMap::new();
    let mut count = |key: Key<'c>, id: GlobalId| found.entry(key).or_insert((0, id)).0 += 1;
    for t in check.tags().filter(|t| t.class == class) {
        if let Some(why) = &t.unreadable {
            count((0, "", why, [0; 16]), t.global_id);
        }
        for p in &t.size_mismatches {
            count((1, p, "", [0; 16]), t.global_id);
        }
        for (p, g) in &t.guid_mismatches {
            count((2, p, "", *g), t.global_id);
        }
        if let Some(g) = t.main_guid {
            count((3, "", "", g), t.global_id);
        }
    }
    let mut gaps: Vec<Gap> = found
        .into_iter()
        .map(|((order, path, why, guid), (tags, example))| {
            let kind = match order {
                0 => GapKind::Unreadable(why.to_owned()),
                1 => GapKind::Size,
                2 => GapKind::Guid { guid, known: known.contains(&guid) },
                _ => GapKind::MainStruct(guid),
            };
            Gap { kind, path: path.to_owned(), tags, example }
        })
        .collect();
    // Main struct GUIDs: most used first (stable, so the others keep their order).
    gaps.sort_by_key(|g| if matches!(g.kind, GapKind::MainStruct(_)) { usize::MAX - g.tags } else { 0 });
    gaps
}

#[cfg(test)]
#[path = "def_check_tests.rs"]
mod tests;
