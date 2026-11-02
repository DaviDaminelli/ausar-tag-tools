//! Survey for PRD Q5 (which copy of a tag the game uses when copies differ)
//! and Q8 (whether any tag needs data merged from several copies), before
//! `map_tags` / `which_maps`. Counts only; the conclusions go to format-notes.
//! Ignored by default.
//!
//! ```text
//! H5_DEPLOY=<folder> cargo test --release -p h5tags-core --test real_layer_survey -- --ignored --nocapture
//! ```

mod common;

use std::collections::{BTreeMap, HashMap, HashSet};

use h5_meta::Body;
use h5_module::index::ModuleRecord;
use h5_module::names::{module_layer, module_stack};
use h5_module::{EntryKind, GlobalId, Module, map_file};
use h5tags_core::catalog::{Build, Copy};
use h5tags_core::references::header_scan;
use h5tags_core::stored::{load_definition, read_stored_tag};

/// (stack, layer) of a module path: `any\levels\globals-rtx-11587-1.module` →
/// (`any\levels\globals`, 11587); the base `…-rtx-1` is layer 0.
fn stack_and_layer(rel_path: &str) -> (String, u32) {
    (module_stack(rel_path).to_owned(), module_layer(rel_path).version)
}

/// The global IDs in the module's load manifest, in file order.
fn manifest(b: &Build, m: &ModuleRecord) -> Option<Vec<GlobalId>> {
    let map = map_file(&m.path_in(&b.config.deploy)).unwrap();
    Module::parse(&map).unwrap().read_load_manifest().unwrap()
}

fn has_problem(body: &Body<'_>) -> bool {
    use h5_meta::Content;
    body.problem.is_some()
        || body.children.iter().any(|n| match &n.content {
            Content::Struct(b) | Content::Array(b) => has_problem(b),
            Content::Block(k) => k.problem.is_some() || k.entries.iter().any(has_problem),
            _ => false,
        })
}

#[test]
#[ignore]
fn module_layers_and_copies() {
    let (s, _cache) = common::real_session("core-layer-survey");
    let b = &s.builds()[0];
    let layer_of: Vec<(String, u32)> = b.index.modules.iter().map(|m| stack_and_layer(&m.rel_path)).collect();

    println!("== modules by stack (layer: entries, tags, manifest)");
    let mut stacks: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, (stack, _)) in layer_of.iter().enumerate() {
        stacks.entry(stack).or_default().push(i);
    }
    for (stack, ms) in &stacks {
        let row: Vec<String> = ms
            .iter()
            .map(|&i| {
                let m = &b.index.modules[i];
                format!("{}: {}, {}, {}", layer_of[i].1, m.entries.len(), m.tag_count(), m.loaded_tag_count)
            })
            .collect();
        println!("{stack}  {}", row.join(" | "));
    }

    // A tag's lowest layer: the layer it first appears in.
    let first_layer = |t: &h5tags_core::catalog::Tag| t.copies.iter().map(|c| layer_of[c.module as usize].1).min();
    let layer_only: HashSet<GlobalId> =
        b.tags.iter().filter(|t| first_layer(t).is_some_and(|l| l > 0)).map(|t| t.global_id).collect();
    // Stacks whose base module isn't indexed: a tag found there only in a
    // layer may be in that missing base.
    let no_base: HashSet<&str> =
        stacks.iter().filter(|(_, ms)| ms.iter().all(|&i| layer_of[i].1 > 0)).map(|(s, _)| *s).collect();
    let in_no_base = |t: &h5tags_core::catalog::Tag| {
        t.copies.iter().any(|c| no_base.contains(layer_of[c.module as usize].0.as_str()))
    };
    let layer_only_sure = b.tags.iter().filter(|t| layer_only.contains(&t.global_id) && !in_no_base(t)).count();
    println!(
        "tags: {}; only in update layers: {} ({layer_only_sure} of them in no stack whose base is missing: {no_base:?})",
        b.tags.len(),
        layer_only.len()
    );

    println!("\n== load manifests (against the stack's lowest layer)");
    for ms in stacks.values() {
        let lists: Vec<(usize, Vec<GlobalId>)> =
            ms.iter().filter_map(|&i| manifest(b, &b.index.modules[i]).map(|l| (i, l))).collect();
        let Some((_, first)) = lists.first() else { continue };
        let first_set: HashSet<GlobalId> = first.iter().copied().collect();
        for (k, (i, list)) in lists.iter().enumerate() {
            let sorted = list.windows(2).all(|w| (w[0].0 as u32) < (w[1].0 as u32));
            let same_as_previous = k > 0 && lists[k - 1].1 == *list;
            let same_set_as_previous = k > 0
                && lists[k - 1].1.iter().copied().collect::<HashSet<GlobalId>>() == list.iter().copied().collect();
            let listed_layer_only_sure = list
                .iter()
                .filter(|g| layer_only.contains(g) && b.tag_by_id(**g).is_some_and(|t| !in_no_base(t)))
                .count();
            println!(
                "{}: sorted by ID: {sorted}; same as the layer below: order {same_as_previous}, set {same_set_as_previous}; only in layers and not in a base-less stack: {listed_layer_only_sure}",
                b.index.modules[*i].rel_path
            );
            let set: HashSet<GlobalId> = list.iter().copied().collect();
            let added: Vec<GlobalId> = set.difference(&first_set).copied().collect();
            let removed = first_set.difference(&set).count();
            let indexed = list.iter().filter(|g| b.tag_by_id(**g).is_some()).count();
            let listed_layer_only = list.iter().filter(|g| layer_only.contains(g)).count();
            let added_layer_only = added.iter().filter(|g| layer_only.contains(g)).count();
            let added_indexed = added.iter().filter(|g| b.tag_by_id(**g).is_some()).count();
            let common = first.iter().zip(list.iter()).take_while(|(a, z)| a == z).count();
            println!(
                "{}  {} ids ({} distinct), {indexed} indexed, {listed_layer_only} only in update layers; vs lowest: +{} (indexed {added_indexed}, only in layers {added_layer_only}) -{removed}; same order for the first {common}",
                b.index.modules[*i].rel_path,
                list.len(),
                set.len(),
                added.len()
            );
        }
    }

    println!("\n== copies");
    let mut by_layers: BTreeMap<Vec<u32>, usize> = BTreeMap::new();
    let (mut complete, mut incomplete, mut unreadable, mut no_def) = (0, 0, 0, 0);
    let (mut fewer_resources, mut layer_without_resources, mut more_resources) = (0, 0, 0);
    let mut examples: Vec<String> = Vec::new();
    for t in b.tags.iter().filter(|t| t.copies.len() > 1) {
        let layers: Vec<u32> = t.copies.iter().map(|c| layer_of[c.module as usize].1).collect();
        *by_layers.entry(layers.clone()).or_default() += 1;
        let def = load_definition(b, t).ok().flatten();
        for &c in &t.copies {
            let Ok(stored) = read_stored_tag(b, c) else {
                unreadable += 1;
                continue;
            };
            let Some(def) = &def else {
                no_def += 1;
                continue;
            };
            let read = h5_meta::read_tag(def, &stored.header, &stored.tag_data);
            if read.divergences.is_empty() && !has_problem(&read.root) {
                complete += 1;
            } else {
                incomplete += 1;
            }
        }
        // Resources per copy, against the copy in the lowest layer.
        let res: Vec<usize> = t.copies.iter().map(|&c| b.entry(c).resources.len()).collect();
        let base = layers.iter().enumerate().min_by_key(|(_, l)| **l).map(|(k, _)| k).unwrap();
        for (_, &n) in res.iter().enumerate().filter(|(k, _)| *k != base) {
            if n < res[base] {
                fewer_resources += 1;
                layer_without_resources += usize::from(n == 0);
                if examples.len() < 6 {
                    examples.push(format!("{} {}: resources {:?} in layers {layers:?}", t.class, t.path, res));
                }
            } else if n > res[base] {
                more_resources += 1;
            }
        }
    }
    println!("tags with copies, by the layers of their copies: {by_layers:?}");
    println!(
        "copies read with their definition: {complete} complete, {incomplete} with a problem or divergence, {unreadable} unreadable, {no_def} without a definition"
    );
    println!(
        "copies above the lowest with fewer resources: {fewer_resources} (none at all: {layer_without_resources}); with more: {more_resources}"
    );
    for e in &examples {
        println!("  {e}");
    }

    println!("\n== entries whose data isn't stored in their module");
    let stored_names: HashMap<&str, Vec<usize>> = {
        let mut m: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, module) in b.index.modules.iter().enumerate() {
            for e in module.entries.iter().filter(|e| e.stored) {
                m.entry(e.name.as_str()).or_default().push(i);
            }
        }
        m
    };
    let mut unstored: BTreeMap<(u32, &str), (usize, usize)> = BTreeMap::new();
    for (i, module) in b.index.modules.iter().enumerate() {
        for e in module.entries.iter().filter(|e| !e.stored) {
            let kind = e.kind().as_str();
            let row = unstored.entry((layer_of[i].1, kind)).or_default();
            row.0 += 1;
            row.1 += usize::from(stored_names.get(e.name.as_str()).is_some_and(|ms| ms.iter().any(|&j| j != i)));
        }
    }
    for ((layer, kind), (n, elsewhere)) in &unstored {
        println!(
            "layer {layer} {kind}: {n} not stored; {elsewhere} of them stored under the same name in another module"
        );
    }

    println!("\n== references to tags newer than the referring copy");
    let (scan, _) = header_scan(b);
    // layer → (references, to a tag first found in a later layer, of those in a base-less stack)
    let mut newer: BTreeMap<u32, (usize, usize, usize)> = BTreeMap::new();
    let mut newer_examples: Vec<String> = Vec::new();
    // Only targets stored in the referring copy's own stack: another stack
    // (a map, the platform folder) says nothing about the order of layers.
    for (mi, (stack, layer)) in layer_of.iter().enumerate() {
        for (ei, e) in b.index.modules[mi].entries.iter().enumerate() {
            if e.kind() != EntryKind::Tag {
                continue;
            }
            let Some(out) = scan.outgoing(Copy { module: mi as u32, entry: ei as u32 }) else { continue };
            let row = newer.entry(*layer).or_default();
            for r in &out.references {
                let Some(target) = b.tag_by_id(r.target) else { continue };
                if !target.copies.iter().all(|c| layer_of[c.module as usize].0 == *stack) {
                    continue;
                }
                row.0 += 1;
                if first_layer(target).is_some_and(|l| l > *layer) {
                    row.1 += 1;
                    if in_no_base(target) {
                        row.2 += 1;
                    } else if newer_examples.len() < 6 {
                        newer_examples.push(format!("{} → {} {}", e.name, target.class, target.path));
                    }
                }
            }
        }
    }
    for (layer, (refs, n, no_base_n)) in &newer {
        println!(
            "copies in layer {layer}: {refs} references to tags of the same stack, {n} to a tag first found in a later layer ({no_base_n} of them to a tag in a stack whose base is missing)"
        );
    }
    for e in &newer_examples {
        println!("  {e}");
    }
}

/// Where could a stack order be stored inside the modules (no `.mapinfo` on
/// Xbox)? Lists every non-tag file entry, and every tag or file whose bytes
/// (header and tag data; resources aren't searched) contain a module-like
/// name: `-rtx-`, `.module`, `mapinfo`.
#[test]
#[ignore]
fn stack_names_inside_modules() {
    let (s, _cache) = common::real_session("core-stack-names-survey");
    let b = &s.builds()[0];
    let needles: [&[u8]; 3] = [b"-rtx-", b".module", b"mapinfo"];
    let mut hits: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut searched = 0u64;
    for m in &b.index.modules {
        let map = map_file(&m.path_in(&b.config.deploy)).unwrap();
        let module = Module::parse(&map).unwrap();
        for (i, e) in m.entries.iter().enumerate() {
            let parts: Vec<Vec<u8>> = match e.kind() {
                EntryKind::File => {
                    println!("file entry: {} {} ({} bytes)", m.rel_path, e.name, e.uncompressed_size);
                    vec![module.read_entry(i as u32).unwrap_or_default()]
                }
                EntryKind::Tag => [h5_module::Section::Header, h5_module::Section::TagData]
                    .iter()
                    .map(|&sec| module.read_section(i as u32, sec).unwrap_or_default())
                    .collect(),
                _ => continue,
            };
            for bytes in &parts {
                searched += bytes.len() as u64;
                for n in needles {
                    let Some(at) = bytes.windows(n.len()).position(|w| w.eq_ignore_ascii_case(n)) else { continue };
                    // The printable run around the hit.
                    let start = bytes[..at].iter().rposition(|c| !c.is_ascii_graphic()).map_or(0, |p| p + 1);
                    let end = bytes[at..].iter().position(|c| !c.is_ascii_graphic()).map_or(bytes.len(), |p| at + p);
                    let text = String::from_utf8_lossy(&bytes[start..end.min(start + 160)]).into_owned();
                    hits.entry(format!("{} {}", e.class_code, e.name)).or_default().push(text);
                }
            }
        }
    }
    println!("searched {:.1} MB of headers, tag data and files", searched as f64 / 1e6);
    println!("{} entries with a module-like name:", hits.len());
    for (entry, texts) in hits.iter().take(40) {
        println!("  {entry}: {texts:?}");
    }
}

/// The module header's `id` and `buildVersionId` per module, in path order:
/// do they order a stack's layers?
#[test]
#[ignore]
fn module_header_ids() {
    let (s, _cache) = common::real_session("core-module-ids-survey");
    let b = &s.builds()[0];
    for m in &b.index.modules {
        let map = map_file(&m.path_in(&b.config.deploy)).unwrap();
        let h = Module::parse(&map).unwrap().into_lists().header;
        println!("{:<55} id {:016X} ({:>20})  build id {:016X}", m.rel_path, h.id, h.id, h.build_version_id);
    }
}
