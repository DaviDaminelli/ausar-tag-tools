use h5_defs::definition::Definition;
use h5_module::tag::TagHeader;
use h5_module::testutil::{TestTag, build_tag, sample_tag};

use super::*;
use crate::read_tag;

/// Names as the real definitions write them: a dot and brackets inside
/// names, repeated names, nesting.
const DEFINITION: &str = r#"<plugin>
	<_field_custom name="WEAPON" />
	<_field_explanation name="Firing">How it fires.</_field_explanation>
	<_field_block_64 name="barrels" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_real name="rounds per second" />
		<_field_real name="Res. Area" />
		<_field_real name="peg threshold [0,1]" />
		<_field_real name="peg threshold" />
	</_field_block_64>
	<_field_struct name="physics">
		<_field_real name="mass" />
		<_field_custom name="Damping" />
		<_field_real name="speed" />
		<_field_real name="Speed" />
	</_field_struct>
	<_field_custom name="ZOOM" />
	<_field_real name="magnification" />
</plugin>"#;

fn def() -> Definition {
    Definition::parse(DEFINITION).unwrap()
}

fn text(d: &Definition, path: &str) -> String {
    parse_path(&d.fields, path).unwrap().display(&d.fields)
}

#[test]
fn steps_match_names_case_insensitively() {
    let d = def();
    assert_eq!(text(&d, "barrels"), "barrels");
    assert_eq!(text(&d, "BARRELS[1].Rounds Per Second"), "barrels[1].rounds per second");
    assert_eq!(text(&d, "barrels[*].rounds per second"), "barrels[*].rounds per second");
    assert_eq!(text(&d, "physics.mass"), "physics.mass");
    // Repeated names: the second is `name#2` (PRD §6.4.2).
    assert_eq!(text(&d, "physics.speed#2"), "physics.Speed#2");
}

#[test]
fn names_may_hold_dots_and_brackets() {
    let d = def();
    assert_eq!(text(&d, "barrels[0].Res. Area"), "barrels[0].Res. Area");
    // The longest name that fits wins: `peg threshold [0,1]` is a name, not an index.
    assert_eq!(text(&d, "barrels[0].peg threshold [0,1]"), "barrels[0].peg threshold [0,1]");
    assert_eq!(text(&d, "barrels[0].peg threshold"), "barrels[0].peg threshold");
}

#[test]
fn a_wrong_name_lists_the_closest_ones() {
    let d = def();
    let err = parse_path(&d.fields, "physics.mas").unwrap_err();
    let PathError::NoSuchField { at, name, similar } = err else { panic!("{err:?}") };
    assert_eq!((at.as_str(), name.as_str()), ("physics", "mas"));
    assert_eq!(similar.first().map(String::as_str), Some("mass"));
    assert!(similar.len() <= 5);
    // Headings aren't fields.
    assert!(matches!(parse_path(&d.fields, "Firing"), Err(PathError::NoSuchField { .. })));
}

#[test]
fn indices_only_on_tag_blocks_and_values_have_no_fields() {
    let d = def();
    assert_eq!(parse_path(&d.fields, "physics[0]"), Err(PathError::NotABlock { at: "physics".into() }));
    assert_eq!(parse_path(&d.fields, "magnification.x"), Err(PathError::NoFields { at: "magnification".into() }));
    assert_eq!(parse_path(&d.fields, ""), Ok(FieldPath::default()), "the whole tag");
}

/// A tag for `DEFINITION`: main struct 0x2C bytes (block field 0x1C, physics
/// 0x0C, magnification 4), barrels block with 2 entries of 16 bytes.
fn tag() -> TestTag {
    let mut main = vec![0u8; 0x2C];
    main[0x28..0x2C].copy_from_slice(&4.0f32.to_le_bytes());
    let mut barrels = vec![0u8; 0x20];
    barrels[0x10..0x14].copy_from_slice(&9.0f32.to_le_bytes()); // barrels[1].rounds per second
    let mut t = sample_tag();
    t.deps.clear();
    t.tag_refs.clear();
    t.data_refs.clear();
    t.blocks = vec![main, barrels];
    t.structs = vec![(0, 0, -1, 0, [1; 16], 0), (1, 1, 0, 0x00, [2; 16], 2)];
    t
}

fn selected(path: &str) -> Result<Vec<(String, Option<u32>)>, PathError> {
    let d = def();
    let (h, data) = build_tag(&tag());
    let header = TagHeader::parse(&h).unwrap();
    let r = read_tag(&d, &header, &data);
    assert_eq!(r.divergences, []);
    let p = parse_path(&d.fields, path)?;
    Ok(select(&r.root, &p)?.into_iter().map(|s| (s.path, s.entry)).collect())
}

#[test]
fn selecting_fields_of_a_read_tag() {
    assert_eq!(selected("magnification").unwrap(), [("magnification".into(), None)]);
    assert_eq!(selected("barrels[1].rounds per second").unwrap(), [("barrels[1].rounds per second".into(), None)]);
    // Every entry.
    assert_eq!(
        selected("barrels[*].rounds per second").unwrap(),
        [("barrels[0].rounds per second".into(), None), ("barrels[1].rounds per second".into(), None)]
    );
    // A tag block passed through without an index: entry 0.
    assert!(matches!(selected("barrels.mass"), Err(PathError::NoSuchField { .. })));
    assert_eq!(selected("barrels.rounds per second").unwrap(), [("barrels[0].rounds per second".into(), None)]);
    // A path ending at a block entry selects that entry.
    assert_eq!(selected("barrels[1]").unwrap(), [("barrels[1]".into(), Some(1))]);
    assert_eq!(selected("barrels[*]").unwrap(), [("barrels[0]".into(), Some(0)), ("barrels[1]".into(), Some(1))]);
    assert_eq!(selected("barrels[5]"), Err(PathError::OutOfRange { at: "barrels".into(), index: 5, count: 2 }));
}

#[test]
fn sections_by_heading() {
    let d = def();
    // A class section runs to the next class section.
    let s = find_sections(&d.fields, "weapon");
    assert_eq!(s.len(), 1);
    assert_eq!((s[0].heading.as_str(), s[0].start, s[0].end, s[0].kind), ("WEAPON", 0, 4, SectionKind::Class));
    // An explanation runs to the next heading of the same or a higher rank;
    // structs and blocks inside it belong to it.
    let s = find_sections(&d.fields, "FIRING");
    assert_eq!((s[0].start, s[0].end, s[0].kind), (1, 4, SectionKind::Explanation));
    // A group heading inside a struct: its parent path says where.
    let s = find_sections(&d.fields, "damping");
    assert_eq!(s[0].parent.display(&d.fields), "physics");
    assert_eq!((s[0].start, s[0].end, s[0].kind), (1, 4, SectionKind::Group));
    assert!(find_sections(&d.fields, "nothing").is_empty());
}
