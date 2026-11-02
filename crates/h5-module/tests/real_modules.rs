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
        let mut shown: Vec<u32> = failures.iter().filter_map(|f| f.item.map(|item| item.index)).collect();
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

/// For modules shorter than their file list says: where the cut is, which entry
/// holds the last byte, and whether the data before the cut looks like module
/// data (each compressed block starts with a zlib header).
#[test]
#[ignore]
fn report_incomplete_modules() {
    for found in find_modules(&deploy()).unwrap() {
        let map = map_file(&found.path).unwrap();
        let m = Module::parse(&map).unwrap();
        let len = m.file_len();
        if m.data_end() <= len {
            continue;
        }
        println!(
            "== {}: {} bytes, file list needs {} ({} missing)",
            found.rel_path,
            len,
            m.data_end(),
            m.data_end() - len
        );
        let (mut blocks_ok, mut blocks_bad, mut uncompressed) = (0, 0, 0);
        for (i, e) in m.entries.iter().enumerate() {
            let Ok(blocks) = m.entry_blocks(i as u32) else { continue };
            let start = m.data_start + e.data_offset;
            for b in blocks.iter() {
                let at = (start + b.compressed_offset as u64) as usize;
                let Some(head) = map.get(at..at + 2) else { continue };
                if !b.compressed {
                    uncompressed += 1;
                } else if head == [0x48, 0x0D] || head[0] == 0x78 {
                    blocks_ok += 1;
                } else {
                    blocks_bad += 1;
                    if blocks_bad <= 3 {
                        println!("  block of entry {i} at {at} starts {head:02X?}, not zlib");
                    }
                }
            }
            if start < len && m.entry_data_end(e) > len {
                println!(
                    "  the cut is inside entry {i} '{}' (data {start}..{}), blocks: {:?}",
                    m.name(i as u32).unwrap_or("?"),
                    m.entry_data_end(e),
                    blocks.iter().map(|b| (b.compressed_offset, b.compressed_size, b.compressed)).collect::<Vec<_>>()
                );
            }
        }
        println!(
            "  blocks inside the file: {blocks_ok} start with a zlib header, {blocks_bad} don't, {uncompressed} stored uncompressed"
        );
    }
}

/// Every v27 compression block's stored checksum equals MurmurHash3 x64 128
/// of its decompressed data (AusarDocs: "the upper 64 bits are discarded").
/// Also proves each block decompressed exactly right.
#[test]
#[ignore]
fn block_checksums_match() {
    use h5_module::murmur3::murmur3_x64_128;
    let (mut first_half, mut second_half, mut neither, mut none_stored) = (0usize, 0usize, 0usize, 0usize);
    let mut examples = Vec::new();
    for found in find_modules(&deploy()).unwrap() {
        let map = map_file(&found.path).unwrap();
        let m = Module::parse(&map).unwrap();
        for i in 0..m.entries.len() as u32 {
            let (Ok(data), Ok(blocks)) = (m.read_entry(i), m.entry_blocks(i)) else { continue };
            for b in blocks.iter() {
                let Some(stored) = b.checksum else {
                    none_stored += 1;
                    continue;
                };
                let start = b.uncompressed_offset as usize;
                let (h1, h2) = murmur3_x64_128(&data[start..start + b.uncompressed_size as usize], 0);
                match (h1 == stored, h2 == stored) {
                    (true, _) => first_half += 1,
                    (_, true) => second_half += 1,
                    _ => {
                        neither += 1;
                        if examples.len() < 5 {
                            examples.push(format!(
                                "{} entry {i}: stored {stored:#018x}, hash {h1:#018x} {h2:#018x}",
                                found.rel_path
                            ));
                        }
                    }
                }
            }
        }
    }
    println!(
        "block checksums: first half (h1) matches {first_half}, second half (h2) matches {second_half}, neither {neither}, no checksum stored (v23 or synthetic) {none_stored}"
    );
    for e in &examples {
        println!("  {e}");
    }
    assert_eq!(neither, 0, "see the mismatches above");
}

/// For every entry with sections: the three sections read one by one equal the
/// whole entry. Also times reading what a tag read needs (header + tag data)
/// against reading everything (PRD D5).
#[test]
#[ignore]
fn sections_add_up_to_whole_entries() {
    use h5_module::Section;
    let (mut checked, mut sections_time, mut whole_time) =
        (0usize, std::time::Duration::ZERO, std::time::Duration::ZERO);
    let (mut needed_bytes, mut whole_bytes) = (0u64, 0u64);
    for found in find_modules(&deploy()).unwrap() {
        let map = map_file(&found.path).unwrap();
        let m = Module::parse(&map).unwrap();
        for (i, e) in m.entries.iter().enumerate() {
            let i = i as u32;
            if e.flags.raw_file() || e.is_stored_elsewhere() {
                continue;
            }
            let Ok(whole) = m.read_entry(i) else { continue };
            let start = Instant::now();
            let header = m.read_section(i, Section::Header).unwrap();
            let tag_data = m.read_section(i, Section::TagData).unwrap();
            sections_time += start.elapsed();
            let resource = m.read_section(i, Section::ResourceData).unwrap();
            assert_eq!([header.as_slice(), &tag_data, &resource].concat(), whole, "{} entry {i}", found.rel_path);
            let start = Instant::now();
            let _ = m.read_entry(i).unwrap();
            whole_time += start.elapsed();
            needed_bytes += (header.len() + tag_data.len()) as u64;
            whole_bytes += whole.len() as u64;
            checked += 1;
        }
    }
    println!(
        "{checked} entries: sections == whole entry. Header + tag data only: {:.1} MB in {sections_time:.2?}; whole entries: {:.1} MB in {whole_time:.2?}",
        needed_bytes as f64 / 1e6,
        whole_bytes as f64 / 1e6
    );
}

/// M2: every tag's header parses and passes every check; its reference fields
/// agree with the lists; dependencies resolve to indexed tags by name and ID.
#[test]
#[ignore]
fn every_tag_header_parses_and_checks() {
    use std::collections::{HashMap, HashSet};

    use h5_module::Section;
    use h5_module::names::split_tag_name;
    use h5_module::tag::TagHeader;

    let modules = find_modules(&deploy()).unwrap();
    let maps: Vec<_> = modules.iter().map(|f| map_file(&f.path).unwrap()).collect();
    let parsed: Vec<Module> = maps.iter().map(|m| Module::parse(m).unwrap()).collect();
    // Every indexed tag: global ID → file name.
    let mut names: HashMap<i32, String> = HashMap::new();
    for m in &parsed {
        for (i, e) in m.entries.iter().enumerate().filter(|(_, e)| e.kind() == EntryKind::Tag) {
            names.entry(e.global_id.0).or_insert_with(|| m.name(i as u32).unwrap().to_owned());
        }
    }

    let start = Instant::now();
    let (mut tags, mut header_bytes) = (0usize, 0u64);
    let mut failures: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut parse_errors: Vec<String> = Vec::new();
    let mut trailing: BTreeMap<String, usize> = BTreeMap::new();
    let mut not_stored = 0;
    let (mut deps, mut deps_indexed, mut deps_same_name, mut deps_other_name) = (0usize, 0usize, 0usize, Vec::new());
    let mut deps_listed_with_class = 0usize;
    let mut deps_unnamed = 0usize; // no listed name: only the index can name them
    let mut unresolved_ids: HashSet<i32> = HashSet::new();
    // Tag references: all, without a dependency, of those naming a tag (ID set), of those with a listed name.
    let (mut refs, mut refs_no_dep, mut refs_no_dep_named, mut refs_no_dep_listed) = (0usize, 0usize, 0usize, 0usize);
    let mut non_ascii_tables = 0usize;
    // Zonesets: names resolve through the tag's own string_ids; each resource is
    // loaded by some zoneset (checked by `TagHeader::check`, `zoneset`).
    let (mut zonesets, mut zs_named, mut zs_tags, mut zs_tag_named) = (0usize, 0usize, 0usize, 0usize);
    let (mut zs_resources, mut zs_tags_with_resources) = (0usize, 0usize);
    // Tags owning a resource that no zoneset lists: (class, module) → count.
    let mut zs_unlisted: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut zs_per_tag: BTreeMap<usize, usize> = BTreeMap::new();
    // Q9: is the resource at position k of the entry's list named `[k:kind]`?
    let (mut resources, mut resources_named_by_position) = (0usize, 0usize);
    for (found, m) in modules.iter().zip(&parsed) {
        for (i, e) in m.entries.iter().enumerate().filter(|(_, e)| e.kind() == EntryKind::Tag) {
            let i = i as u32;
            let class = e.class_code.to_string();
            let header = match m.read_section(i, Section::Header) {
                Ok(h) => h,
                Err(ModuleError::NotStored { .. }) => {
                    not_stored += 1;
                    continue;
                }
                Err(err) => panic!("{} entry {i}: {err}", found.rel_path),
            };
            header_bytes += header.len() as u64;
            let tag = match TagHeader::parse(&header) {
                Ok(t) => t,
                Err(err) => {
                    parse_errors.push(format!("{class} {}: {err}", m.name(i).unwrap_or("?")));
                    continue;
                }
            };
            tags += 1;
            if !tag.trailing.is_empty() {
                *trailing.entry(class.clone()).or_default() += 1;
            }
            let tag_data = m.read_section(i, Section::TagData).unwrap();
            for f in tag.check(e).into_iter().chain(tag.check_tag_data(&tag_data)) {
                failures
                    .entry(format!("{} {class}", f.check))
                    .or_insert((0, format!("{}: {f}", m.name(i).unwrap_or("?"))))
                    .0 += 1;
            }
            for r in &tag.tag_references {
                refs += 1;
                if r.dependency.is_some() {
                    continue;
                }
                refs_no_dep += 1;
                if tag.reference_field(&tag_data, r).is_some_and(|f| !f.is_null()) {
                    refs_no_dep_named += 1;
                    refs_no_dep_listed += usize::from(tag.string_bytes(r.name_offset).is_some_and(|s| !s.is_empty()));
                }
            }
            non_ascii_tables += usize::from(!tag.string_table.is_ascii());
            let sid_names = tag.string_id_names();
            *zs_per_tag.entry(tag.zonesets.len()).or_default() += 1;
            zs_tags_with_resources += usize::from(e.resource_count > 0);
            let listed: HashSet<u32> = tag.zonesets.iter().flat_map(|z| z.resources.iter().copied()).collect();
            if (0..e.resource_count).any(|r| !listed.contains(&r)) {
                *zs_unlisted.entry((class.clone(), found.rel_path.clone())).or_default() += 1;
            }
            for (k, &file) in m.entry_resources(i).unwrap().iter().enumerate() {
                resources += 1;
                let name = m.name(file).unwrap_or("");
                resources_named_by_position +=
                    usize::from(h5_module::names::parse_resource_name(name).is_some_and(|r| r.index as usize == k));
            }
            for z in &tag.zonesets {
                zonesets += 1;
                zs_named += usize::from(sid_names.get(z.name).is_some());
                zs_resources += z.resources.len();
                zs_tags += z.tags.len();
                zs_tag_named += z.tags.iter().filter(|t| sid_names.get(t.zoneset_name).is_some()).count();
            }
            for d in &tag.dependencies {
                deps += 1;
                deps_unnamed += usize::from(tag.string_bytes(d.name_offset).is_none_or(<[u8]>::is_empty));
                let Some(indexed) = names.get(&d.global_id.0) else {
                    unresolved_ids.insert(d.global_id.0);
                    continue;
                };
                deps_indexed += 1;
                let listed = tag.string(d.name_offset).unwrap_or_default();
                if listed.eq_ignore_ascii_case(split_tag_name(indexed).0) {
                    deps_same_name += 1;
                } else if listed.eq_ignore_ascii_case(indexed) {
                    deps_same_name += 1;
                    deps_listed_with_class += 1;
                } else if deps_other_name.len() < 5 {
                    deps_other_name.push(format!("listed '{listed}', indexed '{indexed}'"));
                }
            }
        }
    }
    let elapsed = start.elapsed();
    println!(
        "{tags} tag headers parsed ({:.1} MB) in {elapsed:.2?}; {} parse errors; {not_stored} tags not stored here",
        header_bytes as f64 / 1e6,
        parse_errors.len()
    );
    for e in parse_errors.iter().take(5) {
        println!("  parse error: {e}");
    }
    println!("check failures by check and class: {}", if failures.is_empty() { "none" } else { "" });
    for (k, (n, first)) in &failures {
        println!("  {k}: {n}× e.g. {first}");
    }
    println!("tags with trailing data, by class: {trailing:?}");
    println!(
        "dependencies: {deps}; {deps_indexed} point at indexed tags ({deps_same_name} with the same name, {deps_listed_with_class} of those listed with the class suffix); {} distinct global IDs not in the indexed modules",
        unresolved_ids.len()
    );
    println!(
        "tag references: {refs}; {refs_no_dep} without a dependency, {refs_no_dep_named} of those hold a global ID, {refs_no_dep_listed} of those with a listed name"
    );
    println!("tags whose string table has non-ASCII bytes: {non_ascii_tables}");
    println!("zonesets per tag: {zs_per_tag:?}");
    println!(
        "tag resources: {resources}; {resources_named_by_position} named [k:kind] at position k of the entry's list"
    );
    println!(
        "zonesets: {zonesets}, {zs_named} named by the tag's string_ids; zoneset tags: {zs_tags}, {zs_tag_named} named; {zs_resources} resource indices, from {zs_tags_with_resources} tags with resources"
    );
    println!("tags with a resource no zoneset lists, by (class, module): {zs_unlisted:?}");
    println!("dependencies without a listed name: {deps_unnamed}");
    for d in &deps_other_name {
        println!("  name differs: {d}");
    }
    assert!(parse_errors.is_empty() && failures.is_empty(), "see above");
}

/// M2, PRD §12 Q10: for entries with copies in several modules, does an equal
/// `assetChecksum` (or an equal block checksum list) mean equal stored data?
/// Copies are entries of the same kind and name; each is compared with the
/// first copy found, by decompressed bytes.
#[test]
#[ignore]
fn copy_identity() {
    use std::collections::HashMap;

    use h5_module::Section;

    let modules = find_modules(&deploy()).unwrap();
    let maps: Vec<_> = modules.iter().map(|f| map_file(&f.path).unwrap()).collect();
    let parsed: Vec<Module> = maps.iter().map(|m| Module::parse(m).unwrap()).collect();
    // (kind, name) → every (module, entry) holding it.
    let mut copies: HashMap<(EntryKind, String), Vec<(usize, u32)>> = HashMap::new();
    for (mi, m) in parsed.iter().enumerate() {
        for (i, e) in m.entries.iter().enumerate() {
            let name = m.name(i as u32).unwrap().to_ascii_lowercase();
            copies.entry((e.kind(), name)).or_default().push((mi, i as u32));
        }
    }
    let entry = |(mi, i): (usize, u32)| &parsed[mi].entries[i as usize];
    let read = |(mi, i): (usize, u32)| parsed[mi].read_entry(i);
    let blocks = |(mi, i): (usize, u32)| -> Vec<(Option<u64>, u32)> {
        parsed[mi].entry_blocks(i).unwrap().iter().map(|b| (b.checksum, b.uncompressed_size)).collect()
    };
    let tag_part =
        |(mi, i): (usize, u32)| [Section::Header, Section::TagData].map(|s| parsed[mi].read_section(i, s).unwrap());

    let start = Instant::now();
    let (mut pairs, mut not_stored) = (0usize, 0usize);
    let mut groups: BTreeMap<(&str, usize), usize> = BTreeMap::new(); // (kind, copies) → entries
    // (kind, assetChecksum equal, data equal) → copies; the same with block checksum lists.
    let mut by_checksum: BTreeMap<(&str, bool, bool), usize> = BTreeMap::new();
    let mut by_blocks: BTreeMap<(&str, bool, bool), usize> = BTreeMap::new();
    let mut gid_differs = 0usize;
    let mut only_resource_data_differs = 0usize;
    let mut examples: BTreeMap<(&str, bool, bool), Vec<String>> = BTreeMap::new();
    for ((kind, name), list) in copies.iter().filter(|(_, l)| l.len() > 1) {
        let k = kind.as_str();
        *groups.entry((k, list.len())).or_default() += 1;
        let first = list[0];
        let Ok(first_data) = read(first) else {
            not_stored += list.len() - 1;
            continue;
        };
        for &other in &list[1..] {
            let Ok(data) = read(other) else {
                not_stored += 1;
                continue;
            };
            pairs += 1;
            let same_data = data == first_data;
            let same_checksum = entry(other).asset_checksum == entry(first).asset_checksum;
            gid_differs += usize::from(entry(other).global_id != entry(first).global_id);
            *by_checksum.entry((k, same_checksum, same_data)).or_default() += 1;
            *by_blocks.entry((k, blocks(other) == blocks(first), same_data)).or_default() += 1;
            if !same_data && *kind == EntryKind::Tag && tag_part(other) == tag_part(first) {
                only_resource_data_differs += 1;
            }
            let ex = examples.entry((k, same_checksum, same_data)).or_default();
            if ex.len() < 2 {
                ex.push(format!("{name}: {} vs {}", modules[first.0].rel_path, modules[other.0].rel_path));
            }
        }
    }
    println!(
        "entries with copies, by (kind, copies): {groups:?}; {pairs} copies compared with the first, {not_stored} not stored here; {:.2?}",
        start.elapsed()
    );
    println!("copies with another global ID than the first: {gid_differs}");
    println!("(kind, assetChecksum equal, data equal) → copies: {by_checksum:?}");
    println!("(kind, block checksum list equal, data equal) → copies: {by_blocks:?}");
    println!("tags whose copies differ only in resource data: {only_resource_data_differs}");
    for (k, ex) in &examples {
        for x in ex {
            println!("  e.g. {k:?}: {x}");
        }
    }
}

/// PRD §6.2 header scan: cold build, cache size, warm start (everything reused).
#[test]
#[ignore]
fn header_scan_timings() {
    use h5_module::scan::HeaderScan;
    let deploy = deploy();
    let cache = std::env::temp_dir().join(format!("h5tags-scan-timing-{}.scan", std::process::id()));
    let never = |_: &str| false;
    let (index, _) = Index::update(&deploy, None, &never).unwrap();

    let (scan, report) = HeaderScan::update(&deploy, &index, None, &never, &|_| {});
    let tags: usize = scan.modules.iter().map(|m| m.tags.len()).sum();
    let refs: usize = scan.modules.iter().flat_map(|m| &m.tags).map(|t| t.references.len()).sum();
    let undeclared: usize =
        scan.modules.iter().flat_map(|m| &m.tags).flat_map(|t| &t.references).filter(|r| !r.has_dependency).count();
    let strings: usize = scan.modules.iter().flat_map(|m| &m.tags).map(|t| t.strings.len()).sum();
    let problems: usize = scan.modules.iter().map(|m| m.problems.len()).sum();
    println!(
        "cold scan: {} modules, {tags} tag copies, {refs} non-null references ({undeclared} without a dependency), {:.1} MB of string tables, {problems} tags not readable, {} modules failed, {:.2?}",
        scan.modules.len(),
        strings as f64 / 1e6,
        report.failed,
        report.elapsed
    );
    // Dependencies no reference field points to: `find_references(out)` lists fields only.
    let unreferenced: usize = scan
        .modules
        .iter()
        .flat_map(|m| &m.tags)
        .map(|t| t.dependencies.iter().filter(|d| !t.references.iter().any(|r| r.target == **d)).count())
        .sum();
    println!("dependencies with no reference field pointing to them: {unreferenced}");
    for m in scan.modules.iter().filter(|m| !m.problems.is_empty()) {
        println!("  {}: {} not readable, e.g. {}", m.rel_path, m.problems.len(), m.problems[0].1);
    }

    let start = Instant::now();
    scan.save(&cache).unwrap();
    let size = std::fs::metadata(&cache).unwrap().len();
    println!("save: {:.2?}, cache file {:.1} MB", start.elapsed(), size as f64 / 1e6);

    let start = Instant::now();
    let loaded = HeaderScan::load(&cache).unwrap();
    let (warm, report) = HeaderScan::update(&deploy, &index, Some(loaded), &never, &|_| {});
    println!("warm start (load cache + check stamps): {:.2?} ({report:?})", start.elapsed());
    assert_eq!(warm, scan);
    assert_eq!(report.read, 0);
    std::fs::remove_file(&cache).unwrap();
}
