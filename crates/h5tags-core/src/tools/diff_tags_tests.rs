use super::*;
use crate::output::Format;
use crate::testdeploy::{BULLET, TestDeploy, WEAPON};

const MORE_0: &str = r"objects\test\more_0.weapon";
const MORE_1: &str = r"objects\test\more_1.weapon";
const CANDIDATES_HINT_LINE: &str =
    "hint: these are candidates, not causes: check one across the class with query_field before naming it the cause";

fn diff(d: &mut TestDeploy, a: &str, b: &str, path: Option<&str>) -> Result<String, ToolError> {
    diff_in(d, a, b, path, None)
}

fn diff_in(
    d: &mut TestDeploy,
    a: &str,
    b: &str,
    path: Option<&str>,
    section: Option<&str>,
) -> Result<String, ToolError> {
    let t = DiffTags {
        a: a.into(),
        b: b.into(),
        path: path.map(Into::into),
        section: section.map(Into::into),
        build: BuildArg::default(),
        page: PageArgs::default(),
    };
    t.run(&mut d.session).map(|o| o.render(Format::Text))
}

/// section= compares the fields under one heading, as read_tag reads them.
#[test]
fn a_section_compares_its_fields() {
    let mut d = TestDeploy::with_weapons("diff-section", &[Some(36), Some(12)]);
    let text = diff_in(&mut d, MORE_0, MORE_1, None, Some("weapon")).unwrap();
    assert!(text.contains("\ncount\t36\t12\n"), "{text}");
    let err = diff_in(&mut d, MORE_0, MORE_1, None, Some("nothing")).unwrap_err();
    assert!(err.message.contains("nothing"), "{}", err.message);
    let err = diff_in(&mut d, MORE_0, MORE_1, Some("count"), Some("weapon")).unwrap_err();
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
