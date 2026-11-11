//! M7 survey: what the script tags hold, before any script tool. For every
//! copy of every `hsc*` and `luas` tag: source text or compiled code (and its
//! header bytes), the text's encoding, line endings and trailing NULs; the
//! strings that look like tag names and how many name an indexed tag; copies
//! that differ between layers, and between the first two builds. Counts only;
//! conclusions go to format-notes. Ignored by default.
//!
//! ```text
//! H5_CONFIG=<config> cargo test -p h5tags-core --test real_scripts_survey -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use h5_meta::{Content, Value, parse_path, read_tag, select};
use h5tags_core::catalog::{Build, Copy, Tag, TagLookupError, TagSpec};
use h5tags_core::query::choose_copy;
use h5tags_core::stored::{load_definition, read_stored_tag};
use h5tags_core::{Config, Session};

/// Each script class and the data field holding its script.
const SCRIPT_FIELDS: [(&str, &str); 2] = [("hsc*", "source file.source"), ("luas", "luaFileData")];
/// Compiled Lua (Havok Script included) starts with ESC `Lua`.
const BYTECODE_SIGNATURE: &[u8] = b"\x1bLua";

/// The bytes of a copy's script field.
fn script_bytes(b: &Build, t: &Tag, copy: Copy, field: &str) -> Result<Vec<u8>, String> {
    let def = load_definition(b, t).map_err(|e| e.to_string())?.ok_or("no definition")?;
    let stored = read_stored_tag(b, copy).map_err(|e| e.to_string())?;
    let read = read_tag(&def, &stored.header, &stored.tag_data);
    let path = parse_path(&def.fields, field).map_err(|e| format!("{e:?}"))?;
    let sels = select(&read.root, &path).map_err(|e| format!("{e:?}"))?;
    let node = sels.first().ok_or("no field")?.node;
    let Content::Value(Value::Data(d)) = &node.content else { return Err("not a data field".into()) };
    let Some(target) = d.target else { return Ok(Vec::new()) };
    let bytes = stored.header.block_data(&stored.tag_data, target).ok_or("data block outside the tag data")?;
    Ok(bytes.to_vec())
}

/// The string literals of Lua source, comments skipped. A backslash is taken
/// literally (the scripts write tag paths with single backslashes); `escapes`
/// counts the backslash pairs that standard Lua would read as escapes.
fn literals(text: &str, escapes: &mut BTreeMap<String, usize>) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                let end = if bytes[i + 2..].starts_with(b"[[") {
                    text[i..].find("]]").map_or(bytes.len(), |k| i + k + 2)
                } else {
                    text[i..].find('\n').map_or(bytes.len(), |k| i + k)
                };
                i = end;
            }
            q @ (b'\'' | b'"') => {
                let start = i + 1;
                let end = bytes[start..].iter().position(|&c| c == q || c == b'\n').map_or(bytes.len(), |k| start + k);
                let s = &text[start..end];
                for w in s.as_bytes().windows(2).filter(|w| w[0] == b'\\') {
                    if matches!(w[1], b'n' | b't' | b'r' | b'\\' | b'"' | b'\'' | b'0'..=b'9') {
                        *escapes.entry(format!("\\{}", w[1] as char)).or_default() += 1;
                    }
                }
                out.push(s.to_owned());
                i = end + 1;
            }
            b'[' if bytes.get(i + 1) == Some(&b'[') => {
                *escapes.entry("[[long string]]".into()).or_default() += 1;
                i = text[i..].find("]]").map_or(bytes.len(), |k| i + k + 2);
            }
            _ => i += 1,
        }
    }
    out
}

/// The strings compiled code holds, as the first run showed them stored: an
/// 8-byte big-endian length (the bytes plus a NUL), the bytes, a NUL. Taken
/// wherever such a length is followed by that many printable bytes and the NUL.
fn length_prefixed_strings(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 8 < bytes.len() {
        let len = u64::from_be_bytes(bytes[i..i + 8].try_into().unwrap()) as usize;
        let body = (2..=4096).contains(&len).then(|| bytes.get(i + 8..i + 8 + len)).flatten();
        if let Some(b) = body
            && b[len - 1] == 0
            && b[..len - 1].iter().all(|c| (0x20..0x7F).contains(c) || *c == b'\t')
        {
            out.push(String::from_utf8_lossy(&b[..len - 1]).into_owned());
            i += 8 + len;
        } else {
            i += 1;
        }
    }
    out
}

#[derive(Default)]
struct Names {
    /// Strings with `\` or `/` and only path characters: path-like.
    paths: usize,
    one_tag: usize,
    several: usize,
    not_indexed: BTreeSet<String>,
    /// `@…` strings: a compiled chunk's source file name.
    chunk_names: usize,
    /// Strings without a separator that equal tags' last path part: (one tag, several).
    name_only: (usize, usize),
    resolved_classes: BTreeMap<String, usize>,
}

/// A string as a tag path: `\\` read as `\`, `/` as `\`, lowercase, no leading `\`.
fn normalized(s: &str) -> String {
    s.replace("\\\\", "\\").replace('/', "\\").to_ascii_lowercase().trim_start_matches('\\').to_owned()
}

/// The tag a path names: as written (a class suffix picks the class), else
/// without a `.lua` source extension (compiled scripts name other scripts so).
fn lookup<'b>(b: &'b Build, p: &str) -> Result<&'b Tag, TagLookupError<'b>> {
    match b.find_tag(&TagSpec::Name(p.to_owned())) {
        Err(TagLookupError::NotFound) if p.ends_with(".lua") => {
            b.find_tag(&TagSpec::Name(p.trim_end_matches(".lua").to_owned()))
        }
        other => other,
    }
}

fn classify(b: &Build, last_parts: &HashMap<String, usize>, s: &str, names: &mut Names) {
    if s.starts_with('@') {
        names.chunk_names += 1;
        return;
    }
    let path_chars = |c: char| c.is_ascii_alphanumeric() || "\\/_.-".contains(c);
    if (s.contains('\\') || s.contains('/')) && s.len() >= 3 && s.chars().all(path_chars) {
        names.paths += 1;
        let p = normalized(s);
        match lookup(b, &p) {
            Ok(t) => {
                names.one_tag += 1;
                *names.resolved_classes.entry(t.class.to_string()).or_default() += 1;
            }
            Err(TagLookupError::Ambiguous(_)) => names.several += 1,
            Err(_) => {
                names.not_indexed.insert(p);
            }
        }
    } else if let Some(&n) = last_parts.get(&s.to_ascii_lowercase()) {
        if n == 1 { names.name_only.0 += 1 } else { names.name_only.1 += 1 }
    }
}

/// M7 step 2: the domain (`script::read_script`) reads every script copy as
/// this survey's own code does: the same form, the same text without its
/// trailing NULs, the same compiled strings.
#[test]
#[ignore]
fn the_domain_reads_every_script_as_the_survey_does() {
    use h5tags_core::script::{Form, read_script};
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let s = Session::open(Config::load(&config).unwrap()).unwrap();
    for b in s.builds() {
        let (mut text, mut compiled, mut not_utf8, mut strings) = (0, 0, 0, 0);
        for (code, field) in SCRIPT_FIELDS {
            let class = h5_module::FourCc::from_code(code).unwrap();
            for t in b.tags.iter().filter(|t| t.class == class) {
                for &copy in &t.copies {
                    let bytes = script_bytes(b, t, copy, field).unwrap();
                    let script = read_script(b, t, copy).unwrap_or_else(|e| panic!("{}: {e}", b.display_name(t)));
                    assert_eq!(script.size, bytes.len());
                    match script.form {
                        Form::Compiled { header, strings: found } => {
                            assert!(bytes.starts_with(BYTECODE_SIGNATURE));
                            assert_eq!(header, bytes[..16]);
                            assert_eq!(found, length_prefixed_strings(&bytes), "{}", b.display_name(t));
                            compiled += 1;
                            strings += found.len();
                        }
                        Form::Text { text: found, not_utf8: lossy } => {
                            let body = &bytes[..bytes.len() - bytes.iter().rev().take_while(|&&c| c == 0).count()];
                            assert_eq!(found, String::from_utf8_lossy(body));
                            assert_eq!(lossy, std::str::from_utf8(body).is_err());
                            text += 1;
                            not_utf8 += usize::from(lossy);
                        }
                    }
                }
            }
        }
        println!(
            "{}: text {text} (not UTF-8 {not_utf8}), compiled {compiled} ({strings} strings): all as the survey reads them",
            b.name
        );
    }
}

/// M7 step 4: the tag names the domain finds (`script::tag_names`) against
/// this survey's own count: path-like strings, naming one tag, several, and
/// the distinct names no indexed tag has.
#[test]
#[ignore]
fn the_domain_finds_the_tag_names_the_survey_counts() {
    use h5tags_core::script::{read_script, tag_names};
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let s = Session::open(Config::load(&config).unwrap()).unwrap();
    for b in s.builds() {
        let last_parts: HashMap<String, usize> = HashMap::new();
        for (code, field) in SCRIPT_FIELDS {
            let class = h5_module::FourCc::from_code(code).unwrap();
            let mut survey = Names::default();
            let (mut paths, mut one, mut several) = (0, 0, 0);
            let mut not_indexed: BTreeSet<String> = BTreeSet::new();
            let mut differences: Vec<String> = Vec::new();
            for t in b.tags.iter().filter(|t| t.class == class) {
                for &copy in &t.copies {
                    let bytes = script_bytes(b, t, copy, field).unwrap();
                    let before = survey.paths;
                    // The survey skips source that isn't UTF-8; the domain reads it lossily.
                    let mut survey_read = true;
                    if bytes.starts_with(BYTECODE_SIGNATURE) {
                        for r in length_prefixed_strings(&bytes) {
                            classify(b, &last_parts, &r, &mut survey);
                        }
                    } else if let Ok(src) =
                        std::str::from_utf8(&bytes[..bytes.len() - bytes.iter().rev().take_while(|&&c| c == 0).count()])
                    {
                        for lit in literals(src, &mut BTreeMap::new()) {
                            classify(b, &last_parts, &lit, &mut survey);
                        }
                    } else {
                        survey_read = false;
                    }
                    let script = read_script(b, t, copy).unwrap();
                    let names = tag_names(b, &script);
                    if survey_read {
                        assert_eq!(names.len(), survey.paths - before, "{}", b.display_name(t));
                    } else {
                        differences.push(format!("{} (not UTF-8): domain {}", b.display_name(t), names.len()));
                    }
                    for n in names {
                        paths += 1;
                        match n.tags.len() {
                            0 => {
                                not_indexed.insert(n.text.replace("\\\\", "\\").to_ascii_lowercase());
                            }
                            1 => one += 1,
                            _ => several += 1,
                        }
                    }
                }
            }
            println!(
                "{} {code}: domain path-like {paths}, one tag {one}, several {several}, not indexed {} distinct | survey {}, {}, {}, {}",
                b.name,
                not_indexed.len(),
                survey.paths,
                survey.one_tag,
                survey.several,
                survey.not_indexed.len()
            );
            for d in &differences {
                println!("  only the domain reads: {d}");
            }
        }
    }
}

#[test]
#[ignore]
fn what_the_script_tags_hold() {
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let s = Session::open(Config::load(&config).unwrap()).unwrap();
    // Default copy's script per (build, global ID), for the cross-build comparison.
    let mut defaults: Vec<HashMap<i32, Vec<u8>>> = Vec::new();
    for b in s.builds() {
        let mut last_parts: HashMap<String, usize> = HashMap::new();
        for t in &b.tags {
            *last_parts.entry(t.path_lower.rsplit('\\').next().unwrap_or_default().to_owned()).or_default() += 1;
        }
        let mut by_default = HashMap::new();
        for (code, field) in SCRIPT_FIELDS {
            let class = h5_module::FourCc::from_code(code).unwrap();
            let tags: Vec<&Tag> = b.tags.iter().filter(|t| t.class == class).collect();
            let (mut copies, mut errors, mut bytecode, mut text, mut empty) = (0, Vec::new(), 0, 0, 0);
            let (mut not_utf8, mut bom, mut crlf, mut lf_only, mut total, mut largest) = (0, 0, 0, 0, 0usize, 0usize);
            let mut headers: BTreeMap<String, usize> = BTreeMap::new();
            let mut trailing_nuls: BTreeMap<usize, usize> = BTreeMap::new();
            let mut escapes: BTreeMap<String, usize> = BTreeMap::new();
            let (mut text_names, mut code_names) = (Names::default(), Names::default());
            let (mut differ_in_layers, mut multi) = (0, 0);
            for t in &tags {
                let mut versions: HashSet<Vec<u8>> = HashSet::new();
                for &copy in &t.copies {
                    copies += 1;
                    let bytes = match script_bytes(b, t, copy, field) {
                        Ok(x) => x,
                        Err(e) => {
                            errors.push(format!("{}: {e}", b.display_name(t)));
                            continue;
                        }
                    };
                    total += bytes.len();
                    largest = largest.max(bytes.len());
                    if choose_copy(b, t, None) == Some(copy) {
                        by_default.insert(t.global_id.0, bytes.clone());
                    }
                    versions.insert(bytes.clone());
                    if bytes.is_empty() {
                        empty += 1;
                        continue;
                    }
                    if bytes.starts_with(BYTECODE_SIGNATURE) {
                        bytecode += 1;
                        let head: Vec<String> = bytes.iter().take(16).map(|c| format!("{c:02x}")).collect();
                        *headers.entry(head.join(" ")).or_default() += 1;
                        for r in length_prefixed_strings(&bytes) {
                            classify(b, &last_parts, &r, &mut code_names);
                        }
                        continue;
                    }
                    text += 1;
                    let nuls = bytes.iter().rev().take_while(|&&c| c == 0).count();
                    *trailing_nuls.entry(nuls).or_default() += 1;
                    let body = &bytes[..bytes.len() - nuls];
                    bom += usize::from(body.starts_with(b"\xEF\xBB\xBF"));
                    let Ok(src) = std::str::from_utf8(body) else {
                        not_utf8 += 1;
                        continue;
                    };
                    if src.contains("\r\n") {
                        crlf += 1
                    } else {
                        lf_only += 1
                    }
                    for lit in literals(src, &mut escapes) {
                        classify(b, &last_parts, &lit, &mut text_names);
                    }
                }
                if t.copies.len() > 1 {
                    multi += 1;
                    differ_in_layers += usize::from(versions.len() > 1);
                }
            }
            println!(
                "\n== {} {code} ({field}): {} tags, {copies} copies, {} unreadable",
                b.name,
                tags.len(),
                errors.len()
            );
            for e in errors.iter().take(5) {
                println!("  unreadable: {e}");
            }
            println!("  compiled {bytecode}, text {text}, empty {empty}; {total} bytes, largest {largest}");
            for (h, n) in &headers {
                println!("  compiled header {h}: {n}");
            }
            println!(
                "  text: not UTF-8 {not_utf8}, BOM {bom}, CRLF {crlf}, LF only {lf_only}; trailing NULs (count: copies) {trailing_nuls:?}"
            );
            println!("  escape-like pairs in literals: {escapes:?}");
            for (what, n) in [("text literals", &text_names), ("compiled strings", &code_names)] {
                println!(
                    "  {what}: path-like {}, one tag {}, several tags {}, not indexed {} distinct; chunk names (@…) {}; separator-free names equal to one tag's last path part {}, to several tags' {}; resolved by class {:?}",
                    n.paths,
                    n.one_tag,
                    n.several,
                    n.not_indexed.len(),
                    n.chunk_names,
                    n.name_only.0,
                    n.name_only.1,
                    n.resolved_classes
                );
                for x in n.not_indexed.iter().take(8) {
                    println!("    not indexed: {x}");
                }
            }
            println!("  tags with several copies {multi}, whose script differs between them {differ_in_layers}");
        }
        defaults.push(by_default);
    }
    if let [a, b, ..] = defaults.as_slice() {
        let shared: Vec<&i32> = a.keys().filter(|id| b.contains_key(id)).collect();
        let same = shared.iter().filter(|id| a[id] == b[id]).count();
        println!(
            "\n== between the first two builds: {} scripts in both, {same} the same, {} differ",
            shared.len(),
            shared.len() - same
        );
    }
}
