//! D31 survey for PRD Q12: tag references without a dependency entry. What do
//! they point to, and what would a header scan need to find them for
//! `find_references(in)`? Also times reading every header alone against
//! headers plus tag data (the two ways a scan could work). Ignored by default.
//!
//! ```text
//! H5_DEPLOY=<folder> cargo test --release -p h5tags-core --test real_reference_survey -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;
use std::time::Instant;

use h5_module::{Module, Section, map_file};
use h5tags_core::catalog::{TagLookupError, TagSpec};
use h5tags_core::stored::read_stored_tag;

#[derive(Default)]
struct Counts {
    refs: usize,
    null: usize,
    with_dependency: usize,
    /// Without a dependency entry and not null: the cases Q12 is about.
    without: usize,
    /// Of `without`: the field's global ID is an indexed tag…
    indexed: usize,
    /// …of the class the field states.
    indexed_class_matches: usize,
    /// …whose path is the listed name.
    name_is_path: usize,
    /// Of `without`: the listed name alone finds one tag / several / none.
    name_unique: usize,
    name_ambiguous: usize,
    name_unknown: usize,
    /// Of `without` with an indexed target: that target is a copy-free tag in another module only.
    target_elsewhere: usize,
}

#[test]
#[ignore]
fn references_without_a_dependency() {
    let (s, _cache) = common::real_session("core-reference-survey");
    let b = &s.builds()[0];
    let mut c = Counts::default();
    // (owner class, target class) → count, for references without a dependency.
    let mut by_class: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut examples: Vec<String> = Vec::new();
    let start = Instant::now();
    for tag in &b.tags {
        let copy = tag.copies[0];
        let Ok(stored) = read_stored_tag(b, copy) else { continue };
        let h = &stored.header;
        for r in &h.tag_references {
            c.refs += 1;
            let Some(field) = h.reference_field(&stored.tag_data, r) else { continue };
            if r.dependency.is_some() {
                c.with_dependency += 1;
                continue;
            }
            if field.is_null() {
                c.null += 1;
                continue;
            }
            c.without += 1;
            *by_class.entry((tag.class.to_string(), field.class_code.to_string())).or_default() += 1;
            let listed = h.string(r.name_offset).map(|n| n.into_owned()).unwrap_or_default();
            if let Some(target) = b.tag_by_id(field.global_id) {
                c.indexed += 1;
                c.indexed_class_matches += usize::from(target.class == field.class_code);
                c.name_is_path += usize::from(target.path.eq_ignore_ascii_case(&listed));
                c.target_elsewhere += usize::from(target.copies.iter().all(|t| t.module != copy.module));
            } else if examples.len() < 8 {
                examples.push(format!("{} {} → {} {listed} (not indexed)", tag.class, tag.path, field.global_id));
            }
            match b.find_tag(&TagSpec::Name(listed)) {
                Ok(_) => c.name_unique += 1,
                Err(TagLookupError::Ambiguous(_)) => c.name_ambiguous += 1,
                Err(TagLookupError::NotFound) => c.name_unknown += 1,
            }
        }
    }
    println!("{} tags read (header + tag data) in {:.2?}", b.tags.len(), start.elapsed());
    println!(
        "tag references {}: null {}, with a dependency {}, without (not null) {}",
        c.refs, c.null, c.with_dependency, c.without
    );
    println!(
        "  without: target indexed {} (class matches {}, listed name = its path {}); in other modules only {}",
        c.indexed, c.indexed_class_matches, c.name_is_path, c.target_elsewhere
    );
    println!(
        "  without, by listed name alone: one tag {}, several {}, none {}",
        c.name_unique, c.name_ambiguous, c.name_unknown
    );
    let mut classes: Vec<_> = by_class.into_iter().collect();
    classes.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!("  by owner class → target class (top 12):");
    for ((owner, target), n) in classes.iter().take(12) {
        println!("    {n:7}  {owner} → {target}");
    }
    for e in &examples {
        println!("  e.g. {e}");
    }

    // The cost of the two kinds of scan: every header alone, and header + tag data.
    let mut modules: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for tag in &b.tags {
        let copy = tag.copies[0];
        modules.entry(copy.module).or_default().push(copy.entry);
    }
    for (label, sections) in
        [("headers only", &[Section::Header][..]), ("headers + tag data", &[Section::Header, Section::TagData])]
    {
        let start = Instant::now();
        let mut bytes = 0usize;
        for (&m, entries) in &modules {
            let record = &b.index.modules[m as usize];
            let map = map_file(&b.module_file(record)).unwrap();
            let module = Module::parse(&map).unwrap();
            for &e in entries {
                for &s in sections {
                    bytes += module.read_section(e, s).map_or(0, |d| d.len());
                }
            }
        }
        println!("scan {label}: {bytes} bytes decompressed in {:.2?}", start.elapsed());
    }
}
