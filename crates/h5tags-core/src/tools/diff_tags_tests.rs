use super::*;
use crate::output::Format;
use crate::testdeploy::{BULLET, SecondCopy, TestDeploy, WEAPON, WEAPON_DEFINITION};

const MORE_0: &str = r"objects\test\more_0.weapon";
const MORE_1: &str = r"objects\test\more_1.weapon";
const CANDIDATES_HINT_LINE: &str =
    "hint: these are candidates, not causes: check one across the class with query_field before naming it the cause";

fn diff(d: &mut TestDeploy, a: &str, b: &str, path: Option<&str>) -> Result<String, ToolError> {
    diff_in(&mut d.session, a, b, path, None)
}

fn args(a: &str, b: &str) -> DiffTags {
    DiffTags {
        a: a.into(),
        b: b.into(),
        path: None,
        section: None,
        module_a: None,
        module_b: None,
        build: BuildArg::default(),
        page: PageArgs::default(),
    }
}

fn diff_in(s: &mut Session, a: &str, b: &str, path: Option<&str>, section: Option<&str>) -> Result<String, ToolError> {
    let t = DiffTags { path: path.map(Into::into), section: section.map(Into::into), ..args(a, b) };
    t.run(s).map(|o| o.render(Format::Text))
}

const SCRIPT_0: &str = r"scripts\script_0.lua";
const SCRIPT_1: &str = r"scripts\script_1.lua";

/// D89: two script tags compare their scripts line by line: the changed
/// lines only, each with its line number in a or in b.
#[test]
fn scripts_compare_line_by_line() {
    let mut d = TestDeploy::with_scripts(
        "diff-scripts",
        &[b"a\r\nb\r\nc\r\n\0", b"a\r\n  B\r\nc\r\nd\r\n\0", b"a\r\nb\r\nc\r\n\0"],
    );
    let text = diff(&mut d, SCRIPT_0, SCRIPT_1, None).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        [
            &format!("total: 3  a: {SCRIPT_0}  b: {SCRIPT_1}  lines: 3 / 4"),
            "a\tb\ttext",
            "2\t\t- b",
            "\t2\t+ B",
            "\t4\t+ d",
            "hint: changed lines only; read_script(tag, offset=<line - 1>) shows the lines around them; path= compares the tags' fields instead",
        ]
    );
    let same = diff(&mut d, SCRIPT_0, r"scripts\script_2.lua", None).unwrap();
    assert!(same.starts_with("total: 0 "), "{same}");
    assert!(same.contains("\nhint: the scripts are the same; path= compares the tags' other fields\n"), "{same}");
    // path= compares fields, as for any tag.
    let fields = diff(&mut d, SCRIPT_0, SCRIPT_1, Some("source file")).unwrap();
    assert!(fields.contains("\nsource file.source\t"), "{fields}");
}

/// D89: module_a / module_b read another layer's copy, for any tag; a copy
/// that isn't the latest layer's names its module.
#[test]
fn copies_from_two_layers() {
    let mut d = TestDeploy::with("diff-layers", SecondCopy::Changed);
    let t = DiffTags { module_a: Some("test-rtx-1.module".into()), ..args(WEAPON, WEAPON) };
    let text = t.run(&mut d.session).unwrap().render(Format::Text);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], format!(r"total: 1  a: {WEAPON} (any\levels\test-rtx-1.module)  b: {WEAPON}"));
    assert_eq!(lines[2], "count\t0\t9");
}

const FORGE_MORE_0: &str = r"forge:objects\test\more_0.weapon";
const XBOX_MORE_0: &str = r"xbox:objects\test\more_0.weapon";

/// D83: two tags in two builds, each read in its own build; with the same
/// definition, compared field by field as within one build.
#[test]
fn tags_in_two_builds() {
    let mut d = TestDeploy::two_builds("diff-builds", &[Some(36)], &[Some(50)]);
    let text = diff_in(&mut d.session, FORGE_MORE_0, XBOX_MORE_0, None, None).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], format!("total: 1  a: {FORGE_MORE_0}  b: {XBOX_MORE_0}"));
    assert_eq!(lines[2], "count\t36\t50");
}

/// D83: the other build defines the class differently (`count` is `rounds`
/// there): fields matched by name, a field only one definition has is its
/// own row, and a hint says the definitions differ.
#[test]
fn another_definition_in_the_other_build() {
    let xml = WEAPON_DEFINITION.replace(r#"name="count""#, r#"name="rounds""#);
    let mut d = TestDeploy::two_builds_with_definition("diff-defs", &[Some(36)], &[Some(50)], &xml);
    let text = diff_in(&mut d.session, FORGE_MORE_0, XBOX_MORE_0, None, None).unwrap();
    assert!(text.contains("\ncount\t36\t(no field)\n"), "{text}");
    assert!(text.contains("\nrounds\t(no field)\t50\n"), "{text}");
    assert!(
        text.contains("\nhint: the two builds' definitions of weapon differ: fields are matched by name"),
        "{text}"
    );
    let err = diff_in(&mut d.session, FORGE_MORE_0, XBOX_MORE_0, None, Some("weapon")).unwrap_err();
    assert_eq!(err.message, "section= needs the same definition in both builds");
    // path= is resolved in each build's definition; one without it says which.
    let err = diff_in(&mut d.session, FORGE_MORE_0, XBOX_MORE_0, Some("count"), None).unwrap_err();
    assert!(err.message.starts_with("in xbox's definition: "), "{}", err.message);
    // A field both definitions have compares as usual.
    let same = diff_in(&mut d.session, FORGE_MORE_0, XBOX_MORE_0, Some("entries"), None).unwrap();
    assert!(same.starts_with("total: 0 "), "{same}");
}

/// D83: a tag block only one definition has reads as its short type, as
/// read_tag writes it (`block[N]`), not as an empty value.
#[test]
fn a_block_only_one_definition_has() {
    let xml = WEAPON_DEFINITION.replace(r#"name="entries""#, r#"name="targets""#);
    let mut d = TestDeploy::two_builds_with_definition("diff-def-block", &[Some(36)], &[Some(36)], &xml);
    let text = diff_in(&mut d.session, FORGE_MORE_0, XBOX_MORE_0, None, None).unwrap();
    let row = |name: &str| text.lines().find(|l| l.starts_with(&format!("{name}\t"))).unwrap_or_default().to_owned();
    assert!(row("entries").starts_with("entries\tblock[") && row("entries").ends_with("]\t(no field)"), "{text}");
    assert!(row("targets").starts_with("targets\t(no field)\tblock["), "{text}");
}

/// section= compares the fields under one heading, as read_tag reads them.
#[test]
fn a_section_compares_its_fields() {
    let mut d = TestDeploy::with_weapons("diff-section", &[Some(36), Some(12)]);
    let text = diff_in(&mut d.session, MORE_0, MORE_1, None, Some("weapon")).unwrap();
    assert!(text.contains("\ncount\t36\t12\n"), "{text}");
    let err = diff_in(&mut d.session, MORE_0, MORE_1, None, Some("nothing")).unwrap_err();
    assert!(err.message.contains("nothing"), "{}", err.message);
    let err = diff_in(&mut d.session, MORE_0, MORE_1, Some("count"), Some("weapon")).unwrap_err();
    assert_eq!(err.message, "path= and section= can't be used together");
}

/// PRD §6.6: only the fields whose values differ, as path, a, b.
#[test]
fn only_the_differing_fields() {
    let mut d = TestDeploy::with_weapons("diff-values", &[Some(36), Some(12)]);
    let text = diff(&mut d, MORE_0, MORE_1, None).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], format!("total: 1  a: {MORE_0}  b: {MORE_1}"));
    assert_eq!(lines[1], "field path\ta\tb");
    assert_eq!(lines[2], "count\t36\t12");
    // Method step 2 (AGENT_GUIDE §2.1), where the agent is looking (D64).
    assert_eq!(lines.last().copied(), Some(CANDIDATES_HINT_LINE));
    let same = diff(&mut d, MORE_0, MORE_0, None).unwrap();
    assert!(same.starts_with("total: 0 ") && !same.contains("hint:"), "{same}");
    // path= narrows the comparison to that part.
    assert!(diff(&mut d, MORE_0, MORE_1, Some("entries")).unwrap().starts_with("total: 0 "));
    // A part neither tag has is a path error, not "no differences".
    assert!(diff(&mut d, MORE_0, MORE_1, Some("entries[9]")).is_err());
}

/// D72: a row's path has tag a's indices, and b's where they differ (entries
/// matched by label); an entry only b has shows b's index alone.
#[test]
fn a_path_shows_bs_index_where_it_differs() {
    use h5_meta::{Select, Step};
    let def = h5_defs::definition::Definition::parse(crate::testdeploy::WEAPON_DEFINITION).unwrap();
    // Field 1 is the tag block `entries`, its field 0 `target`.
    let path = |e: u32| FieldPath {
        steps: vec![Step { field: 1, select: Some(Select::Entry(e)) }, Step { field: 0, select: None }],
    };
    let entry = |e: u32| FieldPath { steps: vec![Step { field: 1, select: Some(Select::Entry(e)) }] };
    let d = |path, in_b, kind| Difference { path, in_b, kind };
    assert_eq!(path_text(&d(path(2), Some(path(3)), Kind::Changed), &def.fields), "entries[2] (b: [3]).target");
    assert_eq!(path_text(&d(path(2), None, Kind::Changed), &def.fields), "entries[2].target");
    assert_eq!(path_text(&d(entry(1), None, Kind::OnlyIn(Side::B)), &def.fields), "entries (b: [1])");
    assert_eq!(path_text(&d(entry(1), None, Kind::OnlyIn(Side::A)), &def.fields), "entries[1]");
}

#[test]
fn both_tags_must_have_one_class() {
    let mut d = TestDeploy::new("diff-classes");
    let err = diff(&mut d, WEAPON, BULLET, None).unwrap_err();
    assert_eq!(err.message, format!("{WEAPON} is a weapon and {BULLET} is a projectile"));
    assert_eq!(err.hint.as_deref(), Some("diff_tags compares two tags of the same class"));
}
