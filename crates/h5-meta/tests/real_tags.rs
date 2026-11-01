//! Reading every tag of a deploy folder with its definition (PRD §9 checks 1
//! and 4). Ignored by default; reads `H5_DEPLOY` and `H5_DEFS`.
//!
//! ```text
//! H5_DEPLOY=<folder> cargo test --release -p h5-meta --test real_tags -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use h5_defs::Definitions;
use h5_defs::definition::Definition;
use h5_meta::{Body, Content, Divergence, Problem, read_tag};
use h5_module::index::find_modules;
use h5_module::names::split_tag_name;
use h5_module::tag::TagHeader;
use h5_module::{EntryKind, FourCc, Module, Section, map_file};

fn tags_dir() -> PathBuf {
    std::env::var_os("H5_DEFS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/H5Tags/tags"))
}

#[derive(Default)]
struct Found {
    /// Kind of problem → (count, first example).
    problems: BTreeMap<String, (usize, String)>,
    /// GUID mismatches: (field path without indices, tag GUID) → count.
    guid_mismatches: BTreeMap<(String, [u8; 16]), usize>,
    blocks: usize,
    structs: usize,
}

fn problem_kind(p: &Problem) -> String {
    match p {
        Problem::SizeMismatch { definition, tag } => {
            format!("size mismatch ({} bytes)", *tag as i64 - *definition as i64)
        }
        Problem::UnknownType { type_name, .. } => format!("unknown type {type_name}"),
        Problem::UnknownSize { .. } => "unknown size".into(),
        Problem::NoDataBlock => "no data block".into(),
        Problem::OutOfBounds { .. } => "out of bounds".into(),
    }
}

fn strip_indices(path: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for c in path.chars() {
        match c {
            '[' => skip = true,
            ']' => skip = false,
            _ if !skip => out.push(c),
            _ => {}
        }
    }
    out
}

/// Every srcguid of a definition, as tag bytes.
fn guids(fields: &[h5_defs::definition::Field], out: &mut std::collections::HashSet<[u8; 16]>) {
    for f in fields {
        out.extend(f.guid());
        guids(&f.children, out);
    }
}

fn walk(body: &Body<'_>, path: &str, tag: &str, out: &mut Found) {
    let note = |p: &Problem, at: &str, out: &mut Found| {
        let e = out.problems.entry(problem_kind(p)).or_insert((0, String::new()));
        e.0 += 1;
        if e.1.is_empty() {
            e.1 = format!("{tag} at {}", if at.is_empty() { "(main struct)" } else { at });
        }
    };
    if let Some(p) = &body.problem {
        note(p, path, out);
    }
    if let Some(g) = body.guid_mismatch {
        *out.guid_mismatches.entry((strip_indices(path), g)).or_default() += 1;
    }
    for n in &body.children {
        let step = n.name.unwrap_or("");
        let p = if path.is_empty() { step.to_owned() } else { format!("{path}.{step}") };
        match &n.content {
            Content::Struct(b) | Content::Array(b) => {
                out.structs += 1;
                walk(b, &p, tag, out);
            }
            Content::Block(b) => {
                out.blocks += 1;
                if let Some(g) = b.guid_mismatch {
                    *out.guid_mismatches.entry((strip_indices(&p), g)).or_default() += 1;
                }
                if let Some(problem) = &b.problem {
                    note(problem, &p, out);
                }
                for (i, e) in b.entries.iter().enumerate() {
                    walk(e, &format!("{p}[{i}]"), tag, out);
                }
            }
            _ => {}
        }
    }
}

#[test]
#[ignore]
fn every_tag_reads_with_its_definition() {
    let deploy = PathBuf::from(std::env::var("H5_DEPLOY").expect("set H5_DEPLOY"));
    let defs = Definitions::load(&tags_dir()).unwrap();
    let mut parsed: HashMap<FourCc, Option<Definition>> = HashMap::new();
    let start = Instant::now();
    let (mut tags, mut clean) = (0usize, 0usize);
    let mut found = Found::default();
    // (class, divergence kind) → (count, tags affected, first example).
    let mut divergences: BTreeMap<(String, String), (usize, usize, String)> = BTreeMap::new();
    let mut per_class_problems: BTreeMap<String, usize> = BTreeMap::new();
    // GUID mismatches per class: (class, path, guid) → count.
    let mut mismatches: BTreeMap<(String, String, [u8; 16]), usize> = BTreeMap::new();
    let mut class_guids: HashMap<String, std::collections::HashSet<[u8; 16]>> = HashMap::new();
    for module in find_modules(&deploy).unwrap() {
        let map = map_file(&module.path).unwrap();
        let m = Module::parse(&map).unwrap();
        for (i, e) in m.entries.iter().enumerate().filter(|(_, e)| e.kind() == EntryKind::Tag) {
            let i = i as u32;
            let name = m.name(i).unwrap();
            let class = split_tag_name(name).1.unwrap_or_default();
            let Some(def) = parsed
                .entry(e.class_code)
                .or_insert_with(|| {
                    let file = defs.find(e.class_code, Some(class))?;
                    Definition::parse(&std::fs::read_to_string(&file.path).ok()?).ok()
                })
                .as_ref()
            else {
                continue;
            };
            let (Ok(h), Ok(data)) = (m.read_section(i, Section::Header), m.read_section(i, Section::TagData)) else {
                continue;
            };
            let header = TagHeader::parse(&h).unwrap();
            let r = read_tag(def, &header, &data);
            tags += 1;
            let before = found.problems.values().map(|p| p.0).sum::<usize>();
            let class_key = format!("{} {class}", e.class_code);
            class_guids.entry(class_key.clone()).or_insert_with(|| {
                let mut g = std::collections::HashSet::new();
                guids(&def.fields, &mut g);
                g
            });
            let before_guids = std::mem::take(&mut found.guid_mismatches);
            walk(&r.root, "", name, &mut found);
            for ((path, g), n) in std::mem::replace(&mut found.guid_mismatches, before_guids) {
                *mismatches.entry((class_key.clone(), path, g)).or_default() += n;
            }
            let had_problem = found.problems.values().map(|p| p.0).sum::<usize>() > before;
            if had_problem {
                *per_class_problems.entry(format!("{} {class}", e.class_code)).or_default() += 1;
            }
            clean += usize::from(!had_problem && r.divergences.is_empty());
            let mut kinds_here: Vec<String> = Vec::new();
            for d in &r.divergences {
                let (kind, example) = match d {
                    Divergence::NotInList { what, path } => {
                        (format!("not in list: {what}"), format!("{name} at {path}"))
                    }
                    Divergence::Unreached { what, block, offset, .. } => {
                        (format!("unreached: {what}"), format!("{name} block {block} +0x{offset:X}"))
                    }
                };
                let slot = divergences.entry((format!("{} {class}", e.class_code), kind.clone())).or_insert((
                    0,
                    0,
                    String::new(),
                ));
                slot.0 += 1;
                if !kinds_here.contains(&kind) {
                    slot.1 += 1;
                    kinds_here.push(kind);
                }
                if slot.2.is_empty() {
                    slot.2 = example;
                }
            }
        }
    }
    println!(
        "{tags} tags read in {:.2?}; {clean} with no problem and no divergence; {} tag blocks, {} structs/arrays",
        start.elapsed(),
        found.blocks,
        found.structs
    );
    println!("problems (what wasn't read), by kind:");
    for (k, (n, ex)) in &found.problems {
        println!("  {k}: {n}, e.g. {ex}");
    }
    println!("tags with a problem, by class: {per_class_problems:?}");
    let all_guids: std::collections::HashSet<[u8; 16]> = class_guids.values().flatten().copied().collect();
    let total: usize = mismatches.values().sum();
    println!("GUID mismatches (values kept): {total} at {} (class, field, GUID) places", mismatches.len());
    let mut by_where: BTreeMap<&str, usize> = BTreeMap::new();
    for ((class, _, g), n) in &mismatches {
        let place = if class_guids[class].contains(g) {
            "another field of the same definition"
        } else if all_guids.contains(g) {
            "another class's definition"
        } else {
            "no definition"
        };
        *by_where.entry(place).or_default() += n;
    }
    println!("  the tag's GUID is the srcguid of: {by_where:?}");
    let mut top: Vec<_> = mismatches.iter().collect();
    top.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for ((class, path, _), n) in top.iter().take(15) {
        println!("  {n:6}  {class} | {path}");
    }
    println!("divergences (check 1), by class and kind: count in tags, e.g.:");
    for ((class, kind), (n, t, ex)) in &divergences {
        println!("  {class} | {kind}: {n} in {t} tags, e.g. {ex}");
    }
}
