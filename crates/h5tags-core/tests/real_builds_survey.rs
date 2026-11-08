//! M6 survey: how two builds relate, before `compare_builds` and cross-build
//! `diff_tags`. Do tags keep their global ID and path across builds? Which
//! exist in one build only (the campaign scenarios, for M6's done-when)? Do
//! class layouts differ? How many shared tags hold different data? Counts
//! only; conclusions go to format-notes. Ignored by default.
//!
//! ```text
//! H5_CONFIG=<config with two builds> cargo test --release -p h5tags-core --test real_builds_survey -- --ignored --nocapture
//! ```
//! The first two builds of the config are compared (a, b).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use h5_module::{FourCc, GlobalId};
use h5tags_core::catalog::{Build, Tag};
use h5tags_core::def_check::definition_check;
use h5tags_core::query::choose_copy;
use h5tags_core::stored::read_stored_tag;
use h5tags_core::{Config, Session};

fn count_by_class<'t>(tags: impl Iterator<Item = &'t Tag>) -> Vec<(FourCc, usize)> {
    let mut counts: BTreeMap<FourCc, usize> = BTreeMap::new();
    for t in tags {
        *counts.entry(t.class).or_default() += 1;
    }
    let mut v: Vec<(FourCc, usize)> = counts.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v
}

fn top(v: &[(FourCc, usize)], n: usize) -> String {
    let parts: Vec<String> = v.iter().take(n).map(|(c, k)| format!("{c} {k}")).collect();
    format!("{}{}", parts.join(", "), if v.len() > n { format!(" (+{} classes)", v.len() - n) } else { String::new() })
}

/// Each class's distinct main struct GUIDs in a build (from the definition check).
fn layouts(b: &Build) -> HashMap<FourCc, HashSet<[u8; 16]>> {
    let (check, _) = definition_check(b);
    let mut out: HashMap<FourCc, HashSet<[u8; 16]>> = HashMap::new();
    for t in check.modules.iter().flat_map(|m| &m.tags) {
        if let Some(g) = t.main_guid {
            out.entry(t.class).or_default().insert(g);
        }
    }
    out
}

#[test]
#[ignore]
fn how_two_builds_relate() {
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG to a config with two builds"));
    let s = Session::open(Config::load(&config).unwrap()).unwrap();
    let (a, b) = (&s.builds()[0], &s.builds()[1]);
    println!("a = {} ({} tags), b = {} ({} tags)", a.name, a.tags.len(), b.name, b.tags.len());

    // A. By global ID.
    let ids_b: HashSet<GlobalId> = b.tags.iter().map(|t| t.global_id).collect();
    let ids_a: HashSet<GlobalId> = a.tags.iter().map(|t| t.global_id).collect();
    let shared: Vec<(&Tag, &Tag)> = a.tags.iter().filter_map(|t| b.tag_by_id(t.global_id).map(|u| (t, u))).collect();
    let only_a = count_by_class(a.tags.iter().filter(|t| !ids_b.contains(&t.global_id)));
    let only_b = count_by_class(b.tags.iter().filter(|t| !ids_a.contains(&t.global_id)));
    let total = |v: &[(FourCc, usize)]| v.iter().map(|x| x.1).sum::<usize>();
    println!("\n== A. by global ID");
    println!("in both: {}", shared.len());
    println!("only in {}: {}  by class: {}", a.name, total(&only_a), top(&only_a, 12));
    println!("only in {}: {}  by class: {}", b.name, total(&only_b), top(&only_b, 12));
    let same_path = shared.iter().filter(|(t, u)| t.path_lower == u.path_lower).count();
    let same_class = shared.iter().filter(|(t, u)| t.class == u.class).count();
    println!("shared IDs with the same path: {same_path}; the same class: {same_class}");
    for (t, u) in shared.iter().filter(|(t, u)| t.path_lower != u.path_lower || t.class != u.class).take(5) {
        println!("  differs: {} {} {} | {} {}", t.global_id, t.class, t.path, u.class, u.path);
    }

    // B. By path: the same path under another ID.
    let by_path_b: HashMap<(&str, FourCc), GlobalId> =
        b.tags.iter().map(|t| ((t.path_lower.as_str(), t.class), t.global_id)).collect();
    let moved: Vec<(&Tag, GlobalId)> = a
        .tags
        .iter()
        .filter_map(|t| {
            by_path_b.get(&(t.path_lower.as_str(), t.class)).filter(|&&id| id != t.global_id).map(|&id| (t, id))
        })
        .collect();
    println!("\n== B. same path and class, another global ID: {}", moved.len());
    for (t, id) in moved.iter().take(5) {
        println!("  {} {}: {} vs {}", t.class, t.path, t.global_id, id);
    }

    // C. Scenarios (M6 done-when: the campaign ones appear in one build only).
    let scnr = FourCc::from_code("scnr").unwrap();
    let scenarios = |x: &Build, other: &HashSet<GlobalId>| -> Vec<String> {
        x.tags.iter().filter(|t| t.class == scnr && !other.contains(&t.global_id)).map(|t| t.path.clone()).collect()
    };
    let (sa, sb) = (scenarios(a, &ids_b), scenarios(b, &ids_a));
    let both = shared.iter().filter(|(t, _)| t.class == scnr).count();
    println!("\n== C. scenarios: in both {both}; only in {} {}: {:?}", a.name, sa.len(), &sa[..sa.len().min(8)]);
    println!("   only in {} {}: {:?}", b.name, sb.len(), &sb[..sb.len().min(12)]);

    // D. Class layouts: main struct GUIDs per class in each build.
    let (la, lb) = (layouts(a), layouts(b));
    let mut same = 0;
    let mut differ: Vec<(FourCc, usize, usize)> = Vec::new();
    for (class, ga) in &la {
        let Some(gb) = lb.get(class) else { continue };
        if ga == gb {
            same += 1;
        } else {
            differ.push((*class, ga.len(), gb.len()));
        }
    }
    differ.sort();
    println!("\n== D. classes in both builds: main struct GUIDs equal {same}, different {}", differ.len());
    for (c, na, nb) in differ.iter().take(20) {
        println!("  {c}: {na} GUID(s) in {}, {nb} in {}", a.name, b.name);
    }

    // E. Data of shared tags (each build's default copy, D58).
    let start = Instant::now();
    let (mut identical, mut same_size, mut other_size, mut unreadable) = (0usize, 0usize, 0usize, 0usize);
    let mut differing: BTreeMap<FourCc, usize> = BTreeMap::new();
    let mut identical_by_class: BTreeMap<FourCc, usize> = BTreeMap::new();
    for (t, u) in &shared {
        let (Some(ca), Some(cb)) = (choose_copy(a, t, None), choose_copy(b, u, None)) else { continue };
        let (Ok(ra), Ok(rb)) = (read_stored_tag(a, ca), read_stored_tag(b, cb)) else {
            unreadable += 1;
            continue;
        };
        if ra.tag_data == rb.tag_data {
            identical += 1;
            *identical_by_class.entry(t.class).or_default() += 1;
            continue;
        }
        if ra.tag_data.len() == rb.tag_data.len() {
            same_size += 1
        } else {
            other_size += 1
        }
        *differing.entry(t.class).or_default() += 1;
    }
    let mut d: Vec<(FourCc, usize)> = differing.into_iter().collect();
    d.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!(
        "\n== E. shared tags' tag data: identical {identical}, same size but different {same_size}, other size {other_size}, unreadable {unreadable} ({:.1?})",
        start.elapsed()
    );
    println!("differing, by class: {}", top(&d, 15));
}
