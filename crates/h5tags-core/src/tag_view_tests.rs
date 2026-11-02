use h5_defs::definition::Definition;
use h5_meta::read_tag;
use h5_module::murmur3::murmur3_x86_32;
use h5_module::tag::TagHeader;
use h5_module::testutil::{TestTag, build_tag, sample_tag};

use super::*;
use crate::view::TagView;

/// Everything slice 2b resolves (main struct 0x40, a 2-entry block, a resource).
const VALUES: &str = r#"<plugin>
	<_field_custom name="WEAPON" />
	<_field_short_enum name="trigger mode"><item Name="normal" /><item Name="latch#hold" /></_field_short_enum>
	<_field_short_enum name="odd"><item Name="a" /><item Name="b" /></_field_short_enum>
	<_field_long_flags name="flags"><item Name="first bit" /><item Name="second bit" /><item Name="third bit" /></_field_long_flags>
	<_field_string_id name="primary name" />
	<_field_string_id name="unknown name" />
	<_field_short_block_index name="selected" targetGuid="02020202-0202-0202-0202-020202020202" />
	<_field_pad name="p" length="2" />
	<_field_block_64 name="entries" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_string_id name="name^" />
		<_field_long_integer name="value:rounds" />
	</_field_block_64>
	<_field_pageable_resource_64 name="mesh" />
</plugin>"#;

fn values_tag() -> TestTag {
    let sid = |s: &str| murmur3_x86_32(s.as_bytes(), 0).to_le_bytes();
    let mut main = vec![0u8; 0x40];
    main[0x00..0x02].copy_from_slice(&1i16.to_le_bytes());
    main[0x02..0x04].copy_from_slice(&5i16.to_le_bytes());
    main[0x04..0x08].copy_from_slice(&0b1001u32.to_le_bytes()); // bit 0 (named) and bit 3 (unnamed)
    main[0x08..0x0C].copy_from_slice(&sid("primary"));
    main[0x0C..0x10].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    main[0x10..0x12].copy_from_slice(&1i16.to_le_bytes());
    let mut entries = vec![0u8; 0x10];
    entries[0x00..0x04].copy_from_slice(&sid("first"));
    entries[0x04..0x08].copy_from_slice(&7i32.to_le_bytes());
    entries[0x08..0x0C].copy_from_slice(&sid("second"));
    let mut t = sample_tag();
    t.deps.clear();
    t.tag_refs.clear();
    t.data_refs.clear();
    t.blocks = vec![main, entries];
    t.structs = vec![(0, 0, -1, 0, [1; 16], 0), (1, 1, 0, 0x14, [2; 16], 2), (2, -1, 0, 0x30, [4; 16], 0)];
    t.string_ids = ["__default__", "primary", "first", "second"].map(String::from).to_vec();
    t
}

/// Angle-typed fields: stored in radians, labelled in degrees (format-notes "Angle units").
const ANGLES: &str = r#"<plugin>
	<_field_angle name="steering maximum" />
	<_field_angle_bounds name="error angle:degrees (max_error_angle)" />
	<_field_real_euler_angles_2d name="angle offset" />
	<_field_angle name="kick deceleration:degrees/s/s" />
</plugin>"#;

fn angles_tag() -> TestTag {
    let mut main = Vec::new();
    for rad in [10f32, 0.15, 2.3, -60.0, 90.0, 1350.0] {
        main.extend((rad.to_radians()).to_le_bytes());
    }
    let mut t = sample_tag();
    t.deps.clear();
    t.tag_refs.clear();
    t.data_refs.clear();
    t.blocks = vec![main];
    t.structs = vec![(0, 0, -1, 0, [1; 16], 0)];
    t.string_ids = vec!["__default__".into()];
    t
}

/// D44: the stored value stays exact, marked `rad`; the definition's unit
/// follows with the value converted to degrees (3 decimals). JSON adds `degrees`.
#[test]
fn angles_show_radians_and_degrees() {
    let (text, json) = render(&angles_tag(), ANGLES, NORMAL);
    let l = lines(&text);
    assert!(l.contains(&"steering maximum  real  0.17453292 rad  10 degrees"), "{text}");
    assert!(
        l.contains(&"error angle  bounds  0.0026179939..0.040142573 rad  0.15..2.3 degrees (max_error_angle)"),
        "{text}"
    );
    assert!(l.contains(&"angle offset  euler2  (-1.0471976, 1.5707964) rad  (-60, 90) degrees"), "{text}");
    assert!(l.contains(&"kick deceleration  real  23.561945 rad  1350 degrees/s/s"), "{text}");
    let field = |n: &str| json["fields"].as_array().unwrap().iter().find(|f| f["name"] == n).unwrap().clone();
    assert_eq!(field("steering maximum")["degrees"], 10.0);
    assert_eq!(field("error angle")["degrees"], serde_json::json!({ "min": 0.15, "max": 2.3 }));
    assert_eq!(field("angle offset")["degrees"], serde_json::json!([-60.0, 90.0]));
    // Brief keeps the stored value and its unit mark.
    let (text, _) = render(&angles_tag(), ANGLES, ViewSettings { detail: Detail::Brief, ..NORMAL });
    assert!(text.contains("\nsteering maximum  0.17453292 rad\n"), "{text}");
}

/// The sample tag's layout: a block of tag references, padding, a tag reference, data, an int.
const REFS: &str = r#"<plugin>
	<_field_block_64 name="entries" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_tag_reference_64 name="target" />
	</_field_block_64>
	<_field_pad name="pad" length="4" />
	<_field_tag_reference_64 name="projectile" validtags="proj" />
	<_field_data_64 name="data" />
	<_field_long_integer name="count" />
</plugin>"#;

struct Stub;

impl Lookup for Stub {
    fn tag_name(&self, id: GlobalId) -> Option<String> {
        (id == GlobalId(0x0040_1000)).then(|| r"objects\weapons\rifle\ar\bullet.projectile".into())
    }
    fn class_name(&self, code: FourCc) -> String {
        if code == FourCc::from_code("effe").unwrap() { "effect".into() } else { code.to_string() }
    }
    fn resource(&self, k: u32) -> Option<ResourceInfo> {
        (k == 0).then(|| ResourceInfo { kind: "mesh resource".into(), size: 6_100_000, stored: true })
    }
}

const NORMAL: ViewSettings = ViewSettings {
    depth: 2,
    detail: Detail::Normal,
    options: Options::Auto,
    show_hidden: false,
    guid_check: GuidCheck::Warn,
};

fn render(t: &TestTag, xml: &str, settings: ViewSettings) -> (String, serde_json::Value) {
    let def = Definition::parse(xml).unwrap();
    let (h, data) = build_tag(t);
    let header = TagHeader::parse(&h).unwrap();
    let r = read_tag(&def, &header, &data);
    assert_eq!(r.divergences, []);
    let nodes = view_body(&r.root, &def.fields, "", settings, &Stub);
    let v = TagView { title: "# test".into(), nodes, brief: settings.detail == Detail::Brief, ..TagView::default() };
    (v.render_lines(false), v.render_json())
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

#[test]
fn values_as_the_guide_shows_them() {
    let (text, _) = render(&values_tag(), VALUES, NORMAL);
    let l = lines(&text);
    assert_eq!(l[1], "## WEAPON");
    assert!(l.contains(&"trigger mode  enum16  1 latch  {normal|latch}"), "{text}");
    assert!(l.contains(&"odd  enum16  5 (no option 5; 2 defined)  {a|b}"), "{text}");
    assert!(l.contains(&"flags  flags32  0x00000009 {first bit} unnamed bits: 3  (+2 options)"), "{text}");
    assert!(l.contains(&"primary name  stringid  primary"), "{text}");
    assert!(l.contains(&"unknown name  stringid  0x12345678 (unresolved)"), "{text}");
    assert!(l.contains(&"selected  blkidx  1 → entries[1] second"), "{text}");
    assert!(l.contains(&"### entries  block[2]  {first, second}  showing [0] first"), "{text}");
    assert!(l.contains(&"  value  int32  7  rounds"), "{text}");
    assert!(l.contains(&"mesh  resource  (mesh resource, 6.1 MB, not loaded)"), "{text}");
    // Padding is hidden by default.
    assert!(!text.contains("\np  "), "{text}");
}

/// PRD §6.4.5: `guid_check` off, warn (default) or enforce, on a tag block
/// whose stored struct GUID isn't the definition's `srcguid`.
#[test]
fn guid_check_off_warn_and_enforce() {
    let xml = VALUES.replace(
        r#"<_field_block_64 name="entries" srcguid="02020202-0202-0202-0202-020202020202">"#,
        r#"<_field_block_64 name="entries" srcguid="03030303-0303-0303-0303-030303030303">"#,
    );
    let with = |guid_check| render(&values_tag(), &xml, ViewSettings { guid_check, ..NORMAL }).0;
    let warn = with(GuidCheck::Warn);
    assert!(
        warn.contains(
            "\n### entries  block[2]  {first, second}  showing [0] first  warning: definition GUID mismatch\n"
        ),
        "{warn}"
    );
    assert!(warn.contains("\n  value  int32  7  rounds\n"), "{warn}");
    let off = with(GuidCheck::Off);
    assert!(off.contains("\n### entries  block[2]  {first, second}  showing [0] first\n"), "{off}");
    let enforce = with(GuidCheck::Enforce);
    assert!(
        enforce.contains("\n### entries  block[2]  !! definition GUID mismatch at entries (definition 03030303-0303-0303-0303-030303030303, tag 02020202-0202-0202-0202-020202020202; guid_check = enforce)\n"),
        "{enforce}"
    );
    assert!(!enforce.contains("value  int32"), "no values: {enforce}");
}

/// Labels that only repeat entry indices say nothing (decisions D40); JSON
/// keeps them.
#[test]
fn labels_that_are_only_indices_are_left_out() {
    // An entry with no string_id, string or tag reference has no label.
    let xml = VALUES.replace(r#"<_field_string_id name="name^" />"#, r#"<_field_long_integer name="name" />"#);
    let (text, json) = render(&values_tag(), &xml, NORMAL);
    let l = lines(&text);
    assert!(l.contains(&"### entries  block[2]  showing [0]"), "{text}");
    assert!(l.contains(&"selected  blkidx  1 → entries[1]"), "{text}");
    let entries = json["fields"].as_array().unwrap().iter().find(|f| f["name"] == "entries").unwrap();
    assert_eq!(entries["labels"], serde_json::json!(["0", "1"]));

    let def = Definition::parse(&xml).unwrap();
    let (h, data) = build_tag(&values_tag());
    let r = read_tag(&def, &TagHeader::parse(&h).unwrap(), &data);
    let outline = TagView { nodes: crate::def_view::outline_tag(&r.root, &def.fields, 2), ..TagView::default() };
    let outline = outline.render_lines(false);
    assert!(outline.contains("### entries  block[2]  (2 fields)  path=entries\n"), "{outline}");
}

#[test]
fn references_null_indexed_and_not_indexed() {
    let (text, json) = render(&sample_tag(), REFS, NORMAL);
    let l = lines(&text);
    assert!(
        l.contains(&r"projectile  tagref  objects\weapons\rifle\ar\bullet.projectile  0x00401000 [proj]"),
        "{text}"
    );
    assert!(l.contains(&"data  data  (16 bytes)"), "{text}");
    // Entry 0 of `entries` holds the null reference; entry 1 one to a tag
    // that isn't indexed: named from the tag's own string table.
    assert!(l.contains(&"  target  tagref  (none) []"), "{text}");
    let fields = json["fields"].as_array().unwrap();
    let projectile = fields.iter().find(|f| f["name"] == "projectile").unwrap();
    assert_eq!(projectile["target"]["id"], "0x00401000");
    assert_eq!(projectile["target"]["valid_classes"], serde_json::json!(["proj"]));
}

#[test]
fn a_reference_to_a_tag_that_is_not_indexed() {
    let settings = ViewSettings { depth: 2, ..NORMAL };
    let def = Definition::parse(REFS).unwrap();
    let (h, data) = build_tag(&sample_tag());
    let header = TagHeader::parse(&h).unwrap();
    let r = read_tag(&def, &header, &data);
    let h5_meta::Content::Block(b) = &r.root.children[0].content else { panic!() };
    let entry1 = view_body(&b.entries[1], &def.fields[0].children, "entries[1]", settings, &Stub);
    let v = TagView { title: "# t".into(), nodes: entry1, ..TagView::default() };
    let text = v.render_lines(false);
    assert!(
        text.contains(r"target  tagref  fx\impact\bullet_hit.effect  0x00402000 [] (not in indexed modules)"),
        "{text}"
    );
}

#[test]
fn depth_zero_leaves_blocks_as_stubs() {
    let (text, _) = render(&values_tag(), VALUES, ViewSettings { depth: 0, ..NORMAL });
    assert!(text.contains("### entries  block[2]  {first, second}  → path=entries"), "{text}");
    assert!(!text.contains("showing"), "{text}");
}

#[test]
fn hidden_fields_show_on_request() {
    let (text, _) = render(&values_tag(), VALUES, ViewSettings { show_hidden: true, ..NORMAL });
    assert!(text.contains("\np  pad  (2 bytes)  (hidden)\n"), "{text}");
}

#[test]
fn json_carries_the_same_facts() {
    let (_, json) = render(&values_tag(), VALUES, NORMAL);
    let f = json["fields"].as_array().unwrap();
    let by = |name: &str| f.iter().find(|x| x["name"] == name).unwrap().clone();
    assert_eq!(by("trigger mode")["label"], "latch");
    assert_eq!(by("odd")["state"], "no_option");
    assert_eq!(by("flags")["unnamed_bits"], serde_json::json!([3]));
    assert_eq!(by("unknown name")["state"], "unresolved");
    assert_eq!(by("selected")["target"]["path"], "entries[1]");
    assert_eq!(by("entries")["labels"], serde_json::json!(["first", "second"]));
    assert_eq!(by("mesh")["state"], "not_loaded");
}

#[test]
fn brief_and_full() {
    let (text, _) = render(&values_tag(), VALUES, ViewSettings { detail: Detail::Brief, ..NORMAL });
    assert!(text.contains("\ntrigger mode  1 latch\n"), "{text}");
    assert!(text.contains("\nin entries[0]:\n  name  first\n  value  7\n"), "{text}");
    assert!(!text.contains("##"), "{text}");
    let (text, _) = render(&values_tag(), VALUES, ViewSettings { detail: Detail::Full, ..NORMAL });
    assert!(text.contains("trigger mode  enum16  1 latch  {normal|latch}  @0x0\n"), "{text}");
    assert!(text.contains("  name  stringid  first  markers ^  @0x0\n"), "{text}");
}

#[test]
fn an_empty_explanation_naming_the_next_field_is_not_repeated() {
    // 103 real explanations have no text and the next field's name (`firing`
    // before the `firing` struct): the field's own heading says it.
    let xml = VALUES.replace(
        r#"<_field_block_64 name="entries""#,
        r#"<_field_explanation name="Entries" /><_field_block_64 name="entries""#,
    );
    let (text, _) = render(&values_tag(), &xml, NORMAL);
    assert_eq!(text.matches("### ").count(), 1, "{text}");
    assert!(text.contains("### entries  block[2]"), "{text}");
}

#[test]
fn every_entry_options_go_to_one_legend() {
    // AGENT_GUIDE §4.4: for `[*]` reads, options appear once, in a `legend:` block.
    let def = Definition::parse(VALUES).unwrap();
    let (h, data) = build_tag(&values_tag());
    let header = TagHeader::parse(&h).unwrap();
    let r = read_tag(&def, &header, &data);
    let p = h5_meta::parse_path(&def.fields, "flags").unwrap();
    let sels = h5_meta::select(&r.root, &p).unwrap();
    let mut nodes = view_selection(&sels, "flags", NORMAL, &Stub);
    nodes.extend(view_selection(&sels, "flags", NORMAL, &Stub).into_iter().skip(1));
    let legend = into_legend(&mut nodes);
    let v = TagView { title: "# t".into(), nodes, legend, ..TagView::default() };
    let text = v.render_lines(false);
    assert_eq!(text.matches("(+2 options)").count(), 0, "{text}");
    assert!(text.ends_with("legend:\n  flags  {first bit, second bit, third bit}\n"), "{text}");
    // JSON keeps each field's options.
    assert_eq!(v.render_json()["fields"][1]["options"], serde_json::json!(["first bit", "second bit", "third bit"]));
}
