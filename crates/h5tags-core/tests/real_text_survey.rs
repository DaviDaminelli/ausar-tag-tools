//! M8 survey: what the `unic` (multilingual_unicode_string_list) tags hold,
//! before any text feature. For every copy: its string references (a
//! string_id and one offset per language into `string data utf8`), how the
//! offsets and texts are stored, which languages are present; string_ids in
//! several `unic` tags and whether their texts agree; copies that differ
//! between layers; how many string_id names the other tags use have an
//! English text. Counts only; conclusions go to format-notes. Ignored by default.
//!
//! ```text
//! H5_CONFIG=<config> cargo test -p h5tags-core --test real_text_survey -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use h5_meta::{Content, Value, parse_path, read_tag, select};
use h5tags_core::catalog::{Build, Copy, Tag};
use h5tags_core::query::choose_copy;
use h5tags_core::references::header_scan;
use h5tags_core::stored::{load_definition, read_stored_tag};
use h5tags_core::{Config, Session};

/// The languages in the definition's order, after `string id`.
const LANGUAGES: [&str; 17] = [
    "english",
    "japanese",
    "german",
    "french",
    "spanish",
    "mexican spanish",
    "italian",
    "korean",
    "traditional chinese",
    "simplified chinese",
    "portuguese",
    "polish",
    "russian",
    "danish",
    "finnish",
    "dutch",
    "norwegian",
];

/// One string reference: (string_id hash, its name if the tag lists it, offsets).
type Entry = (u32, Option<String>, Vec<i64>);
/// A copy's string list (hash, offsets) and data: equal versions are equal copies.
type Version = (Vec<(u32, Vec<i64>)>, Vec<u8>);
/// Per class of tags referencing a unic tag: (tags, names, with text in a
/// referenced unic tag, with text in any unic tag).
type Referencing = (usize, usize, usize, usize);

/// A copy's string references and its string data.
fn read_unic(b: &Build, t: &Tag, copy: Copy) -> Result<(Vec<Entry>, Vec<u8>), String> {
    let def = load_definition(b, t).map_err(|e| e.to_string())?.ok_or("no definition")?;
    let stored = read_stored_tag(b, copy).map_err(|e| e.to_string())?;
    let read = read_tag(&def, &stored.header, &stored.tag_data);
    let node = |p: &str| -> Result<_, String> {
        let path = parse_path(&def.fields, p).map_err(|e| format!("{e:?}"))?;
        let sels = select(&read.root, &path).map_err(|e| format!("{e:?}"))?;
        Ok(sels.first().ok_or("no field")?.node.content.clone())
    };
    let Content::Block(block) = node("string references")? else { return Err("not a block".into()) };
    if let Some(p) = &block.problem {
        return Err(format!("string references not read: {p:?}"));
    }
    let mut entries = Vec::with_capacity(block.entries.len());
    for e in &block.entries {
        let mut values = e.children.iter().filter_map(|n| match &n.content {
            Content::Value(v) => Some(v),
            _ => None,
        });
        let Some(Value::StringId(id)) = values.next() else { return Err("entry without a string id".into()) };
        let offsets: Vec<i64> = values
            .filter_map(|v| match v {
                Value::Int(i) => Some(*i),
                _ => None,
            })
            .collect();
        entries.push((id.hash, id.text.clone(), offsets));
    }
    let Content::Value(Value::Data(d)) = node("string data utf8")? else { return Err("no string data".into()) };
    let data = match d.target {
        Some(target) => stored.header.block_data(&stored.tag_data, target).ok_or("data outside the tag data")?.to_vec(),
        None => Vec::new(),
    };
    Ok((entries, data))
}

/// The text at `offset`: up to the next NUL (`None` if none follows).
fn text_at(data: &[u8], offset: usize) -> Option<&[u8]> {
    let rest = data.get(offset..)?;
    rest.iter().position(|&c| c == 0).map(|end| &rest[..end])
}

/// M8 step 1 (D93): the domain reads every copy's string_ids and texts as the
/// survey's own reader does, and finds the same string_ids with different
/// English texts across the default copies.
#[test]
#[ignore]
fn the_domain_reads_every_text_as_the_survey_does() {
    use h5tags_core::text::{Lookup, Scope, TextAt, read_text_list, search_texts, text_languages};
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let s = Session::open(Config::load(&config).unwrap()).unwrap();
    let class = h5_module::FourCc::from_code("unic").unwrap();
    for b in s.builds() {
        assert_eq!(text_languages(b).unwrap(), LANGUAGES);
        let (mut entries, mut texts, mut english) = (0usize, 0usize, HashMap::<u32, HashSet<String>>::new());
        // Texts holding a backslash, and a backslash before n, r or t (which
        // the text output's \n \r \t escapes would make ambiguous, D94).
        let (mut backslash, mut escape_like) = (0usize, 0usize);
        for t in b.tags.iter().filter(|t| t.class == class) {
            let default = choose_copy(b, t, None);
            for &copy in &t.copies {
                let (list, data) = read_unic(b, t, copy).unwrap();
                let domain = read_text_list(b, t, copy).unwrap();
                assert_eq!(domain.string_ids.len(), list.len(), "{}", b.display_name(t));
                for (i, (hash, name, offsets)) in list.iter().enumerate() {
                    assert_eq!((domain.string_ids[i].hash, &domain.string_ids[i].text), (*hash, name));
                    for (lang, &off) in offsets.iter().enumerate() {
                        let survey = (off >= 0).then(|| text_at(&data, off as usize).unwrap());
                        let read = match domain.text(i, lang) {
                            TextAt::Text(x) => Some(x.as_bytes()),
                            TextAt::Missing => None,
                            TextAt::Unreadable => panic!("unreadable in {}", b.display_name(t)),
                        };
                        assert_eq!(read, survey);
                        texts += usize::from(read.is_some());
                        if let Some(r) = read {
                            backslash += usize::from(r.contains(&b'\\'));
                            escape_like += usize::from(r.windows(2).any(|w| matches!(w, [b'\\', b'n' | b'r' | b't'])));
                        }
                        if lang == 0 && default == Some(copy) {
                            english.entry(*hash).or_default().insert(String::from_utf8_lossy(read.unwrap()).into());
                        }
                    }
                }
                entries += list.len();
            }
        }
        let conflicting = english.values().filter(|v| v.len() > 1).count();
        let start = Instant::now();
        let all = search_texts(b, Scope::Latest, &Lookup::Every, Some(0));
        let elapsed = start.elapsed();
        assert_eq!((all.skipped.len(), all.unreadable), (0, 0));
        assert_eq!(all.differing().len(), conflicting);
        println!("   texts (every copy) with a backslash {backslash}, with a backslash before n, r or t {escape_like}");
        assert_eq!(escape_like, 0, "a stored \\n, \\r or \\t would read like an escaped line break (D94)");
        println!(
            "== {}: entries {entries}, texts {texts}; string_ids with different English texts {conflicting}; \
             every latest copy searched ({} tags, {} hits) in {elapsed:.2?}",
            b.name,
            all.searched,
            all.hits.len()
        );
    }
}

#[test]
#[ignore]
fn what_the_unic_tags_hold() {
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let s = Session::open(Config::load(&config).unwrap()).unwrap();
    let class = h5_module::FourCc::from_code("unic").unwrap();
    for b in s.builds() {
        let start = Instant::now();
        let tags: Vec<&Tag> = b.tags.iter().filter(|t| t.class == class).collect();
        let (mut copies, mut errors, mut entries, mut no_name) = (0, Vec::new(), 0, 0);
        let (mut out_of_range, mut no_nul, mut not_utf8, mut empty_text, mut data_bytes) = (0, 0, 0, 0, 0usize);
        let mut present = [0usize; 17];
        let mut negative: BTreeMap<i64, usize> = BTreeMap::new();
        let mut entry_widths: BTreeMap<usize, usize> = BTreeMap::new();
        let (mut multi, mut differ_in_layers) = (0, 0);
        // English text per string_id name, from each tag's default copy: (tag, text).
        let mut english: HashMap<String, Vec<(&Tag, String)>> = HashMap::new();
        for t in &tags {
            let mut versions: HashSet<Version> = HashSet::new();
            for &copy in &t.copies {
                copies += 1;
                let (list, data) = match read_unic(b, t, copy) {
                    Ok(x) => x,
                    Err(e) => {
                        errors.push(format!("{}: {e}", b.display_name(t)));
                        continue;
                    }
                };
                data_bytes += data.len();
                let default = choose_copy(b, t, None) == Some(copy);
                versions.insert((list.iter().map(|(h, _, o)| (*h, o.clone())).collect(), data.clone()));
                for (hash, name, offsets) in &list {
                    entries += 1;
                    *entry_widths.entry(offsets.len()).or_default() += 1;
                    if name.is_none() {
                        no_name += 1;
                    }
                    for (lang, &off) in offsets.iter().enumerate().take(17) {
                        if off < 0 {
                            *negative.entry(off).or_default() += 1;
                            continue;
                        }
                        let Some(text) = text_at(&data, off as usize) else {
                            if (off as usize) < data.len() {
                                no_nul += 1
                            } else {
                                out_of_range += 1
                            }
                            continue;
                        };
                        present[lang] += 1;
                        empty_text += usize::from(text.is_empty());
                        let Ok(text) = std::str::from_utf8(text) else {
                            not_utf8 += 1;
                            continue;
                        };
                        if lang == 0 && default {
                            let key = name.clone().unwrap_or_else(|| format!("#{hash:08x}"));
                            english.entry(key).or_default().push((t, text.to_owned()));
                        }
                    }
                }
            }
            if t.copies.len() > 1 {
                multi += 1;
                differ_in_layers += usize::from(versions.len() > 1);
            }
        }
        println!(
            "\n== {}: {} unic tags, {copies} copies, {} unreadable; read in {:.2?}",
            b.name,
            tags.len(),
            errors.len(),
            start.elapsed()
        );
        for e in errors.iter().take(5) {
            println!("  unreadable: {e}");
        }
        println!(
            "  string references {entries} (string id without a name in the tag {no_name}); offsets per entry {entry_widths:?}; string data {data_bytes} bytes"
        );
        println!(
            "  offsets: negative {negative:?}, past the data {out_of_range}, no NUL after {no_nul}; texts not UTF-8 {not_utf8}, empty {empty_text}"
        );
        let langs: Vec<String> = LANGUAGES.iter().zip(present).map(|(l, n)| format!("{l} {n}")).collect();
        println!("  texts per language: {}", langs.join(", "));
        println!("  tags with several copies {multi}, whose string list or data differ {differ_in_layers}");
        // The same string_id in several unic tags (default copies).
        let shared: Vec<&Vec<(&Tag, String)>> = english.values().filter(|v| v.len() > 1).collect();
        let conflicting = shared.iter().filter(|v| v.iter().any(|(_, x)| *x != v[0].1)).count();
        println!(
            "  distinct string_ids with English text {}; in several unic tags {}, with different texts {conflicting}",
            english.len(),
            shared.len()
        );
        if let Some(v) = shared.iter().find(|v| v.iter().any(|(_, x)| *x != v[0].1)) {
            for (t, x) in v.iter().take(3) {
                println!("    e.g. {}: {x:?}", b.display_name(t));
            }
        }
        // Coverage: the string_id names other tags use (their string tables,
        // without tag paths), and how many have English text.
        let (scan, _) = header_scan(b);
        let mut by_class: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let (mut names, mut with_text) = (0usize, 0usize);
        let lower: HashSet<String> = english.keys().map(|k| k.to_ascii_lowercase()).collect();
        for t in b.tags.iter().filter(|t| t.class != class) {
            let Some(c) = choose_copy(b, t, None).and_then(|c| scan.outgoing(c)) else { continue };
            for s in c.strings.split(|&x| x == 0).filter(|s| !s.is_empty() && !s.contains(&b'\\')) {
                let s = String::from_utf8_lossy(s).to_ascii_lowercase();
                names += 1;
                let hit = lower.contains(&s);
                with_text += usize::from(hit);
                let e = by_class.entry(t.class.to_string()).or_default();
                e.0 += 1;
                e.1 += usize::from(hit);
            }
        }
        println!("  other tags' string_id names (per default copy) {names}, with English text {with_text}");
        let mut top: Vec<(&String, &(usize, usize))> = by_class.iter().filter(|(_, v)| v.1 > 0).collect();
        top.sort_by_key(|(_, v)| std::cmp::Reverse(v.1));
        let parts: Vec<String> = top.iter().take(12).map(|(c, (n, h))| format!("{c} {h}/{n}")).collect();
        println!("  with text, by class (with/all): {}", parts.join(", "));
        // Tags that reference a unic tag themselves: by class, and how many of
        // their string_id names have English text in the unic tags they reference.
        let unic_texts: HashMap<i32, HashSet<String>> = english
            .iter()
            .flat_map(|(name, v)| v.iter().map(move |(t, _)| (t.global_id.0, name.to_ascii_lowercase())))
            .fold(HashMap::new(), |mut m, (id, n)| {
                m.entry(id).or_default().insert(n);
                m
            });
        let mut referencing: BTreeMap<String, Referencing> = BTreeMap::new();
        for t in b.tags.iter().filter(|t| t.class != class) {
            let Some(c) = choose_copy(b, t, None).and_then(|c| scan.outgoing(c)) else { continue };
            let targets: Vec<i32> = c.references.iter().filter(|r| r.class == class).map(|r| r.target.0).collect();
            if targets.is_empty() {
                continue;
            }
            let e = referencing.entry(t.class.to_string()).or_default();
            e.0 += 1;
            for s in c.strings.split(|&x| x == 0).filter(|s| !s.is_empty() && !s.contains(&b'\\')) {
                let s = String::from_utf8_lossy(s).to_ascii_lowercase();
                e.1 += 1;
                e.2 += usize::from(targets.iter().any(|id| unic_texts.get(id).is_some_and(|n| n.contains(&s))));
                e.3 += usize::from(lower.contains(&s));
            }
        }
        let mut refs: Vec<(&String, &Referencing)> = referencing.iter().collect();
        refs.sort_by_key(|(_, v)| std::cmp::Reverse(v.0));
        let total_tags: usize = refs.iter().map(|(_, v)| v.0).sum();
        println!(
            "  tags referencing a unic tag: {total_tags}, by class (tags: names, with text in a referenced unic / in any unic):"
        );
        for (c, (n, names, own, any)) in refs.iter().take(12) {
            println!("    {c} {n}: {names} names, {own} / {any}");
        }
        let weapon = h5_module::FourCc::from_code("weap").unwrap();
        for t in b.tags.iter().filter(|t| t.class == weapon).take(400) {
            let Some(c) = choose_copy(b, t, None).and_then(|c| scan.outgoing(c)) else { continue };
            let hits: Vec<String> = c
                .strings
                .split(|&x| x == 0)
                .filter_map(|s| {
                    english
                        .get(std::str::from_utf8(s).ok()?)
                        .map(|v| format!("{} = {:?}", String::from_utf8_lossy(s), v[0].1))
                })
                .collect();
            if !hits.is_empty() {
                println!("    {}: {}", b.display_name(t), hits.join("; "));
                break;
            }
        }
    }
}
