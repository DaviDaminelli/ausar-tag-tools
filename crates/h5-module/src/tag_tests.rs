use super::*;
use crate::checks;
use crate::module::{Module, Version};
use crate::murmur3::murmur3_x86_32;
use crate::testutil::{TestEntry, TestTag, build_module, build_tag, sample_tag};

fn parse(t: &TestTag) -> TagHeader {
    TagHeader::parse(&build_tag(t).0).unwrap()
}

/// Builds a module holding `t` as one tag entry, and returns what the checks need.
fn in_module(t: &TestTag) -> (Vec<u8>, TagHeader, Vec<u8>) {
    let (header, tag_data) = build_tag(t);
    let mut e = TestEntry::tag(r"objects\test\test.weapon", 0x10, "weap", &tag_data);
    e.sections.0 = header.clone();
    (build_module(Version::V27, &[e], &[]), TagHeader::parse(&header).unwrap(), tag_data)
}

fn failed(failures: &[CheckFailure]) -> Vec<&'static str> {
    failures.iter().map(|f| f.check).collect()
}

#[test]
fn parses_every_list() {
    let tag = parse(&sample_tag());
    assert_eq!(tag.fields.version, 27);
    assert_eq!(tag.dependencies.len(), 2);
    assert_eq!(tag.dependencies[0].class_code, FourCc::from_code("proj").unwrap());
    assert_eq!(tag.dependencies[1].global_id, GlobalId(0x0040_2000));
    assert_eq!(tag.string(tag.dependencies[0].name_offset).as_deref(), Some(r"objects\weapons\rifle\ar\bullet"));
    assert_eq!(tag.data_blocks.len(), 3);
    assert_eq!(tag.data_blocks[1].size, 0x40);
    assert_eq!(tag.data_blocks[1].offset, 0x60);
    assert_eq!(tag.data_blocks[1].section(), Some(Section::TagData));
    assert_eq!(tag.tag_structs.len(), 2);
    assert_eq!(tag.main_struct().map(|s| s.target), Some(Some(0)));
    assert_eq!(tag.tag_structs[1].kind, StructKind::TagBlock);
    assert_eq!(tag.tag_structs[1].guid, [2; 16]);
    assert_eq!((tag.tag_structs[1].field_block, tag.tag_structs[1].field_offset), (Some(0), 0));
    assert_eq!(tag.main_struct().unwrap().field_block, None);
    assert_eq!(
        tag.data_references,
        [DataReference {
            parent_struct: Some(0),
            unknown4: 0,
            target: Some(2),
            field_block: Some(0),
            field_offset: 0x40
        }]
    );
    assert_eq!(tag.tag_references.len(), 3);
    assert_eq!(tag.tag_references[1].dependency, None);
    assert_eq!(tag.tag_references[2].dependency, Some(1));
    let sids: Vec<_> = tag.string_ids.iter().map(|s| tag.string(s.string_offset).unwrap()).collect();
    assert_eq!(sids, ["__default__", "primary"]);
    assert_eq!(tag.zonesets.len(), 1);
    assert_eq!(tag.zonesets[0].name, murmur3_x86_32(b"__default__", 0));
    assert_eq!(tag.zonesets[0].tags.len(), 2);
    assert_eq!(tag.zonesets[0].tags[1].global_id, GlobalId(0x0040_2000));
    assert!(tag.trailing.is_empty());
}

#[test]
fn keeps_trailing_data() {
    let mut t = sample_tag();
    t.trailing = vec![0x15, 0, 0, 0, 1, 0, 0, 0];
    let tag = parse(&t);
    assert_eq!(tag.trailing, t.trailing);
    assert_eq!(tag.zonesets.len(), 1);
}

#[test]
fn rejects_what_is_not_a_tag_header() {
    let (h, _) = build_tag(&sample_tag());
    let mut bad = h.clone();
    bad[0] = b'x';
    assert!(matches!(TagHeader::parse(&bad), Err(TagError::BadMagic(_))));
    let mut bad = h.clone();
    bad[4] = 28;
    assert_eq!(TagHeader::parse(&bad), Err(TagError::UnsupportedVersion(28)));
    assert!(matches!(TagHeader::parse(&h[..0x40]), Err(TagError::Truncated { .. })));
    // The lists claim more than the header section holds.
    assert!(matches!(TagHeader::parse(&h[..h.len() - 20]), Err(TagError::Truncated { .. })));
}

#[test]
fn rejects_a_zoneset_list_outside_its_section() {
    let (mut h, _) = build_tag(&sample_tag());
    let tag = TagHeader::parse(&h).unwrap();
    let zoneset_start = h.len() - tag.trailing.len() - tag.fields.zoneset_size as usize;
    h[zoneset_start..zoneset_start + 8].copy_from_slice(&1000u64.to_le_bytes()); // 1000 zonesets
    assert!(matches!(TagHeader::parse(&h), Err(TagError::BadZoneset(_))));
}

#[test]
fn a_sound_tag_passes_every_check() {
    let (module, tag, tag_data) = in_module(&sample_tag());
    let m = Module::parse(&module).unwrap();
    assert!(tag.check(&m.entries[0]).is_empty(), "{:?}", tag.check(&m.entries[0]));
    assert!(tag.check_tag_data(&tag_data).is_empty(), "{:?}", tag.check_tag_data(&tag_data));
    // Trailing data is a fact, not a failure.
    let mut t = sample_tag();
    t.trailing = vec![7; 52];
    let (module, tag, _) = in_module(&t);
    assert!(tag.check(&Module::parse(&module).unwrap().entries[0]).is_empty());
}

#[test]
fn header_sizes_must_match_the_file_entry() {
    let (module, mut tag, _) = in_module(&sample_tag());
    tag.fields.data_size += 1;
    assert_eq!(failed(&tag.check(&Module::parse(&module).unwrap().entries[0])), [checks::TAG_SIZES]);
}

#[test]
fn list_indices_and_offsets_must_point_inside() {
    let check = |edit: &dyn Fn(&mut TestTag)| {
        let mut t = sample_tag();
        edit(&mut t);
        let (module, tag, _) = in_module(&t);
        failed(&tag.check(&Module::parse(&module).unwrap().entries[0]))
    };
    // Two main structs.
    assert_eq!(check(&|t| t.structs[1].0 = 0), [checks::MAIN_STRUCT]);
    // A tag block struct whose target block doesn't exist.
    assert_eq!(check(&|t| t.structs[1].1 = 9), [checks::TAG_STRUCT]);
    // A tag block field past the end of its block (0x1C bytes from 0x50 > 0x60).
    assert_eq!(check(&|t| t.structs[1].3 = 0x50), [checks::TAG_STRUCT]);
    // A data reference field past the end of its block.
    assert_eq!(check(&|t| t.data_refs[0].3 = 0x48), [checks::DATA_REFERENCE]);
}

#[test]
fn references_and_names_must_resolve() {
    let (module, mut tag, _) = in_module(&sample_tag());
    let entry = &Module::parse(&module).unwrap().entries[0];
    tag.tag_references[0].dependency = Some(5);
    tag.dependencies[1].name_offset = 9999;
    tag.string_ids[0].string_offset = 9999;
    let mut got = failed(&tag.check(entry));
    got.sort();
    assert_eq!(got, [checks::DEPENDENCY, checks::STRING_ID, checks::TAG_REFERENCE]);
}

#[test]
fn data_blocks_must_lie_in_their_section() {
    let (module, mut tag, _) = in_module(&sample_tag());
    let entry = &Module::parse(&module).unwrap().entries[0];
    tag.data_blocks[2].offset = 0x1000;
    tag.data_blocks[1].section = 7;
    let failures = tag.check(entry);
    assert_eq!(failed(&failures), [checks::DATA_BLOCK, checks::DATA_BLOCK]);
    assert_eq!(failures[0].item, Some(Item { kind: "data block", index: 1 }));
}

#[test]
fn tag_data_fields_must_agree_with_the_lists() {
    let (_, tag, mut tag_data) = in_module(&sample_tag());
    // The first tag reference field (block 0 at 0x20): wrong global ID, then wrong class.
    tag_data[0x20 + 0x0C] ^= 1;
    tag_data[0x20 + 0x18] ^= 1;
    // The data reference field (block 0 at 0x40): wrong size.
    tag_data[0x40 + 0x18] = 0x99;
    let mut got = failed(&tag.check_tag_data(&tag_data));
    got.sort();
    assert_eq!(got, [checks::DATA_REFERENCE_FIELD, checks::TAG_REFERENCE_FIELD, checks::TAG_REFERENCE_FIELD]);
}

#[test]
fn a_reference_without_a_dependency_may_still_name_a_tag() {
    // Seen in about 5% of real references (format-notes): the list has no
    // dependency, but the field holds a global ID and class. Not a failure.
    let (_, tag, mut tag_data) = in_module(&sample_tag());
    let null_ref = 0x60; // tag reference 1: block 1 at 0x00
    tag_data[null_ref + 0x0C..null_ref + 0x10].copy_from_slice(&0x0040_9999i32.to_le_bytes());
    assert!(tag.check_tag_data(&tag_data).is_empty(), "{:?}", tag.check_tag_data(&tag_data));
}

#[test]
fn a_reference_field_states_its_name_length() {
    let (_, tag, mut tag_data) = in_module(&sample_tag());
    tag_data[0x20 + 0x08] += 1; // tag reference 0's nameLength
    assert_eq!(failed(&tag.check_tag_data(&tag_data)), [checks::TAG_REFERENCE_FIELD]);
}

#[test]
fn strings_are_windows_1252() {
    let mut t = sample_tag();
    t.string_ids = vec!["fleeing_\u{2026}!!!!".into(), "caf\u{e9}".into()];
    // The builder writes UTF-8; real tables are Windows-1252. Re-encode the two strings.
    let (mut h, _) = build_tag(&t);
    let utf8 = |s: &str| s.as_bytes().to_vec();
    for (from, to) in
        [(utf8("fleeing_\u{2026}!!!!"), b"fleeing_\x85!!!!\0".to_vec()), (utf8("caf\u{e9}"), b"caf\xe9\0\0".to_vec())]
    {
        let at = h.windows(from.len()).position(|w| w == from.as_slice()).unwrap();
        h[at..at + to.len()].copy_from_slice(&to);
    }
    let tag = TagHeader::parse(&h).unwrap();
    assert_eq!(tag.string(tag.string_ids[0].string_offset).as_deref(), Some("fleeing_\u{2026}!!!!"));
    assert_eq!(tag.string(tag.string_ids[1].string_offset).as_deref(), Some("caf\u{e9}"));
    // A string_id's value hashes the stored bytes, not the decoded text.
    assert_eq!(tag.string_bytes(tag.string_ids[0].string_offset), Some(&b"fleeing_\x85!!!!"[..]));
}

#[test]
fn reads_what_a_reference_field_holds() {
    let (_, tag, tag_data) = in_module(&sample_tag());
    let field = |i: usize| tag.reference_field(&tag_data, &tag.tag_references[i]);
    let first = field(0).unwrap();
    assert_eq!(first.global_id, GlobalId(0x0040_1000));
    assert_eq!(first.class_code, FourCc::from_code("proj").unwrap());
    assert_eq!(first.name_length, r"objects\weapons\rifle\ar\bullet".len() as u32);
    assert!(!first.is_null());
    assert!(field(1).unwrap().is_null());
    // A field outside its block can't be read.
    let mut outside = tag.tag_references[0].clone();
    outside.field_offset = 0x50;
    assert_eq!(tag.reference_field(&tag_data, &outside), None);
}

/// `t` as a tag entry owning `resources` resource entries.
fn with_resources(t: &TestTag, resources: u32) -> (Vec<u8>, TagHeader) {
    let (header, tag_data) = build_tag(t);
    let name = r"objects\test\test.weapon";
    let mut e = TestEntry::tag(name, 0x10, "weap", &tag_data);
    e.sections.0 = header.clone();
    e.resources = (1..=resources).collect();
    let mut entries = vec![e];
    entries.extend((0..resources).map(|k| TestEntry::resource(&format!("{name}[{k}:mesh resource]"), 0, b"mesh")));
    (build_module(Version::V27, &entries, &[]), TagHeader::parse(&header).unwrap())
}

fn two_zonesets() -> TestTag {
    let mut t = sample_tag();
    let (default, other) = (murmur3_x86_32(b"__default__", 0), murmur3_x86_32(b"primary", 0));
    t.zonesets = vec![(default, vec![(0x0040_1000, default)], vec![0]), (other, vec![], vec![1])];
    t
}

#[test]
fn zonesets_list_the_resources_they_load() {
    let (module, tag) = with_resources(&two_zonesets(), 2);
    assert_eq!(tag.zonesets[0].resources, [0]);
    assert_eq!(tag.zonesets[1].resources, [1]);
    assert_eq!(tag.zonesets[0].tags.len(), 1);
    assert!(tag.check(&Module::parse(&module).unwrap().entries[0]).is_empty());
}

#[test]
fn zoneset_resources_must_be_the_entrys_resources() {
    let check = |t: &TestTag, resources| {
        let (module, tag) = with_resources(t, resources);
        failed(&tag.check(&Module::parse(&module).unwrap().entries[0]))
    };
    // Index 1 doesn't exist: the entry owns one resource.
    assert_eq!(check(&two_zonesets(), 1), [checks::ZONESET]);
    // Resource 2 is listed by no zoneset.
    assert_eq!(check(&two_zonesets(), 3), [checks::ZONESET]);
    // A zoneset section bigger than its contents.
    let (module, mut tag) = with_resources(&two_zonesets(), 2);
    tag.fields.zoneset_size += 4;
    let failures = tag.check(&Module::parse(&module).unwrap().entries[0]);
    assert!(failed(&failures).contains(&checks::ZONESET), "{failures:?}");
}

#[test]
fn rejects_a_resource_list_outside_the_zoneset_section() {
    let (mut h, _) = build_tag(&two_zonesets());
    let tag = TagHeader::parse(&h).unwrap();
    let zoneset_start = h.len() - tag.trailing.len() - tag.fields.zoneset_size as usize;
    let count_at = zoneset_start + 0x10 + 0x08; // zoneset 0's resource count
    h[count_at..count_at + 8].copy_from_slice(&1000u64.to_le_bytes());
    assert!(matches!(TagHeader::parse(&h), Err(TagError::BadZoneset(_))));
}

#[test]
fn string_id_names_come_from_the_tags_own_strings() {
    let tag = parse(&sample_tag());
    let names = tag.string_id_names();
    assert_eq!(names.get(murmur3_x86_32(b"__default__", 0)).as_deref(), Some("__default__"));
    assert_eq!(names.get(murmur3_x86_32(b"primary", 0)).as_deref(), Some("primary"));
    assert_eq!(names.get(0x1234_5678), None);
}
