use super::*;
use crate::field_type::{FieldType, Size};

/// A small definition like the real ones: the hidden header fields, a class
/// section, an enum, flags, a tag block, an array, padding and an explanation.
const SAMPLE: &str = r#"<!-- comment -->
<plugin>
	<_field_int64_integer name="vtable space*~!" />
	<_field_struct name="internal struct" srcguid="151d0146-479c-181f-3970-ec869043bc33">
		<_field_long_integer name="global tag ID!" />
		<_field_pad name="local tag handle!~" length="4" />
	</_field_struct>
	<_field_custom name="WEAPON" />
	<_field_explanation name="Damage Source">Damage &amp; more:
   - collision damage</_field_explanation>
	<_field_short_enum name="trigger mode">
		<item Name="normal" />
		<item Name="latch#hold to fire" />
	</_field_short_enum>
	<_field_long_flags name="flags">
		<item Name="a" />
	</_field_long_flags>
	<_field_block_64 name="barrels" max="2" srcguid="7ad1ffff-48ff-aaaa-d477-36cb3b0ee7ff" srcName="barrel_block">
		<_field_real name="rounds per second" />
		<_field_tag_reference_64 name="projectile" validtags="proj" />
	</_field_block_64>
	<_field_array name="pairs">
		<_field_short_integer name="a" />
		<_field_short_integer name="b" />
	</_field_array>
	<_field_skip name="skip!" length="12" />
</plugin>
"#;

fn sample() -> Definition {
    Definition::parse(SAMPLE).unwrap()
}

#[test]
fn reads_fields_in_order_with_their_types() {
    let d = sample();
    let types: Vec<FieldKind> = d.fields.iter().map(|f| f.kind.clone()).collect();
    use FieldType as T;
    assert_eq!(
        types,
        [
            T::Int64Integer,
            T::Struct,
            T::Custom,
            T::Explanation,
            T::ShortEnum,
            T::LongFlags,
            T::Block64,
            T::Array,
            T::Skip
        ]
        .map(FieldKind::Known)
    );
    assert_eq!(d.fields[1].children.len(), 2);
    assert_eq!(d.fields[6].children[1].name.display, "projectile");
}

#[test]
fn keeps_raw_names_attributes_options_and_text() {
    let d = sample();
    assert_eq!(d.fields[0].raw_name, "vtable space*~!");
    // The name is parsed (PRD §5.3).
    assert_eq!((d.fields[0].name.display.as_str(), d.fields[0].name.markers.as_str()), ("vtable space", "*~!"));
    // Enum and flag options are the `<item>` children, in order (`Name` attribute),
    // with the same name syntax.
    let options: Vec<(&str, Option<&str>)> =
        d.fields[4].options.iter().map(|o| (o.display.as_str(), o.help.as_deref())).collect();
    assert_eq!(options, [("normal", None), ("latch", Some("hold to fire"))]);
    let barrels = &d.fields[6];
    assert_eq!(barrels.attr("max"), Some("2"));
    assert_eq!(barrels.attr("SRCGUID"), Some("7ad1ffff-48ff-aaaa-d477-36cb3b0ee7ff"), "attributes ignore case");
    assert_eq!(barrels.children[1].attr("validtags"), Some("proj"));
    // Entities are decoded; line breaks are kept.
    assert_eq!(d.fields[3].text.as_deref(), Some("Damage & more:\n   - collision damage"));
    assert_eq!(d.fields[2].text, None);
}

#[test]
fn sizes_come_from_the_field_type_table() {
    let d = sample();
    let size = |i: usize| d.fields[i].size();
    assert_eq!(size(0), Some(8)); // int64
    assert_eq!(size(1), Some(8)); // struct: int32 + 4 bytes of pad
    assert_eq!(size(2), Some(0)); // custom: no space
    assert_eq!(size(3), Some(0)); // explanation: no space
    assert_eq!(size(6), Some(0x1C)); // a tag block field, not its entries
    assert_eq!(d.fields[6].entry_size(), Some(4 + 0x20)); // real + tag reference
    assert_eq!(size(7), Some(4)); // array: one instance of its children (Q7)
    assert_eq!(size(8), Some(12)); // skip: its length
    assert_eq!(d.size(), Some(8 + 8 + 2 + 4 + 0x1C + 4 + 12));
}

#[test]
fn an_unknown_type_has_no_size() {
    let d = Definition::parse(r#"<plugin><_field_real name="a"/><_field_mystery name="b"/></plugin>"#).unwrap();
    assert_eq!(d.fields[1].kind, FieldKind::Unknown("_field_mystery".into()));
    assert_eq!(d.fields[1].size(), None);
    assert_eq!(d.size(), None);
}

#[test]
fn padding_without_a_length_has_no_size() {
    let d = Definition::parse(r#"<plugin><_field_pad name="p"/></plugin>"#).unwrap();
    assert_eq!(d.fields[0].size(), None);
}

#[test]
fn rejects_what_is_not_a_definition() {
    assert!(Definition::parse("<other/>").is_err());
    assert!(Definition::parse("<plugin><_field_real name=\"a\">").is_err(), "unclosed");
}

#[test]
fn the_type_table_names_and_sizes() {
    // AusarDocs gives these sizes (CachedTag.md "Tag Data").
    assert_eq!(FieldType::Block64.size(), Size::Fixed(0x1C));
    assert_eq!(FieldType::Data64.size(), Size::Fixed(0x1C));
    assert_eq!(FieldType::TagReference64.size(), Size::Fixed(0x20));
    assert_eq!(FieldType::PageableResource64.size(), Size::Fixed(0x10));
    assert_eq!(FieldType::StringId.size(), Size::Fixed(4));
    assert_eq!(FieldType::Struct.size(), Size::Children);
    assert_eq!(FieldType::Pad.size(), Size::Length);
    // Every type's XML name maps back to it.
    for t in FieldType::ALL {
        assert_eq!(FieldType::from_xml(t.xml_name()), Some(*t), "{}", t.xml_name());
    }
    assert_eq!(FieldType::ALL.len(), 61);
}

#[test]
fn customs_are_class_sections_group_headings_or_widgets() {
    let d = Definition::parse(
        r#"<plugin>
        <_field_custom name="$$$ UNIT $$$" />
        <_field_custom name="Outlining" />
        <_field_char_enum name="outline color index" />
        <_field_custom name="cutoff frequency:Hz" />
        <_field_real name="cutoff frequency:Hz" />
        <_field_custom name="import - directory" />
        <_field_custom name="" />
        </plugin>"#,
    )
    .unwrap();
    let roles: Vec<Option<Custom>> = (0..d.fields.len()).map(|i| custom_role(&d.fields, i)).collect();
    assert_eq!(
        roles,
        [
            Some(Custom::ClassSection("UNIT".into())),
            Some(Custom::Group("Outlining".into())),
            None,                 // not a custom
            Some(Custom::Widget), // a control for the next field, same name
            None,
            Some(Custom::Widget), // a tool button
            Some(Custom::Widget), // nothing to show
        ]
    );
}

#[test]
fn sibling_names_are_made_unique() {
    let d = Definition::parse(
        r#"<plugin>
        <_field_real name="speed" />
        <_field_explanation name="speed" />
        <_field_real name="Speed:m/s" />
        <_field_real name="other" />
        <_field_real name="speed!" />
        </plugin>"#,
    )
    .unwrap();
    // Paths match names case-insensitively, so `Speed` repeats `speed`.
    // Explanations and customs aren't fields a path can name.
    assert_eq!(
        unique_names(&d.fields),
        [Some("speed".into()), None, Some("Speed#2".into()), Some("other".into()), Some("speed#3".into())]
    );
}

#[test]
fn common_structs_are_recognized() {
    let d = sample();
    assert!(d.fields[0].is_internal(), "vtable space");
    assert!(d.fields[1].is_internal(), "internal struct");
    assert!(!d.fields[6].is_internal());
    let f = Definition::parse(
        r#"<plugin><_field_struct name="curve" srcguid="5a14a98f-4484-53fa-d468-10809e45f10a"><_field_data_64 name="data" /></_field_struct></plugin>"#,
    )
    .unwrap();
    assert!(f.fields[0].is_mapping_function());
    assert!(!d.fields[1].is_mapping_function());
}

#[test]
fn srcguid_bytes_are_in_windows_order() {
    // format-notes "GUIDs": the first three groups are stored little-endian.
    let d = sample();
    assert_eq!(
        d.fields[1].guid(),
        Some([0x46, 0x01, 0x1d, 0x15, 0x9c, 0x47, 0x1f, 0x18, 0x39, 0x70, 0xec, 0x86, 0x90, 0x43, 0xbc, 0x33])
    );
    assert_eq!(d.fields[0].guid(), None, "no srcguid");
    let bad = Definition::parse(r#"<plugin><_field_struct name="s" srcguid="xyz"/></plugin>"#).unwrap();
    assert_eq!(bad.fields[0].guid(), None);
}

#[test]
fn each_field_knows_its_path_step() {
    // The unique names are computed once, when the definition is parsed.
    let d = Definition::parse(
        r#"<plugin>
        <_field_real name="speed" />
        <_field_explanation name="speed" />
        <_field_struct name="s"><_field_real name="x" /><_field_real name="X" /></_field_struct>
        <_field_real name="Speed" />
        </plugin>"#,
    )
    .unwrap();
    let steps: Vec<Option<&str>> = d.fields.iter().map(|f| f.step.as_deref()).collect();
    assert_eq!(steps, [Some("speed"), None, Some("s"), Some("Speed#2")]);
    let inner: Vec<Option<&str>> = d.fields[2].children.iter().map(|f| f.step.as_deref()).collect();
    assert_eq!(inner, [Some("x"), Some("X#2")]);
}
