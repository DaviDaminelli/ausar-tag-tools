//! M9 survey (research composites, PRD §9): before any plan, what each
//! candidate would save on real files.
//! 1. One field across every class that has it: field paths in more than one
//!    class's definition, by class section (inherited `OBJECT` fields or a
//!    name shared by chance), with the tags they cover.
//! 2. One field across a tag's update layers: tags with differing copies,
//!    versions per tag, fields differing between layers, the fields that
//!    change most.
//! 3. A one-call summary of a tag: what orienting on one tag costs now
//!    (outline, references out and in, copies, maps), in calls and characters.
//!
//! Counts only; conclusions go to format-notes and progress. Ignored by default.
//!
//! ```text
//! H5_CONFIG=<config> cargo test --release -p h5tags-core --test real_research_survey -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use h5_defs::definition::{Custom, Field, FieldKind, custom_role};
use h5_defs::field_type::FieldType;
use h5_meta::FieldPath;
use h5_module::FourCc;
use h5_module::names::module_layer;
use h5tags_core::catalog::{Build, Copy, Tag};
use h5tags_core::diff::diff_at;
use h5tags_core::query::choose_copy;
use h5tags_core::stored::{load_class_definition, read_stored_tag};
use h5tags_core::{Command, Config, Format, Session};

fn session() -> Session {
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    Session::open(Config::load(&config).unwrap()).unwrap()
}

/// One value field of a definition: (path, class section it is under, type).
type Leaf = (String, String, String);
/// A field path in one class: (class, section, type, the class's tags).
type InClass = (FourCc, String, String, usize);

/// Every value field of `fields` with its path (`[*]` inside tag blocks) and
/// the class section above it (top level only: `OBJECT`, `ITEM`, `WEAPON`).
fn leaves(fields: &[Field], path: &str, section: &mut String, top: bool, out: &mut Vec<Leaf>) {
    for (i, f) in fields.iter().enumerate() {
        if f.is_internal() {
            continue;
        }
        if f.kind == FieldKind::Known(FieldType::Custom) {
            if let (true, Some(Custom::ClassSection(s))) = (top, custom_role(fields, i)) {
                *section = s;
            }
            continue;
        }
        let Some(step) = &f.step else { continue };
        let p = if path.is_empty() { step.clone() } else { format!("{path}.{step}") };
        if f.children.is_empty() {
            let ty = match &f.kind {
                FieldKind::Known(t) => t.xml_name().to_owned(),
                FieldKind::Unknown(n) => n.clone(),
            };
            out.push((p, section.clone(), ty));
            continue;
        }
        let inside = if f.kind == FieldKind::Known(FieldType::Block64) { format!("{p}[*]") } else { p };
        leaves(&f.children, &inside, section, false, out);
    }
}

#[test]
#[ignore]
fn fields_shared_by_classes() {
    let s = session();
    for b in s.builds() {
        let mut by_path: HashMap<String, Vec<InClass>> = HashMap::new();
        // section → (classes, tags)
        let mut sections: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut classes = 0;
        for (code, info) in &b.classes {
            let Ok(Some(def)) = load_class_definition(b, *code) else { continue };
            classes += 1;
            let mut out = Vec::new();
            leaves(&def.fields, "", &mut String::new(), true, &mut out);
            let mut seen = HashSet::new();
            for (path, section, ty) in out {
                if seen.insert(section.clone()) {
                    let e = sections.entry(section.clone()).or_default();
                    e.0 += 1;
                    e.1 += info.tag_count;
                }
                by_path.entry(path).or_default().push((*code, section, ty, info.tag_count));
            }
        }
        let shared: Vec<(&String, &Vec<InClass>)> =
            by_path.iter().filter(|(_, v)| v.iter().filter(|x| x.3 > 0).count() >= 2).collect();
        let with = |n: usize| shared.iter().filter(|(_, v)| v.iter().filter(|x| x.3 > 0).count() >= n).count();
        let same_section = shared.iter().filter(|(_, v)| v.iter().all(|x| x.1 == v[0].1)).count();
        let same_type = shared.iter().filter(|(_, v)| v.iter().all(|x| x.2 == v[0].2)).count();
        println!(
            "\n== {}: {classes} definitions, {} field paths; in 2+ classes with tags {} (5+ {}, 10+ {}, 20+ {}); \
             of those under the same class section in every class {same_section}, of the same type {same_type}",
            b.name,
            by_path.len(),
            shared.len(),
            with(5),
            with(10),
            with(20)
        );
        let mut secs: Vec<_> = sections.iter().filter(|(_, (n, _))| *n >= 2).collect();
        secs.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
        let parts: Vec<String> =
            secs.iter().take(12).map(|(s, (n, t))| format!("{s:?} {n} classes / {t} tags")).collect();
        println!("  class sections in 2+ definitions: {}", parts.join(", "));
        // A named section (inherited fields): its classes, and the paths under
        // it that every one of them has.
        for (sec, _) in secs.iter().filter(|(s, _)| !s.is_empty()) {
            let mut members: Vec<(FourCc, usize)> = by_path
                .values()
                .flatten()
                .filter(|x| &x.1 == *sec)
                .map(|x| (x.0, x.3))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            members.sort();
            let all = by_path
                .values()
                .filter(|v| {
                    let here: HashSet<FourCc> = v.iter().filter(|x| &x.1 == *sec).map(|x| x.0).collect();
                    here.len() == members.len()
                })
                .count();
            let names: Vec<String> = members.iter().map(|(c, n)| format!("{} {n}", c.to_string().trim_end())).collect();
            println!("  {sec}: paths every class has {all}; classes (tags): {}", names.join(", "));
        }
        let chance = shared.iter().filter(|(_, v)| v.iter().filter(|x| x.3 > 0).all(|x| x.1.is_empty())).count();
        println!("  shared paths outside any class section (same name or struct reused, not inherited): {chance}");
        let mut top: Vec<_> = shared
            .iter()
            .map(|(p, v)| {
                let with_tags: Vec<_> = v.iter().filter(|x| x.3 > 0).collect();
                let tags: usize = with_tags.iter().map(|x| x.3).sum();
                let secs: HashSet<&str> = with_tags.iter().map(|x| x.1.as_str()).collect();
                (p.as_str(), with_tags.len(), tags, secs.len())
            })
            .collect();
        top.sort_by_key(|&(p, n, t, _)| (std::cmp::Reverse(n), std::cmp::Reverse(t), p));
        println!("  most shared (path: classes with tags, their tags, distinct sections):");
        for (p, n, t, secs) in top.iter().take(15) {
            println!("    {p}: {n} classes, {t} tags, {secs} sections");
        }
        let mut by_tags = top.clone();
        by_tags.retain(|&(_, n, _, _)| n >= 3);
        by_tags.sort_by_key(|&(p, _, t, _)| (std::cmp::Reverse(t), p));
        println!("  most tags covered (3+ classes):");
        for (p, n, t, secs) in by_tags.iter().take(10) {
            println!("    {p}: {t} tags in {n} classes, {secs} sections");
        }
    }
}

/// The top-level fields of a definition split at its class section labels:
/// (section, fields); fields before the first label are section "".
fn sections(fields: &[Field]) -> Vec<(String, &[Field])> {
    let mut out: Vec<(String, &[Field])> = Vec::new();
    let (mut name, mut start) = (String::new(), 0);
    for i in 0..fields.len() {
        if let Some(Custom::ClassSection(s)) = custom_role(fields, i) {
            out.push((std::mem::replace(&mut name, s), &fields[start..i]));
            start = i + 1;
        }
    }
    out.push((name, &fields[start..]));
    out
}

/// What differs between two definitions of one field.
fn what_differs(a: &Field, b: &Field) -> &'static str {
    match () {
        _ if a.kind != b.kind => "type",
        _ if a.name.display != b.name.display => "name",
        _ if a.name.units != b.name.units => "units",
        _ if a.name.help != b.name.help => "help",
        _ if a.raw_name != b.raw_name => "markers or other name parts",
        _ if a.options != b.options => "options",
        _ if a.children != b.children => "children",
        _ if a.step != b.step => "path step",
        _ => "attributes",
    }
}

/// M9 step 1 (D97): each parent class's own section (`obje` → `OBJECT`,
/// `item` → `ITEM`, `unit` → `UNIT`, `devi` → `DEVICE`) against that section
/// in every definition that has it: identical fields can be merged across
/// classes; any other is listed with what differs.
#[test]
#[ignore]
fn parent_definitions_against_inheriting_classes() {
    let s = session();
    for b in s.builds() {
        println!("\n== {}", b.name);
        let parents: Vec<(FourCc, std::sync::Arc<h5_defs::definition::Definition>)> = ["obje", "item", "unit", "devi"]
            .iter()
            .filter_map(|c| {
                let code = FourCc::from_code(c)?;
                Some((code, load_class_definition(b, code).ok()??))
            })
            .collect();
        for (parent, pdef) in &parents {
            let psecs = sections(&pdef.fields);
            let (own, own_fields) = psecs.last().expect("a section").clone();
            let mut identical = Vec::new();
            let mut differ: Vec<String> = Vec::new();
            for file in b.defs.files() {
                let Some(code) = FourCc::from_code(&file.file_code) else { continue };
                if code == *parent {
                    continue;
                }
                let Ok(Some(def)) = load_class_definition(b, code) else { continue };
                let secs = sections(&def.fields);
                let Some((_, fields)) = secs.iter().find(|(n, _)| *n == own) else { continue };
                let tags = b.classes.get(&code).map_or(0, |c| c.tag_count);
                let chain: Vec<&str> = secs.iter().map(|(n, _)| n.as_str()).filter(|n| !n.is_empty()).collect();
                let label = format!("{} {tags} [{}]", code.to_string().trim_end(), chain.join(" > "));
                if *fields == own_fields {
                    identical.push(label);
                    continue;
                }
                let by_step = |fs: &[Field]| -> HashMap<String, Field> {
                    fs.iter().filter_map(|f| Some((f.step.clone()?, f.clone()))).collect()
                };
                let (mine, theirs) = (by_step(fields), by_step(own_fields));
                let mut notes: Vec<String> = Vec::new();
                for f in own_fields {
                    let Some(step) = &f.step else { continue };
                    match mine.get(step) {
                        None => notes.push(format!("{step}: missing")),
                        Some(g) if g != f => notes.push(format!("{step}: {}", what_differs(f, g))),
                        _ => {}
                    }
                }
                let changed = notes.len();
                let extra: Vec<&String> =
                    fields.iter().filter_map(|f| f.step.as_ref()).filter(|st| !theirs.contains_key(*st)).collect();
                // The parent's fields first, in its order, the class's own after them.
                let prefix = fields.len() >= own_fields.len() && fields[..own_fields.len()] == *own_fields;
                let shown: Vec<&str> = notes.iter().map(String::as_str).take(6).collect();
                let extras: Vec<&str> = extra.iter().map(|s| s.as_str()).take(4).collect();
                differ.push(format!(
                    "{label}: parent fields missing or changed {changed}{}; parent fields first, in order: {prefix}; \
                     own fields without a section label {} ({}{})",
                    if shown.is_empty() { String::new() } else { format!(" ({})", shown.join("; ")) },
                    extra.len(),
                    extras.join(", "),
                    if extra.len() > 4 { ", …" } else { "" }
                ));
                assert_eq!(changed, 0, "{label}: inherited fields differ");
            }
            let named = own_fields.iter().filter(|f| f.step.is_some()).count();
            println!(
                "  {} ({own}, {named} top-level fields): identical in {} definitions: {}",
                parent.to_string().trim_end(),
                identical.len(),
                identical.join(", ")
            );
            for d in &differ {
                println!("    differs: {d}");
            }
        }
    }
}

/// A tag's copies in layer order (base first), each module's layer number.
fn copies_by_layer(b: &Build, t: &Tag) -> Vec<(u32, Copy)> {
    let mut v: Vec<(u32, Copy)> = t.copies.iter().map(|&c| (module_layer(&b.module(c).rel_path).version, c)).collect();
    v.sort_by_key(|&(l, c)| (l, b.module(c).rel_path.to_ascii_lowercase()));
    v
}

/// `barrels[0].x` → `barrels[*].x`: a field across entries.
fn any_entry(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut skip = false;
    for c in path.chars() {
        match c {
            '[' => {
                out.push_str("[*]");
                skip = true;
            }
            ']' => skip = false,
            c if !skip => out.push(c),
            _ => {}
        }
    }
    out
}

#[test]
#[ignore]
fn fields_across_update_layers() {
    let s = session();
    for b in s.builds() {
        let layered: Vec<&Tag> = b.tags.iter().filter(|t| t.copies.len() > 1).collect();
        let mut by_copies: BTreeMap<usize, usize> = BTreeMap::new();
        let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
        let (mut pairs, mut unread, mut same_bytes, mut no_field_diff) = (0usize, 0usize, 0usize, 0usize);
        let mut diffs_per_pair: Vec<usize> = Vec::new();
        let mut versions_3plus = 0usize;
        // (class, field across entries) → tags where it changed between two layers
        let mut changed: HashMap<(String, String), usize> = HashMap::new();
        for t in &layered {
            *by_copies.entry(t.copies.len()).or_default() += 1;
            *by_class.entry(t.class.to_string()).or_default() += 1;
            let Ok(Some(def)) = load_class_definition(b, t.class) else {
                unread += 1;
                continue;
            };
            let copies = copies_by_layer(b, t);
            let mut stored = Vec::new();
            for &(_, c) in &copies {
                match read_stored_tag(b, c) {
                    Ok(x) => stored.push(x),
                    Err(_) => unread += 1,
                }
            }
            let distinct: HashSet<&[u8]> = stored.iter().map(|x| x.tag_data.as_slice()).collect();
            versions_3plus += usize::from(distinct.len() >= 3);
            let reads: Vec<_> = stored.iter().map(|x| h5_meta::read_tag(&def, &x.header, &x.tag_data)).collect();
            let mut fields_here: HashSet<String> = HashSet::new();
            for (i, pair) in reads.windows(2).enumerate() {
                pairs += 1;
                if stored[i].tag_data == stored[i + 1].tag_data {
                    same_bytes += 1;
                    continue;
                }
                let d = diff_at(&pair[0].root, &pair[1].root, &FieldPath::default());
                no_field_diff += usize::from(d.is_empty());
                diffs_per_pair.push(d.len());
                for x in &d {
                    fields_here.insert(any_entry(&x.path.display(&def.fields)));
                }
            }
            for f in fields_here {
                *changed.entry((t.class.to_string(), f)).or_default() += 1;
            }
        }
        diffs_per_pair.sort_unstable();
        let pct = |p: f64| diffs_per_pair.get(((diffs_per_pair.len() as f64 - 1.0) * p) as usize).copied().unwrap_or(0);
        println!(
            "\n== {}: tags with copies {} ({by_copies:?} by copies); 3+ distinct versions {versions_3plus}; unread {unread}",
            b.name,
            layered.len()
        );
        let mut classes: Vec<_> = by_class.iter().collect();
        classes.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        let parts: Vec<String> = classes.iter().take(12).map(|(c, n)| format!("{c} {n}")).collect();
        println!("  by class: {}", parts.join(", "));
        println!(
            "  layer pairs {pairs}: same bytes {same_bytes}, bytes differ but no field differs {no_field_diff}; \
             differing fields per pair median {}, p90 {}, max {}",
            pct(0.5),
            pct(0.9),
            diffs_per_pair.last().copied().unwrap_or(0)
        );
        let mut top: Vec<_> = changed.iter().collect();
        top.sort_by_key(|((c, f), n)| (std::cmp::Reverse(**n), c.clone(), f.clone()));
        println!("  fields that changed in the most tags (class field: tags):");
        for ((c, f), n) in top.iter().take(15) {
            println!("    {c} {f}: {n}");
        }
        let weap: Vec<_> = top.iter().filter(|((c, _), _)| c == "weap").take(8).collect();
        println!(
            "  weapons, most changed: {}",
            weap.iter().map(|((_, f), n)| format!("{f} {n}")).collect::<Vec<_>>().join(", ")
        );
    }
}

/// What a tool call returns as text, in characters (≈ 4 per token).
fn chars(s: &mut Session, tool: &str, args: serde_json::Value) -> usize {
    match Command::from_json(tool, args).and_then(|c| c.run(s)) {
        Ok(out) => out.render(Format::Text).chars().count(),
        Err(e) => e.render(Format::Text).chars().count(),
    }
}

#[test]
#[ignore]
fn what_orienting_on_a_tag_costs() {
    let mut s = session();
    let classes = ["weap", "vehi", "bipd", "proj", "effe", "jpt!", "scnr", "bitm", "snd!", "hsc*", "unic", "mode"];
    let names: Vec<String> = s.builds().iter().map(|b| b.name.clone()).collect();
    for build in names {
        println!("\n== {build}: characters per call (outline, refs out, refs in, copies, maps) = total; ≈ tokens");
        let picks: Vec<(String, String)> = {
            let b = s.builds().iter().find(|b| b.name == build).unwrap();
            classes
                .iter()
                .filter_map(|code| {
                    let class = FourCc::from_code(code)?;
                    let mut tags: Vec<(&Tag, u64)> = b
                        .tags
                        .iter()
                        .filter(|t| t.class == class)
                        .filter_map(|t| choose_copy(b, t, None).map(|c| (t, b.entry(c).uncompressed_size)))
                        .collect();
                    if tags.is_empty() {
                        return None;
                    }
                    tags.sort_by_key(|&(t, size)| (size, t.path.clone()));
                    let median = tags[tags.len() / 2].0;
                    let largest = tags[tags.len() - 1].0;
                    Some([median, largest].map(|t| (code.to_string(), t.global_id.to_string())))
                })
                .flatten()
                .collect()
        };
        let mut totals = Vec::new();
        for (class, id) in picks {
            let tag = format!("{build}:{id}");
            let a = |extra: serde_json::Value| {
                let mut v = serde_json::json!({ "tag": tag });
                v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
                v
            };
            let calls = [
                chars(&mut s, "outline", a(serde_json::json!({}))),
                chars(&mut s, "find_references", a(serde_json::json!({ "direction": "out" }))),
                chars(&mut s, "find_references", a(serde_json::json!({ "direction": "in" }))),
                chars(&mut s, "tag_copies", a(serde_json::json!({}))),
                chars(&mut s, "which_maps", a(serde_json::json!({}))),
            ];
            let total: usize = calls.iter().sum();
            totals.push(total);
            println!("  {class} {id}: {calls:?} = {total}; ≈ {}", total / 4);
        }
        totals.sort_unstable();
        println!(
            "  median total {} characters (≈ {} tokens) over {} tags",
            totals[totals.len() / 2],
            totals[totals.len() / 2] / 4,
            totals.len()
        );
    }
}
