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
use h5_module::names::{latest_layer, split_tag_name};
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

fn top(counts: BTreeMap<FourCc, usize>, n: usize) -> String {
    let mut v: Vec<(FourCc, usize)> = counts.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    let parts: Vec<String> = v.iter().take(n).map(|(c, k)| format!("{c} {k}")).collect();
    format!("{}{}", parts.join(", "), if v.len() > n { format!(" (+{} classes)", v.len() - n) } else { String::new() })
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
            let other = all_tags.iter().filter(|t| t.other_problems > 0 || t.unreadable.is_some()).count();
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
