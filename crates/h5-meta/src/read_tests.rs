use h5_defs::definition::Definition;
use h5_module::GlobalId;
use h5_module::tag::TagHeader;
use h5_module::testutil::{TestTag, build_tag, sample_tag};

use super::*;

/// The sample tag's layout (h5_module::testutil::sample_tag): the main struct
/// (block 0, 0x60 bytes) holds a tag block (2 entries in block 1, each a tag
/// reference), padding, a tag reference, a data reference (block 2) and an int.
const DEFINITION: &str = r#"<plugin>
	<_field_block_64 name="entries" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_tag_reference_64 name="target" />
	</_field_block_64>
	<_field_pad name="pad!" length="4" />
	<_field_tag_reference_64 name="projectile" validtags="proj" />
	<_field_data_64 name="data" />
	<_field_long_integer name="count" />
</plugin>"#;

fn tag() -> TestTag {
    let mut t = sample_tag();
    t.blocks[0][0x5C..0x60].copy_from_slice(&7i32.to_le_bytes());
    t
}

fn read(t: &TestTag, xml: &str) -> (Definition, TagHeader, Vec<u8>) {
    let (header, tag_data) = build_tag(t);
    (Definition::parse(xml).unwrap(), TagHeader::parse(&header).unwrap(), tag_data)
}

fn names<'a>(body: &'a Body<'_>) -> Vec<&'a str> {
    body.children.iter().filter_map(|n| n.name).collect()
}

fn child<'a, 'd>(body: &'a Body<'d>, name: &str) -> &'a Node<'d> {
    body.children.iter().find(|n| n.name == Some(name)).unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn reads_the_main_struct_and_its_tag_block() {
    let (def, header, data) = read(&tag(), DEFINITION);
    let r = read_tag(&def, &header, &data);
    assert_eq!(r.root.problem, None);
    assert_eq!(names(&r.root), ["entries", "pad", "projectile", "data", "count"]);
    let offsets: Vec<u32> = r.root.children.iter().map(|n| n.offset).collect();
    assert_eq!(offsets, [0x00, 0x1C, 0x20, 0x40, 0x5C]);

    let Content::Block(entries) = &child(&r.root, "entries").content else { panic!("not a block") };
    assert_eq!((entries.count, entries.entries.len(), entries.problem.clone()), (2, 2, None));
    let Content::Value(Value::TagRef(first)) = &child(&entries.entries[0], "target").content else { panic!() };
    assert!(first.field.is_null(), "entry 0 holds the null reference (tag reference 1)");
    let Content::Value(Value::TagRef(second)) = &child(&entries.entries[1], "target").content else { panic!() };
    assert_eq!(second.field.global_id, GlobalId(0x0040_2000));

    let Content::Value(Value::TagRef(p)) = &child(&r.root, "projectile").content else { panic!() };
    assert_eq!((p.field.global_id, p.dependency), (GlobalId(0x0040_1000), Some(0)));
    assert_eq!(p.listed_name.as_deref(), Some(r"objects\weapons\rifle\ar\bullet"));
    assert_eq!(child(&r.root, "data").content, Content::Value(Value::Data(DataRef { size: 0x10 })));
    assert_eq!(child(&r.root, "count").content, Content::Value(Value::Int(7)));
    assert_eq!(child(&r.root, "pad").content, Content::Value(Value::Padding));
}

/// PRD §9 check 1: the definition reaches every header list entry, and every
/// reference, block and data field it places is in the header's lists.
#[test]
fn a_matching_definition_has_no_divergences() {
    let (def, header, data) = read(&tag(), DEFINITION);
    assert_eq!(read_tag(&def, &header, &data).divergences, []);
}

#[test]
fn a_definition_missing_a_reference_diverges_both_ways() {
    // Same entry size (0x20), but no tag reference: the header's two tag
    // references in block 1 are reached by nothing.
    let xml = DEFINITION.replace(
        r#"<_field_tag_reference_64 name="target" />"#,
        r#"<_field_real name="a" /><_field_pad name="b" length="28" />"#,
    );
    let (def, header, data) = read(&tag(), &xml);
    let d = read_tag(&def, &header, &data).divergences;
    assert_eq!(
        d,
        [
            Divergence::Unreached { what: "tag reference", index: 1, block: 1, offset: 0x00 },
            Divergence::Unreached { what: "tag reference", index: 2, block: 1, offset: 0x20 },
        ]
    );
    // The other way: the header doesn't list the reference in entry 1.
    let mut t = tag();
    t.tag_refs.pop();
    let (def, header, data) = read(&t, DEFINITION);
    assert_eq!(
        read_tag(&def, &header, &data).divergences,
        [Divergence::NotInList { what: "tag reference", path: "entries[1].target".into() }]
    );
}

/// PRD §6.4.5: a block whose size doesn't fit is not read; the rest is.
#[test]
fn a_block_that_does_not_fit_is_not_read() {
    let xml = DEFINITION.replace(
        r#"<_field_tag_reference_64 name="target" />"#,
        r#"<_field_tag_reference_64 name="target" /><_field_real name="extra" />"#,
    );
    let (def, header, data) = read(&tag(), &xml);
    let r = read_tag(&def, &header, &data);
    let Content::Block(b) = &child(&r.root, "entries").content else { panic!() };
    assert_eq!(b.problem, Some(Problem::SizeMismatch { definition: 2 * 0x24, tag: 0x40 }));
    assert!(b.entries.is_empty());
    assert!(matches!(child(&r.root, "projectile").content, Content::Value(Value::TagRef(_))));
    // Entries inside a block that wasn't read aren't divergences.
    assert_eq!(r.divergences, []);
}

#[test]
fn a_main_struct_that_does_not_fit_is_not_read() {
    let xml = DEFINITION.replace(r#"<_field_long_integer name="count" />"#, "");
    let (def, header, data) = read(&tag(), &xml);
    let r = read_tag(&def, &header, &data);
    assert_eq!(r.root.problem, Some(Problem::SizeMismatch { definition: 0x5C, tag: 0x60 }));
    assert!(r.root.children.is_empty());
}

/// PRD §6.4.4: an unknown field type stops its struct; what came before stays.
#[test]
fn an_unknown_type_stops_its_struct() {
    let xml = DEFINITION.replace(r#"<_field_data_64 name="data" />"#, r#"<_field_mystery name="data" />"#);
    let (def, header, data) = read(&tag(), &xml);
    let r = read_tag(&def, &header, &data);
    assert_eq!(names(&r.root), ["entries", "pad", "projectile"]);
    assert_eq!(r.root.problem, Some(Problem::UnknownType { at: "data".into(), type_name: "_field_mystery".into() }));
}

#[test]
fn a_guid_mismatch_warns_but_keeps_the_values() {
    let mut t = tag();
    t.structs[1].4 = [3; 16];
    let (def, header, data) = read(&t, DEFINITION);
    let r = read_tag(&def, &header, &data);
    let Content::Block(b) = &child(&r.root, "entries").content else { panic!() };
    // The tag's GUID is kept, for the definition gap list (decisions D33).
    assert_eq!(b.guid_mismatch, Some([3; 16]));
    assert_eq!(b.entries.len(), 2);
}

#[test]
fn repeated_sibling_names_get_numbers() {
    let xml = DEFINITION.replace(r#"name="count""#, r#"name="data""#);
    let (def, header, data) = read(&tag(), &xml);
    assert_eq!(names(&read_tag(&def, &header, &data).root), ["entries", "pad", "projectile", "data", "data#2"]);
}
