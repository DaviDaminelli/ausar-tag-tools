//! Every tag of a deploy folder rendered as `read_tag` shows it (text and
//! JSON), to catch any value the view can't present. Ignored by default;
//! reads `H5_DEPLOY` and `H5_DEFS`.
//!
//! ```text
//! H5_DEPLOY=<folder> cargo test --release -p h5tags-core --test real_read -- --ignored --nocapture
//! ```

mod common;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use h5_defs::definition::Definition;
use h5_meta::read_tag;
use h5_module::tag::TagHeader;
use h5_module::{EntryKind, FourCc, Module, Section, map_file};
use h5tags_core::catalog::Copy;
use h5tags_core::stored::load_definition;
use h5tags_core::tag_view::{BuildLookup, Detail, GuidCheck, Options, ViewSettings, view_body};
use h5tags_core::view::TagView;

#[test]
#[ignore]
fn every_tag_renders() {
    let (s, _cache) = common::real_session("core-real-read");
    let b = &s.builds()[0];
    let settings = ViewSettings {
        depth: 2,
        detail: Detail::Normal,
        options: Options::Auto,
        show_hidden: false,
        guid_check: GuidCheck::Warn,
    };
    let mut parsed: HashMap<FourCc, Option<Arc<Definition>>> = HashMap::new();
    let start = Instant::now();
    let (mut tags, mut lines, mut text_bytes, mut json_bytes) = (0usize, 0usize, 0usize, 0usize);
    let mut largest: Vec<(usize, String)> = Vec::new();
    // `!!` lines, by their text without the path → count.
    let mut failures: BTreeMap<String, usize> = BTreeMap::new();
    for (mi, record) in b.index.modules.iter().enumerate() {
        let map = map_file(&b.module_file(record)).unwrap();
        let m = Module::parse(&map).unwrap();
        for (i, e) in m.entries.iter().enumerate().filter(|(_, e)| e.kind() == EntryKind::Tag) {
            let copy = Copy { module: mi as u32, entry: i as u32 };
            let Some(tag) = b.tag_by_id(e.global_id) else { continue };
            let Some(def) = parsed.entry(e.class_code).or_insert_with(|| load_definition(b, tag).unwrap()).as_ref()
            else {
                continue;
            };
            let (Ok(h), Ok(data)) =
                (m.read_section(i as u32, Section::Header), m.read_section(i as u32, Section::TagData))
            else {
                continue;
            };
            let header = TagHeader::parse(&h).unwrap();
            let r = read_tag(def, &header, &data);
            let lookup = BuildLookup { build: b, copy };
            let nodes = view_body(&r.root, &def.fields, "", settings, &lookup);
            let v = TagView { title: format!("# {}", b.display_name(tag)), nodes, ..TagView::default() };
            let text = v.render_lines(false);
            let json = serde_json::to_string(&v.render_json()).unwrap();
            tags += 1;
            let n = text.lines().count();
            lines += n;
            text_bytes += text.len();
            json_bytes += json.len();
            largest.push((n, b.display_name(tag)));
            for l in text.lines().filter(|l| l.trim_start().starts_with("!!")) {
                let kind = l.trim_start().split(" at ").next().unwrap_or(l).to_owned();
                *failures.entry(kind).or_default() += 1;
            }
        }
    }
    largest.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    println!(
        "{tags} tags rendered in {:.2?}: {lines} text lines ({:.1} MB), JSON {:.1} MB; average {} lines",
        start.elapsed(),
        text_bytes as f64 / 1e6,
        json_bytes as f64 / 1e6,
        lines / tags.max(1)
    );
    println!("largest at depth 2: {:?}", &largest[..largest.len().min(5)]);
    println!("!! lines by kind: {failures:?}");
}

/// Every definition as get_definition (fully expanded, full detail) and outline show it.
#[test]
#[ignore]
fn every_definition_renders() {
    use h5tags_core::def_view::{definition_view, outline_definition};
    use h5tags_core::stored::load_definition_file;

    let defs = common::defs_dir();
    let settings = ViewSettings {
        depth: 99,
        detail: Detail::Full,
        options: Options::All,
        show_hidden: true,
        guid_check: GuidCheck::Warn,
    };
    let start = Instant::now();
    let (mut files, mut lines, mut outline_lines) = (0usize, 0usize, 0usize);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&defs).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    for p in &paths {
        let def = load_definition_file(p).unwrap();
        let v = TagView {
            title: "# d".into(),
            nodes: definition_view(&def.fields, "", settings).unwrap(),
            ..TagView::default()
        };
        lines += v.render_lines(false).lines().count();
        serde_json::to_string(&v.render_json()).unwrap();
        let o = TagView { title: "# d".into(), nodes: outline_definition(&def.fields, 3), ..TagView::default() };
        outline_lines += o.render_lines(false).lines().count();
        files += 1;
    }
    println!(
        "{files} definitions rendered in {:.2?}: {lines} lines fully expanded, outlines {outline_lines} lines",
        start.elapsed()
    );
}
