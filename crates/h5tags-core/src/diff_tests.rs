use h5_defs::definition::Definition;
use h5_meta::{parse_path, read_tag};
use h5_module::tag::TagHeader;
use h5_module::testutil::{TestTag, build_tag, sample_tag};

use super::*;

/// A real, a tag block of two-int entries (labelled by nothing), and an int.
const DEFINITION: &str = r#"<plugin>
	<_field_real name="speed" />
	<_field_block_64 name="barrels" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_long_integer name="rounds" />
		<_field_long_integer name="spare" />
	</_field_block_64>
	<_field_long_integer name="count" />
</plugin>"#;

/// `speed`, `count`, and the `rounds` of each barrel (one entry per value).
fn tag(speed: f32, count: i32, rounds: &[i32]) -> TestTag {
    let mut main = Vec::new();
    main.extend(speed.to_le_bytes());
    main.extend(vec![0u8; 0x1C]);
    main.extend(count.to_le_bytes());
    let mut block = Vec::new();
    for r in rounds {
        block.extend(r.to_le_bytes());
        block.extend(0i32.to_le_bytes());
    }
    let mut t = sample_tag();
    t.deps.clear();
    t.tag_refs.clear();
    t.data_refs.clear();
    t.blocks = vec![main, block];
    t.structs = vec![(0, 0, -1, 0, [1; 16], 0), (1, 1, 0, 0x04, [2; 16], rounds.len() as i32)];
    t
}

/// The differences under `path` (`""` = the whole tag), as (path, kind).
fn diffs_at(xml: &str, a: &TestTag, b: &TestTag, path: &str) -> Vec<(String, Kind)> {
    let def = Definition::parse(xml).unwrap();
    let (ha, da) = build_tag(a);
    let (hb, db) = build_tag(b);
    let (ha, hb) = (TagHeader::parse(&ha).unwrap(), TagHeader::parse(&hb).unwrap());
    let (ra, rb) = (read_tag(&def, &ha, &da), read_tag(&def, &hb, &db));
    assert_eq!((ra.divergences.len(), rb.divergences.len()), (0, 0));
    let at = if path.is_empty() { FieldPath::default() } else { parse_path(&def.fields, path).unwrap() };
    diff_at(&ra.root, &rb.root, &at).into_iter().map(|d| (d.path.display(&def.fields), d.kind)).collect()
}

fn diffs(a: &TestTag, b: &TestTag) -> Vec<(String, Kind)> {
    diffs_at(DEFINITION, a, b, "")
}

#[test]
fn identical_tags_have_no_differences() {
    let t = tag(10.0, 7, &[1, 2]);
    assert_eq!(diffs(&t, &t.clone()), []);
}

/// PRD §6.6: changed values by path; tag blocks by count, then entry by
/// entry, extra entries as only in one tag.
#[test]
fn values_counts_and_extra_entries() {
    let a = tag(10.0, 7, &[1, 2]);
    let b = tag(15.0, 7, &[1]);
    assert_eq!(
        diffs(&a, &b),
        [
            ("speed".into(), Kind::Changed),
            ("barrels".into(), Kind::Count { a: 2, b: 1 }),
            ("barrels[1]".into(), Kind::OnlyIn(Side::A)),
        ]
    );
    let c = tag(10.0, 7, &[1, 9]);
    assert_eq!(diffs(&a, &c), [("barrels[1].rounds".into(), Kind::Changed)]);
}

/// path= compares only that part, with the entry indices in the paths: a
/// block by count then entries, one entry, a block passed through (entry 0,
/// PRD §6.4.2), every entry, or one value.
#[test]
fn a_path_compares_that_part() {
    let a = tag(10.0, 7, &[1, 2]);
    let b = tag(15.0, 8, &[5]);
    let at = |path| diffs_at(DEFINITION, &a, &b, path);
    assert_eq!(
        at("barrels"),
        [
            ("barrels".into(), Kind::Count { a: 2, b: 1 }),
            ("barrels[0].rounds".into(), Kind::Changed),
            ("barrels[1]".into(), Kind::OnlyIn(Side::A)),
        ]
    );
    assert_eq!(at("barrels[1]"), [("barrels[1]".into(), Kind::OnlyIn(Side::A))]);
    assert_eq!(at("barrels.rounds"), [("barrels[0].rounds".into(), Kind::Changed)]);
    assert_eq!(
        at("barrels[*].rounds"),
        [("barrels[0].rounds".into(), Kind::Changed), ("barrels[1]".into(), Kind::OnlyIn(Side::A))]
    );
    assert_eq!(at("count"), [("count".into(), Kind::Changed)]);
}

/// section= compares only the fields under that heading (PRD §6.4.3).
#[test]
fn a_section_compares_its_fields() {
    let xml = DEFINITION.replace(
        "\t<_field_long_integer name=\"count\" />",
        "\t<_field_explanation name=\"AMMO\" />\n\t<_field_long_integer name=\"count\" />",
    );
    let def = Definition::parse(&xml).unwrap();
    let a = tag(10.0, 7, &[1]);
    let b = tag(15.0, 8, &[1]);
    let [sec] = h5_meta::find_sections(&def.fields, "ammo").try_into().unwrap();
    let (ha, da) = build_tag(&a);
    let (hb, db) = build_tag(&b);
    let (ha, hb) = (TagHeader::parse(&ha).unwrap(), TagHeader::parse(&hb).unwrap());
    let (ra, rb) = (read_tag(&def, &ha, &da), read_tag(&def, &hb, &db));
    let found: Vec<(String, Kind)> = diff_section(&ra.root, &rb.root, &sec.parent, sec.start + 1..sec.end)
        .into_iter()
        .map(|d| (d.path.display(&def.fields), d.kind))
        .collect();
    // speed differs too, but it isn't in the section.
    assert_eq!(found, [("count".into(), Kind::Changed)]);
}

/// D46: a curve is one value, compared by its fingerprint: one row at the curve.
#[test]
fn a_curve_is_one_value() {
    use h5_defs::definition::MAPPING_FUNCTION_GUID;
    let xml = format!(
        r#"<plugin>
	<_field_block_64 name="entries" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_tag_reference_64 name="target" />
	</_field_block_64>
	<_field_pad name="pad" length="4" />
	<_field_tag_reference_64 name="projectile" validtags="proj" />
	<_field_struct name="curve" srcguid="{MAPPING_FUNCTION_GUID}"><_field_data_64 name="data" /></_field_struct>
	<_field_long_integer name="count" />
</plugin>"#
    );
    let guid =
        Definition::parse(&xml).unwrap().fields.iter().find(|f| f.step.as_deref() == Some("curve")).unwrap().guid();
    let mut a = sample_tag();
    a.structs.push((3, -1, 0, 0x40, guid.unwrap(), 0));
    let mut b = a.clone();
    b.blocks[2] = vec![0xCD; 0x10];
    assert_eq!(diffs_at(&xml, &a, &a.clone(), ""), []);
    assert_eq!(diffs_at(&xml, &a, &b, ""), [("curve".into(), Kind::Changed)]);
}

/// `DEFINITION` with a string first in each barrel: it labels the entries (PRD §6.4.4).
const LABELLED: &str = r#"<plugin>
	<_field_real name="speed" />
	<_field_block_64 name="barrels" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_string name="name" />
		<_field_long_integer name="rounds" />
	</_field_block_64>
	<_field_long_integer name="count" />
</plugin>"#;

/// A tag whose barrels are `(name, rounds)`.
fn labelled(barrels: &[(&str, i32)]) -> TestTag {
    let mut t = tag(10.0, 7, &vec![0; barrels.len()]);
    t.blocks[1] = barrels
        .iter()
        .flat_map(|(name, rounds)| {
            let mut entry = [0u8; 0x24];
            entry[..name.len()].copy_from_slice(name.as_bytes());
            entry[0x20..].copy_from_slice(&rounds.to_le_bytes());
            entry
        })
        .collect();
    t
}

/// (path in a, path in b when it differs, kind) under `path`.
fn labelled_diffs(a: &TestTag, b: &TestTag, path: &str) -> Vec<(String, Option<String>, Kind)> {
    let def = Definition::parse(LABELLED).unwrap();
    let (ha, da) = build_tag(a);
    let (hb, db) = build_tag(b);
    let (ha, hb) = (TagHeader::parse(&ha).unwrap(), TagHeader::parse(&hb).unwrap());
    let (ra, rb) = (read_tag(&def, &ha, &da), read_tag(&def, &hb, &db));
    let at = if path.is_empty() { FieldPath::default() } else { parse_path(&def.fields, path).unwrap() };
    diff_at(&ra.root, &rb.root, &at)
        .into_iter()
        .map(|d| (d.path.display(&def.fields), d.in_b.map(|p| p.display(&def.fields)), d.kind))
        .collect()
}

/// D72: entries labelled in both tags (every one, no label twice in a block,
/// at least one label shared) are matched by label, so one extra entry in b
/// doesn't make every later entry differ. A matched entry's row has a's
/// index, and b's when it differs; an entry only b has, b's.
#[test]
fn labelled_entries_are_matched_by_label() {
    let a = labelled(&[("alpha", 1), ("beta", 2), ("gamma", 3)]);
    let b = labelled(&[("alpha", 1), ("extra", 9), ("beta", 2), ("gamma", 4)]);
    let s = |t: &str| t.to_owned();
    assert_eq!(
        labelled_diffs(&a, &b, ""),
        [
            (s("barrels"), None, Kind::Count { a: 3, b: 4 }),
            (s("barrels[2].rounds"), Some(s("barrels[3].rounds")), Kind::Changed),
            (s("barrels[1]"), None, Kind::OnlyIn(Side::B)),
        ]
    );
    // path= picks a's entry, and b's entry with its label.
    assert_eq!(
        labelled_diffs(&a, &b, "barrels[2]"),
        [(s("barrels[2].rounds"), Some(s("barrels[3].rounds")), Kind::Changed)]
    );
    assert_eq!(
        labelled_diffs(&a, &b, "barrels[*].rounds"),
        [
            (s("barrels[2].rounds"), Some(s("barrels[3].rounds")), Kind::Changed),
            (s("barrels[1]"), None, Kind::OnlyIn(Side::B)),
        ]
    );
}

/// By position otherwise: a label used twice, or no label in common.
#[test]
fn entries_without_usable_labels_are_matched_by_position() {
    let s = |t: &str| t.to_owned();
    let twice = labelled(&[("alpha", 1), ("alpha", 2)]);
    let other = labelled(&[("alpha", 1), ("beta", 2)]);
    assert_eq!(labelled_diffs(&twice, &other, ""), [(s("barrels[1].name"), None, Kind::Changed)]);
    let (one, two) = (labelled(&[("one", 1)]), labelled(&[("two", 1)]));
    assert_eq!(labelled_diffs(&one, &two, ""), [(s("barrels[0].name"), None, Kind::Changed)]);
}

/// NaN is the same as NaN (the stored bits), unlike `==` on floats.
#[test]
fn equal_bits_are_the_same_value() {
    let t = tag(f32::NAN, 7, &[]);
    assert_eq!(diffs(&t, &t.clone()), []);
}
