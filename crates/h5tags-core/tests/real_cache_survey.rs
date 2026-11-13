//! Full-build survey from cache files alone (no game files on disk): what a
//! build's index, header scan and definition check hold, and how two builds
//! relate (M6). Counts only. Ignored by default.
//!
//! ```text
//! H5_CACHES=<folder with <a>.index/.scan/.check and <b>.…> H5_BUILDS=forge-12719,xbox-97230 \
//!   cargo test --release -p h5tags-core --test real_cache_survey -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use h5_module::index::{EntryKind, Index};
use h5_module::names::{latest_layer, module_layer, module_stack, split_tag_name};
use h5_module::scan::HeaderScan;
use h5_module::{FourCc, GlobalId};
use h5tags_core::def_check::DefinitionCheck;

/// A tag as the index lists it: class and path (first copy's).
struct Seen {
    class: FourCc,
    path: String,
    copies: usize,
}

fn tags(index: &Index) -> HashMap<GlobalId, Seen> {
    let mut out: HashMap<GlobalId, Seen> = HashMap::new();
    for m in &index.modules {
        for e in m.entries.iter().filter(|e| e.kind() == EntryKind::Tag) {
            out.entry(e.global_id).and_modify(|s| s.copies += 1).or_insert_with(|| Seen {
                class: e.class_code,
                path: split_tag_name(&e.name).0.to_lowercase(),
                copies: 1,
            });
        }
    }
    out
}

/// `path` without its `{…}` parts (platform tags such as `{pc}`).
fn without_braces(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut depth = 0;
    for c in path.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// `path` without a leading platform folder (`__chore\pc__\`, `__chore\x1__\`)
/// and its `{…}` parts.
fn platform_free(path: &str) -> String {
    let rest = path.strip_prefix("__chore\\").and_then(|p| p.split_once("__\\")).map_or(path, |(_, rest)| rest);
    without_braces(rest)
}

/// How two paths of one global ID differ (M6 step 3).
fn path_change(a: &str, b: &str) -> &'static str {
    if without_braces(a) == without_braces(b) {
        return "a {…} part only";
    }
    if platform_free(a) == platform_free(b) {
        return "platform folder and {…} part only";
    }
    fn split(p: &str) -> (&str, &str) {
        p.rsplit_once('\\').unwrap_or(("", p))
    }
    let ((fa, na), (fb, nb)) = (split(a), split(b));
    match (fa == fb, without_braces(na) == without_braces(nb)) {
        (false, true) => "same name, another folder",
        (true, false) => "same folder, another name",
        _ => "another folder and name",
    }
}

/// M6 step 3: the global IDs both builds have, but under another path. Index
/// files only (no game files): `H5_CACHES`, `H5_BUILDS` as above.
#[test]
#[ignore]
fn shared_ids_with_another_path() {
    let dir = PathBuf::from(std::env::var("H5_CACHES").expect("set H5_CACHES"));
    let names: Vec<String> =
        std::env::var("H5_BUILDS").expect("set H5_BUILDS=a,b").split(',').map(str::to_owned).collect();
    let [a, b] = [&names[0], &names[1]].map(|n| tags(&Index::load(&dir.join(format!("{n}.index"))).expect("index")));
    /// Per kind of change: how many, by class, and a few (path in a, path in b).
    type Kind = (usize, BTreeMap<FourCc, usize>, Vec<(String, String)>);
    let mut kinds: BTreeMap<&str, Kind> = BTreeMap::new();
    for (id, s) in &a {
        let Some(t) = b.get(id) else { continue };
        if s.path == t.path {
            continue;
        }
        let k = kinds.entry(path_change(&s.path, &t.path)).or_default();
        k.0 += 1;
        *k.1.entry(s.class).or_default() += 1;
        if k.2.len() < 4 {
            k.2.push((s.path.clone(), t.path.clone()));
        }
    }
    // The other way: the same path (without platform parts) and class under another ID.
    let by_path: std::collections::HashMap<(String, FourCc), GlobalId> =
        b.iter().map(|(id, t)| ((platform_free(&t.path), t.class), *id)).collect();
    let other_id: Vec<(&GlobalId, GlobalId, &Seen)> = a
        .iter()
        .filter(|(id, _)| !b.contains_key(id))
        .filter_map(|(id, s)| by_path.get(&(platform_free(&s.path), s.class)).map(|&o| (id, o, s)))
        .collect();
    println!("only in one build by ID, but the same path and class in both: {}", other_id.len());
    for (id, o, s) in other_id.iter().take(4) {
        println!("   {} {} ({}) / {}", s.class, s.path, id, o);
    }
    let total: usize = kinds.values().map(|k| k.0).sum();
    println!("shared IDs with another path: {total} ({} vs {})", names[0], names[1]);
    for (kind, (n, classes, examples)) in kinds {
        println!("== {kind}: {n}  by class: {}", top(classes, 8));
        for (x, y) in examples {
            println!("   {x}\n   {y}\n");
        }
    }
}

fn top(counts: BTreeMap<FourCc, usize>, n: usize) -> String {
    let mut v: Vec<(FourCc, usize)> = counts.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    let parts: Vec<String> = v.iter().take(n).map(|(c, k)| format!("{c} {k}")).collect();
    format!("{}{}", parts.join(", "), if v.len() > n { format!(" (+{} classes)", v.len() - n) } else { String::new() })
}

/// How many bytes the header scan and the definition check read (every
/// stored tag copy: at most its compressed bytes from disk, its uncompressed
/// bytes to decompress), to tell from their times whether a pass waits on the
/// disk or on the CPU.
#[test]
#[ignore]
fn bytes_the_scans_read() {
    let dir = PathBuf::from(std::env::var("H5_CACHES").expect("set H5_CACHES"));
    for name in std::env::var("H5_BUILDS").expect("set H5_BUILDS=a,b").split(',') {
        let index = Index::load(&dir.join(format!("{name}.index"))).expect("index");
        let tags = index.modules.iter().flat_map(|m| &m.entries).filter(|e| e.kind() == EntryKind::Tag && e.stored);
        let (copies, compressed, uncompressed) = tags.fold((0usize, 0u64, 0u64), |(n, c, u), e| {
            (n + 1, c + u64::from(e.compressed_size), u + e.uncompressed_size)
        });
        let on_disk: u64 = index.modules.iter().map(|m| m.stamp.size).sum();
        println!(
            "{name}: {copies} stored tag copies, {:.2} GB compressed, {:.2} GB uncompressed (modules {:.1} GB on disk)",
            compressed as f64 / 1e9,
            uncompressed as f64 / 1e9,
            on_disk as f64 / 1e9
        );
    }
}

/// M9 survey (a field across a tag's update layers): on the full builds, the
/// tags stored in several modules, how many distinct data versions their
/// copies hold (the scan's data fingerprint), by class.
#[test]
#[ignore]
fn layered_tags_in_the_full_builds() {
    let dir = PathBuf::from(std::env::var("H5_CACHES").expect("set H5_CACHES"));
    for name in std::env::var("H5_BUILDS").expect("set H5_BUILDS=a,b").split(',') {
        let index = Index::load(&dir.join(format!("{name}.index"))).expect("index");
        let t = tags(&index);
        let by_copies: BTreeMap<usize, usize> =
            t.values().filter(|s| s.copies > 1).fold(BTreeMap::new(), |mut m, s| {
                *m.entry(s.copies).or_default() += 1;
                m
            });
        println!("\n== {name}: tags {}, with copies by count {by_copies:?}", t.len());
        // Update layers are modules of one stack; copies in several stacks are
        // the same tag stored for several maps.
        let mut layers: HashMap<(GlobalId, &str), HashSet<u32>> = HashMap::new();
        for m in &index.modules {
            let (stack, layer) = (module_stack(&m.rel_path), module_layer(&m.rel_path).version);
            for e in m.entries.iter().filter(|e| e.kind() == EntryKind::Tag) {
                layers.entry((e.global_id, stack)).or_default().insert(layer);
            }
        }
        let most_layers: HashMap<GlobalId, usize> = layers.iter().fold(HashMap::new(), |mut m, ((id, _), l)| {
            let e = m.entry(*id).or_default();
            *e = (*e).max(l.len());
            m
        });
        let stacks: HashMap<GlobalId, usize> = layers.keys().fold(HashMap::new(), |mut m, (id, _)| {
            *m.entry(*id).or_default() += 1;
            m
        });
        let in_layers = |n: usize| most_layers.values().filter(|&&l| l >= n).count();
        let weap = FourCc::from_code("weap").unwrap();
        let weap_layers = |n: usize| most_layers.iter().filter(|(id, l)| **l >= n && t[*id].class == weap).count();
        println!(
            "  in 2+ layers of one stack {} (weapons {}), 3+ layers {} (weapons {}); in 2+ stacks {}",
            in_layers(2),
            weap_layers(2),
            in_layers(3),
            weap_layers(3),
            stacks.values().filter(|&&n| n >= 2).count()
        );
        let layered_classes = most_layers.iter().filter(|(_, l)| **l >= 2).fold(BTreeMap::new(), |mut m, (id, _)| {
            *m.entry(t[id].class).or_default() += 1;
            m
        });
        println!("  in 2+ layers by class: {}", top(layered_classes, 12));
        let Some(scan) = HeaderScan::load(&dir.join(format!("{name}.scan"))) else {
            println!("header scan: missing or another format");
            continue;
        };
        let mut prints: HashMap<GlobalId, HashSet<(u64, u64)>> = HashMap::new();
        for m in &scan.modules {
            for c in &m.tags {
                prints.entry(c.global_id).or_default().insert(c.data_print);
            }
        }
        let prints = &prints;
        let layered =
            |n: usize| t.iter().filter(move |(id, s)| s.copies > 1 && prints.get(id).is_some_and(|p| p.len() >= n));
        let by_class = |n: usize| {
            layered(n).fold(BTreeMap::new(), |mut m: BTreeMap<FourCc, usize>, (_, s)| {
                *m.entry(s.class).or_default() += 1;
                m
            })
        };
        let weap = FourCc::from_code("weap").unwrap();
        println!(
            "with copies holding 2+ versions {} (weapons {}), 3+ versions {} (weapons {})",
            layered(2).count(),
            layered(2).filter(|(_, s)| s.class == weap).count(),
            layered(3).count(),
            layered(3).filter(|(_, s)| s.class == weap).count()
        );
        println!("  2+ versions by class: {}", top(by_class(2), 12));
        println!("  3+ versions by class: {}", top(by_class(3), 12));
    }
}

#[test]
#[ignore]
fn full_builds_from_their_caches() {
    let dir = PathBuf::from(std::env::var("H5_CACHES").expect("set H5_CACHES"));
    let names: Vec<String> =
        std::env::var("H5_BUILDS").expect("set H5_BUILDS=a,b").split(',').map(str::to_owned).collect();
    let mut all: Vec<HashMap<GlobalId, Seen>> = Vec::new();
    for name in &names {
        let index = Index::load(&dir.join(format!("{name}.index"))).expect("index");
        let t = tags(&index);
        let versions: BTreeMap<u32, usize> = index.modules.iter().fold(BTreeMap::new(), |mut m, r| {
            *m.entry(r.version).or_default() += 1;
            m
        });
        let manifests = index.modules.iter().filter(|m| m.has_loadmanifest()).count();
        let latest = latest_layer(index.modules.iter().map(|m| m.rel_path.as_str()));
        let copies: usize = t.values().map(|s| s.copies).sum();
        let gb: f64 = index.modules.iter().map(|m| m.stamp.size as f64).sum::<f64>() / 1e9;
        println!(
            "\n== {name}: {} modules ({gb:.1} GB), formats {versions:?}, latest layer {latest:?}",
            index.modules.len()
        );
        println!(
            "tags {} ({copies} copies), modules with a load manifest {manifests}, not indexed {}",
            t.len(),
            index.failed.len()
        );
        // M5d step 9: may `/` stand for `\` in a typed path? Only if no name holds one.
        let slash_entries = index.modules.iter().flat_map(|m| &m.entries).filter(|e| e.name.contains('/')).count();
        let slash_modules = index.modules.iter().filter(|m| m.rel_path.contains('/')).count();
        println!("entry names with '/': {slash_entries}; module paths with '/': {slash_modules}");
        let by_class: BTreeMap<FourCc, usize> = t.values().fold(BTreeMap::new(), |mut m, s| {
            *m.entry(s.class).or_default() += 1;
            m
        });
        println!("classes {}: {}", by_class.len(), top(by_class, 10));
        let scnr = FourCc::from_code("scnr").unwrap();
        let scenarios: Vec<&str> = t.values().filter(|s| s.class == scnr).map(|s| s.path.as_str()).collect();
        println!("scenarios {}", scenarios.len());

        if let Some(scan) = HeaderScan::load(&dir.join(format!("{name}.scan"))) {
            let scanned: usize = scan.modules.iter().map(|m| m.tags.len()).sum();
            let problems: usize = scan.modules.iter().map(|m| m.problems.len()).sum();
            let refs: usize = scan.modules.iter().flat_map(|m| &m.tags).map(|t| t.references.len()).sum();
            println!(
                "header scan: {} modules, {scanned} copies, {problems} unreadable, {refs} references",
                scan.modules.len()
            );
        } else {
            println!("header scan: missing or another format");
        }
        if let Some(check) = DefinitionCheck::load(&dir.join(format!("{name}.check"))) {
            let all_tags: Vec<_> = check.modules.iter().flat_map(|m| &m.tags).collect();
            let size: BTreeMap<FourCc, usize> =
                all_tags.iter().filter(|t| !t.size_mismatches.is_empty()).fold(BTreeMap::new(), |mut m, t| {
                    *m.entry(t.class).or_default() += 1;
                    m
                });
            let other = all_tags.iter().filter(|t| !t.other.is_empty() || t.unreadable.is_some()).count();
            let guid = all_tags.iter().filter(|t| !t.guid_mismatches.is_empty()).count();
            println!(
                "definition check: {} modules, {} copies; size mismatches {} copies in {} classes: {}; GUID mismatches {guid}; other problems {other}",
                check.modules.len(),
                all_tags.len(),
                size.values().sum::<usize>(),
                size.len(),
                top(size, 12)
            );
            // Where the mismatches are: the main struct (an empty path) or a tag block.
            let mut paths: BTreeMap<(FourCc, &str), usize> = BTreeMap::new();
            for t in &all_tags {
                for p in &t.size_mismatches {
                    *paths.entry((t.class, p.as_str())).or_default() += 1;
                }
            }
            for ((class, p), n) in &paths {
                println!("  size mismatch {class} {}: {n} copies", if p.is_empty() { "(main struct)" } else { p });
            }
        } else {
            println!("definition check: missing or another format");
        }
        all.push(t);
    }
    if let [a, b] = all.as_slice() {
        let shared: Vec<GlobalId> = a.keys().filter(|id| b.contains_key(id)).copied().collect();
        let same_class = shared.iter().filter(|id| a[id].class == b[id].class).count();
        let same_path = shared.iter().filter(|id| a[id].path == b[id].path).count();
        let only = |x: &HashMap<GlobalId, Seen>, y: &HashMap<GlobalId, Seen>| -> BTreeMap<FourCc, usize> {
            x.iter().filter(|(id, _)| !y.contains_key(id)).fold(BTreeMap::new(), |mut m, (_, s)| {
                *m.entry(s.class).or_default() += 1;
                m
            })
        };
        let (oa, ob) = (only(a, b), only(b, a));
        println!("\n== {} vs {}", names[0], names[1]);
        println!("shared IDs {}: same class {same_class}, same path {same_path}", shared.len());
        println!("only in {}: {}  {}", names[0], oa.values().sum::<usize>(), top(oa.clone(), 10));
        println!("only in {}: {}  {}", names[1], ob.values().sum::<usize>(), top(ob.clone(), 10));
        let scnr = FourCc::from_code("scnr").unwrap();
        let only_scn = |x: &HashMap<GlobalId, Seen>, y: &HashMap<GlobalId, Seen>| -> Vec<String> {
            let mut v: Vec<String> = x
                .iter()
                .filter(|(id, s)| s.class == scnr && !y.contains_key(id))
                .map(|(_, s)| s.path.clone())
                .collect();
            v.sort();
            v
        };
        let (sa, sb) = (only_scn(a, b), only_scn(b, a));
        let shared_scn = shared.iter().filter(|id| a[id].class == scnr).count();
        println!(
            "scenarios: shared {shared_scn}; only in {} {}; only in {} {}",
            names[0],
            sa.len(),
            names[1],
            sb.len()
        );
        let campaign: HashSet<&str> = sb.iter().filter(|p| p.contains("campaign")).map(String::as_str).collect();
        println!(
            "  only in {} with 'campaign' in the path: {}; first: {:?}",
            names[1],
            campaign.len(),
            &sb[..sb.len().min(6)]
        );
    }
}
