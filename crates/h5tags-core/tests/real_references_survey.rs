//! Which tags of a class reference tags of another class, from the tag
//! headers alone (no definition): the dependency list, and the tag reference
//! fields at the locations the header lists. An independent check of what a
//! definition-based read (`query_field`, `read_tag`) says about reference
//! fields. Counts only. Ignored by default.
//!
//! ```text
//! H5_CONFIG=<config> H5_CLASS=weap H5_TARGETS=hsc*,proj \
//!   cargo test --release -p h5tags-core --test real_references_survey -- --ignored --nocapture
//! ```
//! Every build of the config; each tag's default copy (D58).

use std::path::PathBuf;

use h5_module::FourCc;
use h5tags_core::query::choose_copy;
use h5tags_core::stored::read_stored_tag;
use h5tags_core::{Config, Session};

/// Per target class: tags with a dependency on it, tags with a non-null
/// reference field to it, and those fields in all.
#[derive(Default)]
struct Found {
    tags_with_dependency: usize,
    tags_with_field: usize,
    fields: usize,
}

/// Does the header list empty tag blocks? (M5d step 2: whether a tag block
/// can be an anchor when it has no entries.) A listed entry with no data
/// block can only be an empty one; the block field's stored count says.
#[test]
#[ignore]
fn are_empty_tag_blocks_listed() {
    use h5_module::tag::StructKind;
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let s = Session::open(Config::load(&config).unwrap()).unwrap();
    for b in s.builds() {
        let (mut tags, mut listed, mut no_data, mut with_data, mut empty_with_data) = (0usize, 0, 0, 0, 0);
        let mut count_of_no_data: std::collections::BTreeMap<i32, usize> = Default::default();
        for tag in &b.tags {
            let Some(copy) = choose_copy(b, tag, None) else { continue };
            let Ok(stored) = read_stored_tag(b, copy) else { continue };
            tags += 1;
            let h = &stored.header;
            for st in h.tag_structs.iter().filter(|st| st.kind == StructKind::TagBlock) {
                listed += 1;
                // The block field's entry count (+0x10), where the header places the field.
                let count = st
                    .field_block
                    .and_then(|fb| h.block_data(&stored.tag_data, fb))
                    .and_then(|d| d.get(st.field_offset as usize + 0x10..st.field_offset as usize + 0x14))
                    .map(|c| i32::from_le_bytes(c.try_into().unwrap()));
                match st.target {
                    None => {
                        no_data += 1;
                        *count_of_no_data.entry(count.unwrap_or(-999)).or_default() += 1;
                    }
                    Some(_) => {
                        with_data += 1;
                        empty_with_data += usize::from(count == Some(0));
                    }
                }
            }
        }
        println!(
            "\n== {}: {tags} tags; tag block entries listed {listed}: with a data block {with_data} (count 0: {empty_with_data}), without {no_data}",
            b.name
        );
        println!("   counts stored in the fields of entries without a data block: {count_of_no_data:?}");
    }
}

#[test]
#[ignore]
fn references_to_a_class_from_the_headers() {
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let code = |c: &str| FourCc::from_code(c).unwrap_or_else(|| panic!("not a class code: {c}"));
    let class = code(&std::env::var("H5_CLASS").unwrap_or_else(|_| "weap".into()));
    let targets: Vec<FourCc> =
        std::env::var("H5_TARGETS").unwrap_or_else(|_| "hsc*,proj".into()).split(',').map(code).collect();
    let s = Session::open(Config::load(&config).unwrap()).unwrap();
    for b in s.builds() {
        let mut found: Vec<Found> = targets.iter().map(|_| Found::default()).collect();
        let (mut tags, mut unreadable, mut unreadable_fields) = (0usize, 0usize, 0usize);
        for tag in b.tags.iter().filter(|t| t.class == class) {
            let Some(copy) = choose_copy(b, tag, None) else { continue };
            let Ok(stored) = read_stored_tag(b, copy) else {
                unreadable += 1;
                continue;
            };
            tags += 1;
            let h = &stored.header;
            for (target, f) in targets.iter().zip(found.iter_mut()) {
                if h.dependencies.iter().any(|d| d.class_code == *target) {
                    f.tags_with_dependency += 1;
                }
                let mut fields = 0;
                for r in &h.tag_references {
                    match h.reference_field(&stored.tag_data, r) {
                        Some(rf) if !rf.is_null() && rf.class_code == *target => fields += 1,
                        Some(_) => {}
                        None => unreadable_fields += 1,
                    }
                }
                f.fields += fields;
                f.tags_with_field += usize::from(fields > 0);
            }
        }
        println!(
            "\n== {}: {class} tags {tags}, unreadable {unreadable}, reference fields not readable {}",
            b.name,
            unreadable_fields / targets.len().max(1)
        );
        for (target, f) in targets.iter().zip(&found) {
            println!(
                "  -> {target}: tags with a dependency {}, tags with a non-null field {}, fields {}",
                f.tags_with_dependency, f.tags_with_field, f.fields
            );
        }
    }
}
