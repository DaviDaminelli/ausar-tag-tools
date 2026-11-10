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

/// M6 step 4: shared tags compared by their tag data fingerprints (D81), each
/// build's default copy; the platform variants (another path) apart. The
/// scans are built in a fresh cache folder, not the config's.
#[test]
#[ignore]
fn shared_tags_by_fingerprint() {
    use h5tags_core::references::header_scan;
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG to a config with two builds"));
    let cache = h5_module::testutil::TempDir::new("builds-by-fingerprint");
    let mut config = Config::load(&config).unwrap();
    config.cache_dir = Some(cache.0.clone());
    let s = Session::open(config).unwrap();
    let (a, b) = (&s.builds()[0], &s.builds()[1]);
    let start = Instant::now();
    let (sa, sb) = (header_scan(a).0, header_scan(b).0);
    println!("both scans built: {:.1?}", start.elapsed());
    let print = |x: &Build, scan: &h5tags_core::references::ScanIndex, t: &Tag| {
        choose_copy(x, t, None).and_then(|c| scan.outgoing(c)).map(|s| s.data_print)
    };
    let start = Instant::now();
    let (mut same, mut different, mut variant_same, mut variant_different, mut unscanned) = (0, 0, 0, 0, 0);
    for t in &a.tags {
        let Some(u) = b.tag_by_id(t.global_id) else { continue };
        let (Some(p), Some(q)) = (print(a, sa, t), print(b, sb, u)) else {
            unscanned += 1;
            continue;
        };
        match (t.path_lower == u.path_lower, p == q) {
            (true, true) => same += 1,
            (true, false) => different += 1,
            (false, true) => variant_same += 1,
            (false, false) => variant_different += 1,
        }
    }
    println!(
        "shared, same path: same data {same}, different {different}; platform variants: same data {variant_same}, different {variant_different}; not scanned {unscanned} ({:.1?})",
        start.elapsed()
    );
}

/// M6 step 4: `compare_builds` as the agent reads it, on the first two builds
/// (scans built in a fresh cache folder), with how long each call took.
#[test]
#[ignore]
fn compare_builds_as_the_agent_reads_it() {
    use h5tags_core::output::Format;
    use h5tags_core::tools::args::PageArgs;
    use h5tags_core::tools::{CompareBuilds, RowStatus, Tool};
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG to a config with two builds"));
    let cache = h5_module::testutil::TempDir::new("compare-builds-real");
    let mut config = Config::load(&config).unwrap();
    config.cache_dir = Some(cache.0.clone());
    let mut s = Session::open(config).unwrap();
    let (a, b) = (s.builds()[0].name.clone(), s.builds()[1].name.clone());
    let calls: [(Option<&str>, Option<RowStatus>, usize); 3] =
        [(None, None, 3), (Some("weap"), Some(RowStatus::Different), 5), (Some("scnr"), None, 5)];
    for (class, status, limit) in calls {
        let t = CompareBuilds {
            build_a: a.clone(),
            build_b: b.clone(),
            class: class.map(Into::into),
            status,
            page: PageArgs { offset: 0, limit: Some(limit) },
        };
        let start = Instant::now();
        let text = t.run(&mut s).map(|o| o.render(Format::Text)).unwrap_or_else(|e| e.render(Format::Text));
        println!("== class {class:?}, status {status:?} ({:.0?})\n{text}", start.elapsed());
    }
}

/// M6 step 6 (D84): what the tag line's note costs. The shared tags with the
/// most data in build b's default copy, each compared as `read_tag` does after
/// reading the tag in a (its copy already in the tag cache): the time is b's
/// read and the comparison. Each status checked against the fingerprints.
#[test]
#[ignore]
fn the_tag_line_note_on_the_largest_shared_tags() {
    use h5tags_core::compare::{Status, tag_in_other_build};
    use h5tags_core::references::header_scan;
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG to a config with two builds"));
    let cache = h5_module::testutil::TempDir::new("tag-line-note");
    let mut config = Config::load(&config).unwrap();
    config.cache_dir = Some(cache.0.clone());
    let s = Session::open(config).unwrap();
    let (a, b) = (&s.builds()[0], &s.builds()[1]);
    let mut shared: Vec<(&Tag, &Tag, u64)> = a
        .tags
        .iter()
        .filter_map(|t| {
            let u = b.tag_by_id(t.global_id)?;
            Some((t, u, b.entry(choose_copy(b, u, None)?).uncompressed_size))
        })
        .collect();
    shared.sort_by_key(|x| std::cmp::Reverse(x.2));
    let (sa, sb) = (header_scan(a).0, header_scan(b).0);
    for (t, u, size) in shared.iter().take(10) {
        let copy = choose_copy(a, t, None).unwrap();
        read_stored_tag(a, copy).unwrap();
        let start = Instant::now();
        let status = tag_in_other_build(a, t, copy, b).unwrap();
        let took = start.elapsed();
        let print = |x: &Build, scan: &h5tags_core::references::ScanIndex, t: &Tag| {
            choose_copy(x, t, None).and_then(|c| scan.outgoing(c)).map(|s| s.data_print)
        };
        let by_print = if print(a, sa, t) == print(b, sb, u) { Status::Same } else { Status::Different };
        assert_eq!(status, by_print, "{}", t.path);
        println!("{} {} ({} MB entry in b): {status:?}, {took:.1?}", t.class, t.path, size / 1_000_000);
    }
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
