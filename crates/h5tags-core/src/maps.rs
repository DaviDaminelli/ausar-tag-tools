//! `map_tags` / `which_maps` (PRD §6.6): the tags a map loads, from its
//! modules' load manifests. Domain only. A map is a stack of modules (base and
//! update layers); each layer's manifest is a full list, and the latest
//! layer's is the one in effect (format-notes "Layers and load manifests", D59).

use std::collections::BTreeMap;

use h5_module::names::{module_layer, module_stack};
use h5_module::{FourCc, GlobalId};

use crate::catalog::{Build, Tag};
use crate::filters::ModuleFilter;
use crate::stored::{ReadError, read_manifest};

/// For each map, the module of its latest layer that has a load manifest; by
/// module path.
pub fn manifests_in_effect(b: &Build) -> Vec<u32> {
    let mut latest: BTreeMap<&str, u32> = BTreeMap::new();
    for (i, m) in b.index.modules.iter().enumerate().filter(|(_, m)| m.has_loadmanifest()) {
        let slot = latest.entry(module_stack(&m.rel_path)).or_insert(i as u32);
        if module_layer(&m.rel_path) > module_layer(&b.index.modules[*slot as usize].rel_path) {
            *slot = i as u32;
        }
    }
    let mut modules: Vec<u32> = latest.into_values().collect();
    modules.sort_unstable();
    modules
}

#[derive(Debug)]
pub enum PickError<'b> {
    /// No module accepted by the filter has a load manifest.
    NoManifest,
    /// The accepted modules belong to several maps (their stacks).
    SeveralMaps(Vec<&'b str>),
}

/// The module whose manifest `map_tags` reads, among those `matches` accepts
/// (by module path) that have one: the only one, or the latest layer when all
/// are layers of one map.
pub fn pick_manifest<'b>(b: &'b Build, matches: impl Fn(&str) -> bool) -> Result<u32, PickError<'b>> {
    let found: Vec<u32> = (0..b.index.modules.len() as u32)
        .filter(|&i| {
            let m = &b.index.modules[i as usize];
            m.has_loadmanifest() && matches(&m.rel_path)
        })
        .collect();
    let path = |i: u32| b.index.modules[i as usize].rel_path.as_str();
    let mut stacks: Vec<&str> = found.iter().map(|&i| module_stack(path(i))).collect();
    stacks.sort_unstable();
    stacks.dedup();
    match stacks.len() {
        0 => Err(PickError::NoManifest),
        1 => Ok(found.into_iter().max_by_key(|&i| module_layer(path(i))).expect("one module at least")),
        _ => Err(PickError::SeveralMaps(stacks)),
    }
}

/// The tags in a load manifest.
pub struct MapTags<'b> {
    /// The indexed tags it lists (of `class`, if given), by class, then path.
    pub tags: Vec<&'b Tag>,
    /// How many of its IDs are no indexed tag (stored in modules not indexed).
    pub not_indexed: usize,
}

/// The tags module `module`'s load manifest lists (a module without one lists none).
pub fn map_tags(b: &Build, module: u32, class: Option<FourCc>) -> Result<MapTags<'_>, ReadError> {
    let ids = read_manifest(b, module)?.unwrap_or_default();
    let mut not_indexed = 0;
    let mut tags: Vec<&Tag> = Vec::with_capacity(ids.len());
    for &id in ids.iter() {
        match b.tag_by_id(id) {
            Some(t) if class.is_none_or(|c| t.class == c) => tags.push(t),
            Some(_) => {}
            None => not_indexed += 1,
        }
    }
    tags.sort_unstable_by(|x, y| x.class.cmp(&y.class).then_with(|| x.path_lower.cmp(&y.path_lower)));
    Ok(MapTags { tags, not_indexed })
}

/// The maps whose manifest in effect lists a tag.
pub struct WhichMaps {
    /// Their manifest modules, by module path.
    pub modules: Vec<u32>,
    /// Manifests that couldn't be read: the tag may be in them too.
    pub failed: Vec<(u32, ReadError)>,
    /// How many maps' manifests were looked at.
    pub checked: usize,
}

/// The maps whose manifest in effect lists `id`; with `module`, only maps whose
/// manifest module path matches it, chosen before any manifest is read (D76).
pub fn which_maps(b: &Build, id: GlobalId, module: Option<&ModuleFilter>) -> WhichMaps {
    let mut in_effect = manifests_in_effect(b);
    if let Some(f) = module {
        in_effect.retain(|&m| f.matches(&b.index.modules[m as usize].rel_path));
    }
    let mut found = WhichMaps { modules: Vec::new(), failed: Vec::new(), checked: in_effect.len() };
    for module in in_effect {
        match read_manifest(b, module) {
            Ok(ids) if ids.as_ref().is_some_and(|ids| ids.contains(&id)) => found.modules.push(module),
            Ok(_) => {}
            Err(e) => found.failed.push((module, e)),
        }
    }
    found
}

#[cfg(test)]
#[path = "maps_tests.rs"]
mod tests;
