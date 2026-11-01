//! The domain operations behind the tools: typed inputs, typed results. No
//! text, no hints, no rendering; tools turn the results into tables.

use std::collections::{BTreeMap, HashMap};

use h5_module::FourCc;
use h5_module::index::{EntryKind, EntryRecord, ModuleRecord};
use serde::Deserialize;

use crate::catalog::{Build, Copy, Tag};
use crate::filters::{Filters, ModuleFilter, entry_kind};

/// Tags matching `filters`, in tag path order (PRD §6.3).
pub fn search_tags<'b>(build: &'b Build, filters: &Filters) -> Vec<&'b Tag> {
    build.tags.iter().filter(|t| filters.matches_tag(build, t)).collect()
}

/// How many of `tags` each class has, most first (ties by class code).
pub fn count_by_class(tags: &[&Tag]) -> Vec<(FourCc, usize)> {
    let mut counts: BTreeMap<FourCc, usize> = BTreeMap::new();
    for t in tags {
        *counts.entry(t.class).or_default() += 1;
    }
    let mut counts: Vec<(FourCc, usize)> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    counts
}

/// Folder and file names used in tag paths that are close to `text` (its last
/// `\`-separated part), best first: "did you mean" for a search with no hits.
pub fn similar_names(build: &Build, text: &str, max: usize) -> Vec<String> {
    /// Below this, a name is too different to suggest.
    const MIN_SIMILARITY: f64 = 0.7;
    let wanted = text.trim_matches('\\').rsplit('\\').next().unwrap_or_default().to_ascii_lowercase();
    if wanted.is_empty() {
        return Vec::new();
    }
    // Every distinct name once, keyed in lowercase, as first seen.
    let mut names: HashMap<String, &str> = HashMap::new();
    for t in &build.tags {
        for part in t.path.split('\\') {
            names.entry(part.to_ascii_lowercase()).or_insert(part);
        }
    }
    let mut scored: Vec<(f64, &str)> = names
        .iter()
        .map(|(lower, name)| (strsim::normalized_damerau_levenshtein(&wanted, lower), *name))
        .filter(|(score, _)| *score >= MIN_SIMILARITY)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(b.1)));
    scored.into_iter().take(max).map(|(_, name)| name.to_owned()).collect()
}

pub struct Subfolder {
    /// As first seen (folders are matched case-insensitively).
    pub name: String,
    /// Tags under it, recursively.
    pub tag_count: usize,
}

pub struct Folder<'b> {
    /// Byte length of the folder prefix (`a\b\`) in each tag path.
    pub prefix_len: usize,
    pub subfolders: Vec<Subfolder>,
    /// Tags directly in the folder.
    pub tags: Vec<&'b Tag>,
}

/// The contents of `folder` (`\`-separated, case-insensitive; empty = top).
/// `None` if no tag lies under it.
pub fn browse<'b>(build: &'b Build, folder: &str) -> Option<Folder<'b>> {
    let folder = folder.trim_matches('\\');
    let prefix = if folder.is_empty() { String::new() } else { format!("{}\\", folder.to_ascii_lowercase()) };
    let start = build.tags.partition_point(|t| t.path_lower < prefix);
    let mut subfolders: BTreeMap<String, Subfolder> = BTreeMap::new();
    let mut tags = Vec::new();
    for t in build.tags[start..].iter().take_while(|t| t.path_lower.starts_with(&prefix)) {
        match t.path[prefix.len()..].split_once('\\') {
            Some((sub, _)) => {
                subfolders
                    .entry(sub.to_ascii_lowercase())
                    .or_insert_with(|| Subfolder { name: sub.to_owned(), tag_count: 0 })
                    .tag_count += 1
            }
            None => tags.push(t),
        }
    }
    if !prefix.is_empty() && subfolders.is_empty() && tags.is_empty() {
        return None;
    }
    Some(Folder { prefix_len: prefix.len(), subfolders: subfolders.into_values().collect(), tags })
}

/// Which copy of a tag is used (PRD §6.4.6, provisional rule): the first one,
/// in module path order, whose module matches `module`.
pub fn choose_copy(build: &Build, tag: &Tag, module: Option<&ModuleFilter>) -> Option<Copy> {
    tag.copies.iter().copied().find(|&c| module.is_none_or(|m| m.matches(&build.module(c).rel_path)))
}

/// Which entries `entries` looks at.
#[derive(Clone, Copy, Debug)]
pub enum EntryScope {
    /// Every entry of every module.
    All,
    /// One copy of a tag and the resources it owns.
    Tag(Copy),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntrySort {
    /// By name.
    #[default]
    Name,
    /// Largest first.
    Size,
}

#[derive(Clone, Copy)]
pub struct EntryHit<'b> {
    pub module: &'b ModuleRecord,
    pub entry: &'b EntryRecord,
}

/// File entries in `scope` matching `filters`, sorted.
pub fn entries<'b>(build: &'b Build, scope: EntryScope, filters: &Filters, sort: EntrySort) -> Vec<EntryHit<'b>> {
    let in_scope: Vec<EntryHit> = match scope {
        EntryScope::All => build
            .index
            .modules
            .iter()
            .flat_map(|module| module.entries.iter().map(move |entry| EntryHit { module, entry }))
            .collect(),
        EntryScope::Tag(copy) => {
            let module = build.module(copy);
            let owner = build.entry(copy);
            std::iter::once(owner)
                .chain(owner.resources.iter().filter_map(|&r| module.entries.get(r as usize)))
                .map(|entry| EntryHit { module, entry })
                .collect()
        }
    };
    let mut hits: Vec<EntryHit> =
        in_scope.into_iter().filter(|h| filters.matches_entry(&h.module.rel_path, h.entry)).collect();
    match sort {
        EntrySort::Name => hits.sort_by_cached_key(|h| (h.entry.name.to_ascii_lowercase(), h.module.rel_path.clone())),
        EntrySort::Size => hits.sort_by_key(|h| std::cmp::Reverse(h.entry.uncompressed_size)),
    }
    hits
}

/// True if any entry of the build is a resource matching `filters`' kind.
pub fn any_resource_matches(build: &Build, filters: &Filters) -> bool {
    let Some(kind) = &filters.kind else { return false };
    build.index.modules.iter().flat_map(|m| &m.entries).any(|e| e.kind() == EntryKind::Resource && kind.matches(e))
}

pub struct KindTotal {
    pub kind: String,
    pub count: usize,
    pub bytes: u64,
}

/// Every resource kind with its entry count and total size, by kind.
pub fn resource_kinds(build: &Build) -> Vec<KindTotal> {
    let mut kinds: BTreeMap<String, KindTotal> = BTreeMap::new();
    for e in build.index.modules.iter().flat_map(|m| &m.entries).filter(|e| e.kind() == EntryKind::Resource) {
        let kind = entry_kind(e);
        let total = kinds.entry(kind.clone()).or_insert(KindTotal { kind, count: 0, bytes: 0 });
        total.count += 1;
        total.bytes += e.uncompressed_size;
    }
    kinds.into_values().collect()
}

/// A module's health, from its structural checks (decisions D14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleStatus {
    Ok,
    /// Shorter than its file list says.
    Incomplete {
        missing_bytes: u64,
        entries_past_end: u32,
    },
    /// Some other check failed.
    Damaged {
        first: String,
        other_checks: usize,
    },
}

impl ModuleStatus {
    pub fn of(m: &ModuleRecord) -> ModuleStatus {
        let Some(first) = m.issues.first() else { return ModuleStatus::Ok };
        let past_end = m.entries_past_end();
        if past_end > 0 && m.data_end > m.stamp.size {
            return ModuleStatus::Incomplete { missing_bytes: m.data_end - m.stamp.size, entries_past_end: past_end };
        }
        ModuleStatus::Damaged { first: first.first.clone(), other_checks: m.issues.len() - 1 }
    }
}
