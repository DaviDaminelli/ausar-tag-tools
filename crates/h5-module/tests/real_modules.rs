//! Integration tests against real game files (PRD §9, M1). Ignored by default:
//!
//! ```text
//! H5_DEPLOY=<folder with .module files> cargo test -p h5-module -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use h5_module::index::{Index, find_modules};
use h5_module::names::{parse_resource_name, split_tag_name};
use h5_module::{EntryKind, Module, ModuleError, checks, map_file};

fn deploy() -> PathBuf {
    PathBuf::from(std::env::var("H5_DEPLOY").expect("set H5_DEPLOY to a folder with .module files"))
}

/// Every module parses, passes every cross-check, and every file entry of every
/// kind decompresses to its stated size.
#[test]
#[ignore]
fn every_module_checks_and_decompresses() {
    let modules = find_modules(&deploy()).unwrap();
    assert!(!modules.is_empty(), "no .module files found");
    let mut total_failures = 0;

    for found in &modules {
        let map = map_file(&found.path).unwrap();
        let m = Module::parse(&map).unwrap_or_else(|e| panic!("{}: {e}", found.rel_path));
        let failures = m.check();
        let start = Instant::now();

        // Decompress every entry, in parallel.
        let next = AtomicUsize::new(0);
        let errors = Mutex::new(Vec::new());
        let bytes = AtomicUsize::new(0);
        let not_stored = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..std::thread::available_parallelism().map_or(1, |n| n.get()) {
                s.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= m.entries.len() {
                            break;
                        }
                        match m.read_entry(i as u32) {
                            Ok(data) => {
                                assert_eq!(data.len(), m.entries[i].total_uncompressed_size as usize);
                                bytes.fetch_add(data.len(), Ordering::Relaxed);
                            }
                            Err(ModuleError::NotStored { .. }) => {
                                not_stored.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => errors.lock().unwrap().push(e.to_string()),
                        }
                    }
                });
            }
        });
        let errors = errors.into_inner().unwrap();

        let tags = m.entries.iter().filter(|e| e.kind() == EntryKind::Tag).count();
        let resources = m.entries.iter().filter(|e| e.kind() == EntryKind::Resource).count();
        println!(
            "{}: v{} files={} tags={} resources={} other={} loaded_tags={} | checks failed={} | decompressed {} entries, {:.1} MB in {:.2?}, not stored here={}, errors={}",
            found.rel_path,
            m.header.version.raw(),
            m.entries.len(),
            tags,
            resources,
            m.entries.len() - tags - resources,
            m.header.loaded_tag_count,
            failures.len(),
            m.entries.len(),
            bytes.load(Ordering::Relaxed) as f64 / 1e6,
            start.elapsed(),
            not_stored.load(Ordering::Relaxed),
            errors.len(),
        );
        let mut by_check: BTreeMap<&str, (usize, String)> = BTreeMap::new();
        for f in &failures {
            by_check.entry(f.check).or_insert((0, f.to_string())).0 += 1;
        }
        for (check, (n, first)) in &by_check {
            println!("  check {check} failed {n}×, first: {first}");
        }
        let mut shown: Vec<u32> = failures.iter().filter_map(|f| f.entry).collect();
        shown.dedup();
        for &i in shown.iter().take(5) {
            let e = &m.entries[i as usize];
            println!(
                "    entry {i} '{}': flags={:#04x} gid={} class={} parent={} sizes total={}/{} (c/u) h/t/r={}/{}/{} section blocks={}/{}/{} blocks={:?}",
                m.name(i).unwrap_or("?"),
                e.flags.0,
                e.global_id,
                e.class_code,
                e.parent_file_index,
                e.total_compressed_size,
                e.total_uncompressed_size,
                e.header_size,
                e.tag_data_size,
                e.resource_data_size,
                e.header_block_count,
                e.tag_data_block_count,
                e.resource_block_count,
                m.entry_blocks(i).map(|b| b
                    .iter()
                    .map(|b| (b.compressed_offset, b.compressed_size, b.uncompressed_offset, b.uncompressed_size))
                    .collect::<Vec<_>>()),
            );
            let at = m.data_start + e.data_offset;
            println!(
                "      data_offset={} → file offset {at} of {}; bytes there: {:02X?}",
                e.data_offset,
                m.file_len(),
                map.get(at as usize..at as usize + 4)
            );
            let same: Vec<_> = m
                .entries
                .iter()
                .enumerate()
                .filter(|(j, o)| *j != i as usize && o.data_offset == e.data_offset && o.total_compressed_size > 0)
                .map(|(j, o)| format!("{j} ({} bytes)", o.total_compressed_size))
                .collect();
            println!("      other entries with data at the same offset: {same:?}");
        }
        if let Some(e) = errors.first() {
            println!("  decompress errors {}×, e.g.: {e}", errors.len());
        }
        if failures.iter().any(|f| f.check == checks::DATA_BOUNDS) {
            let needed =
                m.entries.iter().map(|e| m.data_start + e.data_offset + e.total_compressed_size as u64).max().unwrap();
            println!(
                "  entry data needs {needed} bytes, file has {} (missing {:.1} MB)",
                m.file_len(),
                (needed - m.file_len()) as f64 / 1e6
            );
        }
        total_failures += failures.len() + errors.len();
    }
    assert_eq!(total_failures, 0, "see the failures printed above");
}

/// Prints format facts to verify and record in `docs/format-notes.md`.
#[test]
#[ignore]
fn report_format_facts() {
    for found in find_modules(&deploy()).unwrap() {
        let map = map_file(&found.path).unwrap();
        let m = Module::parse(&map).unwrap();
        let mut flags: BTreeMap<(u8, &str), usize> = BTreeMap::new();
        let mut suffixes: BTreeMap<(String, String), usize> = BTreeMap::new();
        let mut no_suffix = Vec::new();
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        let mut unparsed_resources = Vec::new();
        let mut resource_tags = 0;
        let mut zlib_headers: BTreeMap<[u8; 2], usize> = BTreeMap::new();

        for (i, e) in m.entries.iter().enumerate() {
            let name = m.name(i as u32).unwrap();
            let what = if e.kind() == EntryKind::Tag {
                "tag"
            } else if e.kind() == EntryKind::Resource {
                "resource"
            } else {
                "file"
            };
            *flags.entry((e.flags.0, what)).or_default() += 1;
            if e.kind() == EntryKind::Tag {
                match split_tag_name(name) {
                    (_, Some(suffix)) => {
                        *suffixes.entry((e.class_code.to_string(), suffix.to_owned())).or_default() += 1
                    }
                    (path, None) => no_suffix.push(path.to_owned()),
                }
            }
            if e.parent().is_some() {
                if e.kind() == EntryKind::Tag {
                    resource_tags += 1;
                }
                match parse_resource_name(name) {
                    Some(r) => *kinds.entry(r.raw_kind.to_owned()).or_default() += 1,
                    None => unparsed_resources.push(name.to_owned()),
                }
            }
            if let Ok(blocks) = m.entry_blocks(i as u32)
                && let Some(b) = blocks.iter().find(|b| b.compressed)
            {
                let at = (m.data_start + e.data_offset + b.compressed_offset as u64) as usize;
                if let Some(h) = map.get(at..at + 2) {
                    *zlib_headers.entry([h[0], h[1]]).or_default() += 1;
                }
            }
        }

        println!("== {} (v{})", found.rel_path, m.header.version.raw());
        println!("  header: id=0x{:016X} build_version_id=0x{:016X}", m.header.id, m.header.build_version_id);
        println!("  flags (value, kind) → count: {flags:?}");
        println!("  zlib stream headers → count: {zlib_headers:02X?}");
        println!("  tags without a .class suffix: {} {:?}", no_suffix.len(), &no_suffix[..no_suffix.len().min(5)]);
        println!("  (class code, file name suffix) pairs: {}", suffixes.len());
        for ((code, suffix), n) in &suffixes {
            println!("    {code:<4} {suffix} ({n})");
        }
        println!("  resource kinds: {kinds:?}");
        println!("  entries with a parent that are tags: {resource_tags}");
        println!(
            "  resource names not matching [index:kind]: {} {:?}",
            unparsed_resources.len(),
            &unparsed_resources[..unparsed_resources.len().min(5)]
        );
    }
}

/// Index build and cache load times (PRD §6.2 targets: cold < 10 s, warm < 1 s on a full install).
#[test]
#[ignore]
fn index_timings() {
    let deploy = deploy();
    let cache = std::env::temp_dir().join(format!("h5tags-index-timing-{}.index", std::process::id()));
    let never = |_: &str| false;

    let (index, report) = Index::update(&deploy, None, &never).unwrap();
    let entries: usize = index.modules.iter().map(|m| m.entries.len()).sum();
    println!("cold build: {} modules, {entries} entries, {:.2?} ({report:?})", index.modules.len(), report.elapsed);

    let start = Instant::now();
    index.save(&cache).unwrap();
    let size = std::fs::metadata(&cache).unwrap().len();
    println!("save: {:.2?}, cache file {:.1} MB", start.elapsed(), size as f64 / 1e6);

    let start = Instant::now();
    let loaded = Index::load(&cache).unwrap();
    let (warm, report) = Index::update(&deploy, Some(loaded), &never).unwrap();
    println!("warm start (load cache + check stamps): {:.2?} ({report:?})", start.elapsed());
    assert_eq!(warm, index);
    assert_eq!(report.read, 0);
    std::fs::remove_file(&cache).unwrap();
}
