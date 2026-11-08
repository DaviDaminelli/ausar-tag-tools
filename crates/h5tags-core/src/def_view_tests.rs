use h5_defs::definition::Definition;
use h5_meta::read_tag;
use h5_module::tag::TagHeader;
use h5_module::testutil::{build_tag, sample_tag};

use super::*;
use crate::tag_view::GuidCheck;
use crate::view::TagView;

const DEFINITION: &str = r#"<plugin>
	<_field_int64_integer name="vtable space*~!" />
	<_field_custom name="WEAPON" />
	<_field_explanation name="Firing">How it fires.
More.</_field_explanation>
	<_field_block_64 name="barrels" max="2" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_real name="rounds per second:[0-60]per second#how fast" />
		<_field_short_enum name="trigger mode"><item Name="normal" /><item Name="latch" /></_field_short_enum>
		<_field_struct name="shots">
			<_field_real name="spread" />
			<_field_block_64 name="patterns"><_field_real name="angle" /></_field_block_64>
		</_field_struct>
	</_field_block_64>
	<_field_pad name="pad" length="4" />
	<_field_tag_reference_64 name="projectile" validtags="proj" />
	<_field_short_block_index name="primary barrel" targetGuid="02020202-0202-0202-0202-020202020202" />
</plugin>"#;

const NORMAL: ViewSettings = ViewSettings {
    depth: 2,
    detail: Detail::Normal,
    options: Options::Auto,
    show_hidden: false,
    guid_check: GuidCheck::Warn,
};

fn text(nodes: Vec<ViewNode>) -> String {
    TagView { title: "# weap weapon (definition)".into(), nodes, ..TagView::default() }.render_lines(false)
}

#[test]
fn a_definition_shows_types_units_options_and_targets() {
    let d = Definition::parse(DEFINITION).unwrap();
    let t = text(definition_view(&d.fields, "", NORMAL).unwrap());
    let l: Vec<&str> = t.lines().collect();
    assert_eq!(l[1], "## WEAPON");
    assert_eq!(l[2], "### Firing");
    assert_eq!(l[3], "> How it fires. […]");
    assert_eq!(l[4], "### barrels  block  (max 2)");
    assert_eq!(l[5], "  rounds per second  real  [0-60] per second");
    assert_eq!(l[6], "  trigger mode  enum16  {normal|latch}");
    assert_eq!(l[7], "  #### shots  struct");
    assert_eq!(l[8], "    spread  real");
    assert_eq!(l[9], "    ##### patterns  block  → path=barrels.shots.patterns");
    assert_eq!(l[10], "projectile  tagref  [proj]");
    assert_eq!(l[11], "primary barrel  blkidx  → barrels");
    // The internal header and padding are hidden.
    assert_eq!(l.len(), 12, "{t}");
}

#[test]
fn full_detail_adds_help_offsets_and_markers() {
    let d = Definition::parse(DEFINITION).unwrap();
    let t = text(definition_view(&d.fields, "", ViewSettings { detail: Detail::Full, ..NORMAL }).unwrap());
    assert!(t.contains("  rounds per second  real  [0-60] per second  @0x0  # how fast\n"), "{t}");
    assert!(t.contains("> How it fires.\n> More.\n"), "{t}");
}

#[test]
fn an_outline_is_the_section_tree() {
    let d = Definition::parse(DEFINITION).unwrap();
    let t = text(outline_definition(&d.fields, 3));
    assert_eq!(
        t,
        "# weap weapon (definition)\n\
         ## WEAPON\n\
         ### Firing\n\
         ### barrels  block  (3 fields)  path=barrels\n\
         \x20 #### shots  struct  (2 fields)  path=barrels.shots\n\
         \x20   ##### patterns  block  (1 field)  path=barrels.shots.patterns\n"
    );
}

/// The sample tag (block of 2 entries, a tag reference, data, an int) for a tag outline.
const SAMPLE: &str = r#"<plugin>
	<_field_block_64 name="entries" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_tag_reference_64 name="target" />
	</_field_block_64>
	<_field_pad name="pad" length="4" />
	<_field_tag_reference_64 name="projectile" />
	<_field_data_64 name="data" />
	<_field_long_integer name="count" />
</plugin>"#;

#[test]
fn a_tag_outline_has_entry_counts_and_labels() {
    let d = Definition::parse(SAMPLE).unwrap();
    let (h, data) = build_tag(&sample_tag());
    let r = read_tag(&d, &TagHeader::parse(&h).unwrap(), &data);
    let t = text(outline_tag(&r.root, &d.fields, 3));
    assert!(t.contains("### entries  block[2]  {0, bullet_hit}  (1 field)  path=entries\n"), "{t}");
}

/// D70: the outline comes from the definition; what wasn't read (here the
/// fields after `data`, past the last anchor the header confirms, since the
/// definition has one more block than the tag) says `(not read)`.
#[test]
fn a_tag_outline_shows_every_container_read_or_not() {
    let later = r#"<_field_block_64 name="later"><_field_real name="x" /></_field_block_64>"#;
    let xml = SAMPLE.replace("</plugin>", &format!("{later}</plugin>"));
    let d = Definition::parse(&xml).unwrap();
    let (h, data) = build_tag(&sample_tag());
    let r = read_tag(&d, &TagHeader::parse(&h).unwrap(), &data);
    assert!(r.root.problem.is_some(), "the definition is 0x1C bytes longer");
    let t = text(outline_tag(&r.root, &d.fields, 3));
    assert!(t.contains("### entries  block[2]  {0, bullet_hit}  (1 field)  path=entries\n"), "{t}");
    assert!(t.contains("### later  block  (not read)  (1 field)  path=later\n"), "{t}");

    // Nothing read (no anchor confirmed): the whole tree, every container not read.
    let xml = xml.replace("<plugin>", r#"<plugin><_field_long_integer name="first" />"#);
    let d = Definition::parse(&xml).unwrap();
    let r = read_tag(&d, &TagHeader::parse(&h).unwrap(), &data);
    assert!(r.root.children.is_empty());
    let t = text(outline_tag(&r.root, &d.fields, 3));
    assert!(t.contains("### entries  block  (not read)  (1 field)  path=entries\n"), "{t}");
    assert!(t.contains("### later  block  (not read)  (1 field)  path=later\n"), "{t}");
}

#[test]
fn a_path_narrows_the_definition() {
    let d = Definition::parse(DEFINITION).unwrap();
    // Tag blocks are stepped into without an index; [i] and [*] are ignored.
    let t = text(definition_view(&d.fields, "barrels[*].shots", NORMAL).unwrap());
    assert!(t.contains("\n### barrels.shots  (path)\n"), "{t}");
    assert!(t.contains("\nspread  real\n"), "{t}");
}
