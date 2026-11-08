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
        Problem::SizeMismatch { definition, tag, read_up_to } => {
            let read = read_up_to.as_ref().map_or(String::new(), |r| format!(", read up to {}", r.last_anchor));
            format!("size mismatch ({} bytes{read})", *tag as i64 - *definition as i64)
        }
        Problem::Unconfirmed { .. } => "past the last confirmed anchor".into(),
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

#[derive(Default)]
struct Values {
    string_ids: usize,
    sid_resolved: usize,
    sid_zero: usize,
    sid_unresolved: BTreeMap<String, (usize, String)>,
    enums: usize,
    enum_missing: BTreeMap<String, (usize, String)>,
    enum_negative: usize,
    block_indices: usize,
    index_none: usize,
    index_no_target: BTreeMap<String, usize>,
    index_out_of_range: BTreeMap<String, (usize, String)>,
    over_max: BTreeMap<String, (usize, String)>,
    reals: usize,
    not_finite: BTreeMap<String, (usize, String)>,
    resources: usize,
    /// (class, field path) → resource kind → count.
    resource_kinds: BTreeMap<(String, String), BTreeMap<String, usize>>,
    resource_index_past_end: usize,
}

fn note(map: &mut BTreeMap<String, (usize, String)>, key: String, example: impl FnOnce() -> String) {
    let e = map.entry(key).or_insert((0, String::new()));
    e.0 += 1;
    if e.1.is_empty() {
        e.1 = example();
    }
}

/// What `values` needs about the tag it walks.
struct TagCtx<'a> {
    class: &'a str,
    tag: &'a str,
    /// The file entry's resource kinds, by resource index.
    resource_kinds: &'a [String],
}

fn values<'a, 'd>(body: &'a Body<'d>, scopes: &mut Vec<&'a Body<'d>>, path: &str, ctx: &TagCtx<'_>, out: &mut Values) {
    use h5_meta::{EnumOption, Value, target_block};
    let TagCtx { class, tag, resource_kinds } = ctx;
    scopes.push(body);
    for n in &body.children {
        let step = n.name.unwrap_or("");
        let p = if path.is_empty() { step.to_owned() } else { format!("{path}.{step}") };
        let field = strip_indices(&p);
        match &n.content {
            Content::Value(Value::StringId(s)) => {
                out.string_ids += 1;
                match (&s.text, s.hash) {
                    (Some(_), _) => out.sid_resolved += 1,
                    (None, 0) => out.sid_zero += 1,
                    (None, h) => {
                        note(&mut out.sid_unresolved, format!("{class} | {field}"), || format!("{tag} 0x{h:08X}"))
                    }
                }
            }
            Content::Value(Value::Enum(v)) => {
                out.enums += 1;
                out.enum_negative += usize::from(*v < 0);
                if let Some(EnumOption::Missing { value, defined }) = n.enum_option()
                    && value >= 0
                {
                    note(&mut out.enum_missing, format!("{class} | {field}"), || {
                        format!("{tag}: {value} of {defined}")
                    });
                }
            }
            Content::Value(Value::BlockIndex(i)) => {
                out.block_indices += 1;
                if *i < 0 {
                    out.index_none += 1;
                    continue;
                }
                match target_block(n.field, scopes) {
                    Some(t) => {
                        let Content::Block(b) = &t.content else { unreachable!() };
                        if *i as u64 >= b.count as u64 {
                            note(&mut out.index_out_of_range, format!("{class} | {field}"), || {
                                format!("{tag}: {i} of {}", b.count)
                            });
                        }
                    }
                    None => *out.index_no_target.entry(format!("{class} | {field}")).or_default() += 1,
                }
            }
            Content::Value(Value::Real(x)) => {
                out.reals += 1;
                if !x.is_finite() {
                    note(&mut out.not_finite, format!("{class} | {field}"), || format!("{tag}: {x}"));
                }
            }
            Content::Value(Value::Reals(xs)) => {
                out.reals += xs.len();
                if xs.iter().any(|x| !x.is_finite()) {
                    note(&mut out.not_finite, format!("{class} | {field}"), || format!("{tag}: {xs:?}"));
                }
            }
            Content::Value(Value::Resource(r)) => {
                out.resources += 1;
                if let Some(k) = r.index {
                    match resource_kinds.get(k as usize) {
                        Some(kind) => {
                            *out.resource_kinds
                                .entry((class.to_string(), field.clone()))
                                .or_default()
                                .entry(kind.clone())
                                .or_default() += 1
                        }
                        None => out.resource_index_past_end += 1,
                    }
                }
            }
            Content::Struct(b) | Content::Array(b) => values(b, scopes, &p, ctx, out),
            Content::Block(b) => {
                if let Some(max) = n.field.attr("max").and_then(|m| m.parse::<u32>().ok())
                    && b.count > max
                {
                    note(&mut out.over_max, format!("{class} | {field}"), || format!("{tag}: {} > max {max}", b.count));
                }
                for (i, e) in b.entries.iter().enumerate() {
                    values(e, scopes, &format!("{p}[{i}]"), ctx, out);
                }
            }
            _ => {}
        }
    }
    scopes.pop();
}

/// PRD §9 checks 5 and 6, and the Q9 lead (resource structs ↔ resources).
#[test]
#[ignore]
fn values_are_plausible() {
    use h5_module::names::parse_resource_name;
    use h5_module::tag::StructKind;

    let deploy = PathBuf::from(std::env::var("H5_DEPLOY").expect("set H5_DEPLOY"));
    let defs = Definitions::load(&tags_dir()).unwrap();
    let mut parsed: HashMap<FourCc, Option<Definition>> = HashMap::new();
    let start = Instant::now();
    let mut v = Values::default();
    let (mut tags, mut tags_resource_count_equal, mut tags_with_resources) = (0usize, 0usize, 0usize);
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
            let kinds: Vec<String> = m
                .entry_resources(i)
                .unwrap()
                .iter()
                .map(|&f| m.name(f).ok().and_then(parse_resource_name).map_or("?".into(), |r| r.kind.to_owned()))
                .collect();
            let resource_structs = header.tag_structs.iter().filter(|s| s.kind == StructKind::Resource).count();
            if !kinds.is_empty() || resource_structs > 0 {
                tags_with_resources += 1;
                tags_resource_count_equal += usize::from(resource_structs == kinds.len());
            }
            let class_key = format!("{} {class}", e.class_code);
            let ctx = TagCtx { class: &class_key, tag: name, resource_kinds: &kinds };
            values(&r.root, &mut Vec::new(), "", &ctx, &mut v);
        }
    }
    println!("{tags} tags in {:.2?}", start.elapsed());
    let sum = |m: &BTreeMap<String, (usize, String)>| m.values().map(|e| e.0).sum::<usize>();
    println!(
        "string_ids {}: resolved {}, zero {}, unresolved {} at {} fields",
        v.string_ids,
        v.sid_resolved,
        v.sid_zero,
        sum(&v.sid_unresolved),
        v.sid_unresolved.len()
    );
    for (k, (n, ex)) in v.sid_unresolved.iter().take(6) {
        println!("    {n:6} {k}, e.g. {ex}");
    }
    println!(
        "enums {}: negative {}, no option at a positive value {} at {} fields",
        v.enums,
        v.enum_negative,
        sum(&v.enum_missing),
        v.enum_missing.len()
    );
    for (k, (n, ex)) in v.enum_missing.iter().take(8) {
        println!("    {n:6} {k}, e.g. {ex}");
    }
    println!(
        "block indices {}: none (-1) {}, no target block found {} at {} fields, out of range {} at {} fields",
        v.block_indices,
        v.index_none,
        v.index_no_target.values().sum::<usize>(),
        v.index_no_target.len(),
        sum(&v.index_out_of_range),
        v.index_out_of_range.len()
    );
    for (k, n) in v.index_no_target.iter().take(6) {
        println!("    no target: {n:6} {k}");
    }
    for (k, (n, ex)) in v.index_out_of_range.iter().take(6) {
        println!("    out of range: {n:6} {k}, e.g. {ex}");
    }
    println!("tag blocks over their max: {} at {} fields", sum(&v.over_max), v.over_max.len());
    for (k, (n, ex)) in v.over_max.iter().take(6) {
        println!("    {n:6} {k}, e.g. {ex}");
    }
    println!("reals {}: not finite {} at {} fields", v.reals, sum(&v.not_finite), v.not_finite.len());
    for (k, (n, ex)) in v.not_finite.iter().take(6) {
        println!("    {n:6} {k}, e.g. {ex}");
    }
    let mixed: Vec<_> = v.resource_kinds.iter().filter(|(_, kinds)| kinds.len() > 1).collect();
    println!(
        "resources {}: tags whose resource structs = resources {} of {}; index past the end {}; (class, field) places {}, with more than one kind {}",
        v.resources,
        tags_resource_count_equal,
        tags_with_resources,
        v.resource_index_past_end,
        v.resource_kinds.len(),
        mixed.len()
    );
    for ((class, field), kinds) in v.resource_kinds.iter().take(12) {
        println!("    {class} | {field}: {kinds:?}");
    }
    for ((class, field), kinds) in mixed.iter().take(6) {
        println!("    mixed: {class} | {field}: {kinds:?}");
    }
}

/// Check 5: a resolved string_id's text hashes back to the stored value.
fn hashes_back(body: &Body<'_>, ok: &mut usize, bad: &mut Vec<String>) {
    use h5_meta::Value;
    for n in &body.children {
        match &n.content {
            Content::Value(Value::StringId(s)) => {
                // Strings are Windows-1252; the 1-byte encoding of these texts is their chars.
                if let Some(t) = &s.text {
                    let bytes: Option<Vec<u8>> = t.chars().map(|c| u8::try_from(c as u32).ok()).collect();
                    match bytes {
                        Some(b) if h5_module::murmur3::murmur3_x86_32(&b, 0) == s.hash => *ok += 1,
                        _ if bad.len() < 5 => bad.push(format!("{t:?} 0x{:08X}", s.hash)),
                        _ => {}
                    }
                }
            }
            Content::Struct(b) | Content::Array(b) => hashes_back(b, ok, bad),
            Content::Block(b) => b.entries.iter().for_each(|e| hashes_back(e, ok, bad)),
            _ => {}
        }
    }
}

/// M3 done-when (PRD §9): checks 1–7 on every tag of weap, proj, bipd, vehi, effe.
/// Checks 1, 2, 4 and 5 must have zero failures; checks 3 (GUIDs) and 6
/// (plausibility) are reported per field, to compare with their explanations
/// (decisions D32, format-notes "Values"). Check 7 is `every_tag_reads_with_its_definition`.
#[test]
#[ignore]
fn m3_checks_on_the_five_classes() {
    const CLASSES: [&str; 5] = ["weap", "proj", "bipd", "vehi", "effe"];
    let deploy = PathBuf::from(std::env::var("H5_DEPLOY").expect("set H5_DEPLOY"));
    let defs = Definitions::load(&tags_dir()).unwrap();
    let mut parsed: HashMap<FourCc, Option<Definition>> = HashMap::new();
    let start = Instant::now();
    #[derive(Default)]
    struct Class {
        tags: usize,
        divergences: usize,
        header_failures: usize,
        problems: usize,
        sids_ok: usize,
        sids_bad: Vec<String>,
        found: Found,
        values: Values,
    }
    let mut by_class: BTreeMap<&str, Class> = BTreeMap::new();
    for module in find_modules(&deploy).unwrap() {
        let map = map_file(&module.path).unwrap();
        let m = Module::parse(&map).unwrap();
        for (i, e) in m.entries.iter().enumerate().filter(|(_, e)| e.kind() == EntryKind::Tag) {
            let Some(code) = CLASSES.iter().find(|c| FourCc::from_code(c) == Some(e.class_code)) else { continue };
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
                panic!("{code}: no definition");
            };
            let (h, data) = (m.read_section(i, Section::Header).unwrap(), m.read_section(i, Section::TagData).unwrap());
            let header = TagHeader::parse(&h).unwrap();
            let c = by_class.entry(code).or_default();
            c.tags += 1;
            // Check 2 (reference values) and the M2 header checks.
            c.header_failures += header.check(e).len() + header.check_tag_data(&data).len();
            let r = read_tag(def, &header, &data);
            c.divergences += r.divergences.len();
            let before = c.found.problems.values().map(|p| p.0).sum::<usize>();
            walk(&r.root, "", name, &mut c.found);
            c.problems += c.found.problems.values().map(|p| p.0).sum::<usize>() - before;
            hashes_back(&r.root, &mut c.sids_ok, &mut c.sids_bad);
            let kinds: Vec<String> = Vec::new();
            let ctx = TagCtx { class: code, tag: name, resource_kinds: &kinds };
            values(&r.root, &mut Vec::new(), "", &ctx, &mut c.values);
        }
    }
    println!("checks 1–7 on {:?} in {:.2?}", CLASSES, start.elapsed());
    let sum = |m: &BTreeMap<String, (usize, String)>| m.values().map(|e| e.0).sum::<usize>();
    let mut failed = false;
    for (code, c) in &by_class {
        println!(
            "{code}: {} tags | 1 divergences {} | 2 header/reference failures {} | 4 size problems {} | 5 string_ids hashing back {} (not {}) | 3 GUID mismatches {} at {} fields | 6 enum no option {}, block index out of range {} / no target {}, over max {}, not finite {}",
            c.tags,
            c.divergences,
            c.header_failures,
            c.problems,
            c.sids_ok,
            c.sids_bad.len(),
            c.found.guid_mismatches.values().sum::<usize>(),
            c.found.guid_mismatches.len(),
            sum(&c.values.enum_missing),
            sum(&c.values.index_out_of_range),
            c.values.index_no_target.values().sum::<usize>(),
            sum(&c.values.over_max),
            sum(&c.values.not_finite),
        );
        for ((field, _), n) in &c.found.guid_mismatches {
            println!("    GUID mismatch: {n:6} at {field}");
        }
        for (k, (n, ex)) in c.values.enum_missing.iter().chain(&c.values.index_out_of_range).chain(&c.values.not_finite)
        {
            println!("    plausibility: {n:6} {k}, e.g. {ex}");
        }
        for (k, n) in &c.values.index_no_target {
            println!("    plausibility: {n:6} no target block: {k}");
        }
        failed |= c.divergences + c.header_failures + c.problems + c.sids_bad.len() > 0;
    }
    assert_eq!(by_class.len(), 5, "every class has sample tags");
    assert!(!failed, "checks 1, 2, 4 or 5 failed: see above");
}

/// Reads every tag of `H5_DEPLOY` that has a definition and passes it to `f`
/// with its class key (`weap weapon`) and name.
fn for_each_read_tag(mut f: impl FnMut(&str, &str, &h5_meta::ReadTag<'_>)) -> usize {
    let deploy = PathBuf::from(std::env::var("H5_DEPLOY").expect("set H5_DEPLOY"));
    let defs = Definitions::load(&tags_dir()).unwrap();
    let mut parsed: HashMap<FourCc, Option<Definition>> = HashMap::new();
    let mut tags = 0;
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
            tags += 1;
            f(&format!("{} {class}", e.class_code), name, &read_tag(def, &header, &data));
        }
    }
    tags
}

/// One group of the angle survey: how its non-zero values look as radians
/// and as degrees.
#[derive(Default)]
struct AngleStats {
    values: usize,
    nonzero: usize,
    above_pi: usize,
    above_two_pi: usize,
    /// × 180/π is a whole number of degrees (the value was typed in degrees and stored in radians).
    whole_degrees_if_radians: usize,
    /// The value itself is a whole number (stored as typed).
    whole_as_stored: usize,
    /// The largest |value| and where.
    largest: (f32, String),
}

impl AngleStats {
    fn add(&mut self, v: f32, at: impl FnOnce() -> String) {
        self.values += 1;
        if v == 0.0 || !v.is_finite() {
            return;
        }
        self.nonzero += 1;
        let a = v.abs();
        self.above_pi += usize::from(a > std::f32::consts::PI + 1e-4);
        self.above_two_pi += usize::from(a > std::f32::consts::TAU + 1e-4);
        let d = f64::from(v).to_degrees();
        self.whole_degrees_if_radians += usize::from((d - d.round()).abs() < 1e-3);
        self.whole_as_stored += usize::from((f64::from(v) - f64::from(v).round()).abs() < 1e-5);
        if a > self.largest.0 {
            self.largest = (a, at());
        }
    }
}

/// D31 survey: are angle fields stored in radians or in degrees? Every
/// definition labels them `degrees`, the editor's unit (PRD §5.3 units).
#[test]
#[ignore]
fn angle_units_survey() {
    use h5_defs::field_type::FieldType as T;
    use h5_meta::Value;

    /// Per real-typed field with a degree unit: (class field path) → (radian-like, other non-zero).
    type ByField = BTreeMap<String, (usize, usize)>;

    fn visit(
        body: &Body<'_>,
        path: &str,
        tag: (&str, &str),
        out: &mut BTreeMap<String, AngleStats>,
        by_field: &mut ByField,
    ) {
        for n in &body.children {
            let p = format!("{path}.{}", n.name.unwrap_or_default());
            match &n.content {
                Content::Struct(b) | Content::Array(b) => visit(b, &p, tag, out, by_field),
                Content::Block(b) => b.entries.iter().for_each(|e| visit(e, &p, tag, out, by_field)),
                Content::Value(v) => {
                    let h5_defs::definition::FieldKind::Known(t) = n.field.kind else { continue };
                    let angle = matches!(t, T::Angle | T::AngleBounds | T::RealEulerAngles2d | T::RealEulerAngles3d);
                    let units = n.field.name.units.as_deref().unwrap_or("");
                    // Reals whose unit is a degree, for comparison.
                    let degree_real = !angle && t == T::Real && units.contains("degree");
                    if !angle && !degree_real {
                        continue;
                    }
                    let key = format!("{} [{}]", t.xml_name(), if units.is_empty() { "no unit" } else { units });
                    let s = out.entry(key).or_default();
                    match v {
                        Value::Real(x) => {
                            s.add(*x, || format!("{} {p}", tag.1));
                            if degree_real && *x != 0.0 {
                                let d = f64::from(*x).to_degrees();
                                let radian_like = x.abs() <= std::f32::consts::TAU
                                    && (d - d.round()).abs() < 1e-3
                                    && x.fract() != 0.0;
                                let e = by_field.entry(format!("{} {p}", tag.0)).or_default();
                                if radian_like { e.0 += 1 } else { e.1 += 1 }
                            }
                        }
                        Value::Reals(xs) => xs.iter().for_each(|x| s.add(*x, || format!("{} {p}", tag.1))),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    let start = Instant::now();
    let mut stats: BTreeMap<String, AngleStats> = BTreeMap::new();
    let mut by_field = ByField::new();
    let tags = for_each_read_tag(|class, name, r| visit(&r.root, "", (class, name), &mut stats, &mut by_field));
    println!("{tags} tags in {:.2?}", start.elapsed());
    println!("type [unit]: values, non-zero, |v|>π, |v|>2π, whole degrees if radians, whole as stored, largest |v|");
    for (k, s) in stats.iter().filter(|(_, s)| s.nonzero > 0) {
        println!(
            "{k}: {} {} {} {} {} {} {} at {}",
            s.values,
            s.nonzero,
            s.above_pi,
            s.above_two_pi,
            s.whole_degrees_if_radians,
            s.whole_as_stored,
            s.largest.0,
            s.largest.1
        );
    }
    println!(
        "real-typed fields with a degree unit, radian-like values (non-zero, ≤ 2π, whole degrees if radians) / others:"
    );
    let mut fields: Vec<_> = by_field.iter().filter(|(_, c)| c.0 > 0).collect();
    fields.sort_by_key(|(_, c)| std::cmp::Reverse(c.0));
    for (k, (rad, other)) in fields.iter().take(20) {
        println!("  {rad:6} / {other:6}  {k}");
    }
}
