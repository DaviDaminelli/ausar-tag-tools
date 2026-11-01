//! M3 step 0, the survey (decisions D31): one run over every tag of a
//! deploy folder, comparing the definitions with the real tags. Ignored by
//! default; reads `H5_DEPLOY` and `H5_DEFS` (see `real_definitions.rs`).
//!
//! ```text
//! H5_DEPLOY=<folder> cargo test --release -p h5-defs --test survey -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use h5_defs::Definitions;
use h5_defs::definition::{Definition, Field, FieldKind};
use h5_defs::field_type::FieldType;
use h5_module::index::find_modules;
use h5_module::names::split_tag_name;
use h5_module::tag::{StructKind, TagHeader};
use h5_module::{EntryKind, FourCc, Module, Section, map_file};

fn tags_dir() -> PathBuf {
    std::env::var_os("H5_DEFS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/H5Tags/tags"))
}

/// A GUID as written (`151d0146-479c-...`), in the two candidate byte orders.
fn guid_orders(text: &str) -> Option<[[u8; 16]; 2]> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let mut written = [0u8; 16];
    for (i, b) in written.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    // Windows GUID layout: the first three groups little-endian.
    let mut windows = written;
    windows[0..4].reverse();
    windows[4..6].reverse();
    windows[6..8].reverse();
    Some([written, windows])
}

/// What a `srcguid` stands for in a definition.
#[derive(Clone)]
struct Described {
    path: String,
    block: bool,
    /// Entry size (tag block) or struct size.
    size: Option<u32>,
}

/// Every `srcguid` of a definition, by GUID bytes, in both orders (0 = as
/// written, 1 = Windows layout).
fn guids(fields: &[Field], prefix: &str, out: &mut HashMap<[u8; 16], (usize, Described)>) {
    for f in fields {
        let path = if prefix.is_empty() { f.name.display.clone() } else { format!("{prefix}.{}", f.name.display) };
        if let Some(orders) = f.attr("srcguid").and_then(guid_orders) {
            let block = f.kind == FieldKind::Known(FieldType::Block64);
            let size = if block { f.entry_size() } else { f.size() };
            for (order, bytes) in orders.into_iter().enumerate() {
                out.entry(bytes).or_insert((order, Described { path: path.clone(), block, size }));
            }
        }
        guids(&f.children, &path, out);
    }
}

#[derive(Default)]
struct ClassStats {
    tags: usize,
    main_match: usize,
    main_mismatch: BTreeMap<i64, usize>,       // tag size − definition size → tags
    structs: BTreeMap<String, (usize, usize)>, // struct kind → (seen, GUID matched)
    blocks_checked: usize,
    blocks_size_match: usize,
    block_mismatch_example: Option<String>,
}

#[test]
#[ignore]
fn survey_definitions_against_tags() {
    let deploy = PathBuf::from(std::env::var("H5_DEPLOY").expect("set H5_DEPLOY"));
    let defs = Definitions::load(&tags_dir()).unwrap();
    let modules = find_modules(&deploy).unwrap();
    let start = Instant::now();
    // Per class: the parsed definition and its GUIDs (or None without one).
    type Parsed = Option<(Definition, HashMap<[u8; 16], (usize, Described)>)>;
    let mut parsed_defs: HashMap<FourCc, Parsed> = HashMap::new();
    let mut stats: BTreeMap<String, ClassStats> = BTreeMap::new();
    let mut no_definition: BTreeMap<String, usize> = BTreeMap::new();
    let mut guid_order: [usize; 2] = [0, 0];
    // GUID → (definition file, path) over every class and tag_resources definition.
    let mut all_guids: HashMap<[u8; 16], (String, String)> = HashMap::new();
    for dir in [tags_dir(), tags_dir().join("../tag_resources")] {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
        paths.sort();
        for p in paths {
            let Ok(def) = Definition::parse(&std::fs::read_to_string(&p).unwrap()) else { continue };
            let mut g = HashMap::new();
            guids(&def.fields, "", &mut g);
            let file = p.file_name().unwrap().to_string_lossy().into_owned();
            for (bytes, (order, d)) in g {
                if order == 1 {
                    all_guids.entry(bytes).or_insert((file.clone(), d.path));
                }
            }
        }
    }
    // Unmatched tag block GUIDs: (class, found in which file) → count, example.
    let mut unmatched_blocks: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    // Resource struct GUIDs: matching definition file → count.
    let mut resource_guids: BTreeMap<String, usize> = BTreeMap::new();
    // Main struct GUIDs per class: distinct GUIDs.
    let mut main_guids: BTreeMap<String, std::collections::HashSet<[u8; 16]>> = BTreeMap::new();
    for found in &modules {
        let map = map_file(&found.path).unwrap();
        let m = Module::parse(&map).unwrap();
        for (i, e) in m.entries.iter().enumerate().filter(|(_, e)| e.kind() == EntryKind::Tag) {
            let i = i as u32;
            let name = m.name(i).unwrap();
            let class_name = split_tag_name(name).1.unwrap_or_default();
            let parsed = parsed_defs.entry(e.class_code).or_insert_with(|| {
                let file = defs.find(e.class_code, Some(class_name))?;
                let def = Definition::parse(&std::fs::read_to_string(&file.path).ok()?).ok()?;
                let mut g = HashMap::new();
                guids(&def.fields, "", &mut g);
                Some((def, g))
            });
            let Some((def, def_guids)) = parsed else {
                *no_definition.entry(format!("{} {class_name}", e.class_code)).or_default() += 1;
                continue;
            };
            let (Ok(header), Ok(tag_data)) = (m.read_section(i, Section::Header), m.read_section(i, Section::TagData))
            else {
                continue;
            };
            let Ok(tag) = TagHeader::parse(&header) else { continue };
            let s = stats.entry(format!("{} {class_name}", e.class_code)).or_default();
            s.tags += 1;
            // 1. Main struct size.
            let main = tag.main_struct().and_then(|ms| ms.target).and_then(|t| tag.data_blocks.get(t as usize));
            if let (Some(block), Some(want)) = (main, def.size()) {
                if block.size == want {
                    s.main_match += 1;
                } else {
                    *s.main_mismatch.entry(block.size as i64 - want as i64).or_default() += 1;
                }
            }
            // 2. GUIDs, and 3. tag block entry sizes.
            for ts in &tag.tag_structs {
                let kind = format!("{:?}", ts.kind);
                let entry = s.structs.entry(kind).or_default();
                entry.0 += 1;
                let class_key = format!("{} {class_name}", e.class_code);
                match ts.kind {
                    StructKind::Main => {
                        main_guids.entry(class_key.clone()).or_default().insert(ts.guid);
                    }
                    StructKind::Resource => {
                        let file = all_guids.get(&ts.guid).map_or("(none)".to_owned(), |(f, _)| f.clone());
                        *resource_guids.entry(file).or_default() += 1;
                    }
                    StructKind::TagBlock if !def_guids.contains_key(&ts.guid) => {
                        let place =
                            all_guids.get(&ts.guid).map_or("(no definition)".to_owned(), |(f, p)| format!("{f} {p}"));
                        let field =
                            ts.field_block.map_or("-".into(), |b| format!("block {b} +0x{:X}", ts.field_offset));
                        let slot = unmatched_blocks.entry((class_key.clone(), place)).or_insert((0, String::new()));
                        slot.0 += 1;
                        if slot.1.is_empty() {
                            slot.1 = format!("{name} ({field})");
                        }
                    }
                    _ => {}
                }
                let Some((order, d)) = def_guids.get(&ts.guid) else { continue };
                entry.1 += 1;
                guid_order[*order] += 1;
                if ts.kind != StructKind::TagBlock || !d.block {
                    continue;
                }
                let (Some(target), Some(field_block)) = (ts.target, ts.field_block) else { continue };
                let Some(field) = tag
                    .block_data(&tag_data, field_block)
                    .and_then(|b| b.get(ts.field_offset as usize..ts.field_offset as usize + 0x1C))
                else {
                    continue;
                };
                let count = i32::from_le_bytes(field[0x10..0x14].try_into().unwrap());
                let (Some(block), Some(entry_size)) = (tag.data_blocks.get(target as usize), d.size) else { continue };
                s.blocks_checked += 1;
                if count >= 0 && block.size as u64 == count as u64 * entry_size as u64 {
                    s.blocks_size_match += 1;
                } else if s.block_mismatch_example.is_none() {
                    s.block_mismatch_example = Some(format!(
                        "{name}: {} has {count} entries × {entry_size} B, block is {} B",
                        d.path, block.size
                    ));
                }
            }
        }
    }
    let elapsed = start.elapsed();

    let total = |f: fn(&ClassStats) -> usize| stats.values().map(f).sum::<usize>();
    println!(
        "{} tags of {} classes with a definition, in {elapsed:.2?}; {} tags of {} classes without one",
        total(|s| s.tags),
        stats.len(),
        no_definition.values().sum::<usize>(),
        no_definition.len()
    );
    println!(
        "main struct size = definition size: {} of {} tags",
        total(|s| s.main_match),
        total(|s| s.main_match + s.main_mismatch.values().sum::<usize>())
    );
    println!("GUID matches by byte order: as written {}, Windows layout {}", guid_order[0], guid_order[1]);
    let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for s in stats.values() {
        for (k, (seen, matched)) in &s.structs {
            let t = by_kind.entry(k.clone()).or_default();
            t.0 += seen;
            t.1 += matched;
        }
    }
    println!("tag structs whose GUID is a srcguid of the definition, by kind: {by_kind:?}");
    println!(
        "tag blocks with a matched GUID: size = count × entry size in {} of {}",
        total(|s| s.blocks_size_match),
        total(|s| s.blocks_checked)
    );
    println!("classes with main struct size mismatches (tag − definition bytes → tags):");
    for (class, s) in stats.iter().filter(|(_, s)| !s.main_mismatch.is_empty()) {
        println!("  {class}: {} of {} tags match; {:?}", s.main_match, s.tags, s.main_mismatch);
    }
    println!("classes with tag block size mismatches:");
    for (class, s) in stats.iter().filter(|(_, s)| s.blocks_size_match < s.blocks_checked) {
        println!(
            "  {class}: {} of {} blocks; e.g. {}",
            s.blocks_size_match,
            s.blocks_checked,
            s.block_mismatch_example.as_deref().unwrap_or("")
        );
    }
    println!("classes without a definition: {no_definition:?}");
    println!("unmatched tag block GUIDs, by (class, where the GUID is defined) → count, e.g.:");
    for ((class, place), (n, ex)) in &unmatched_blocks {
        println!("  {class} | {place}: {n}, e.g. {ex}");
    }
    println!("resource struct GUIDs, by matching definition file: {resource_guids:?}");
    let multi: Vec<String> =
        main_guids.iter().filter(|(_, g)| g.len() > 1).map(|(c, g)| format!("{c}: {}", g.len())).collect();
    let in_defs = main_guids.values().flatten().filter(|g| all_guids.contains_key(*g)).count();
    println!(
        "main struct GUIDs: {} classes, {} with more than one GUID {multi:?}; {in_defs} distinct main GUIDs appear in some definition",
        main_guids.len(),
        multi.len()
    );
}
