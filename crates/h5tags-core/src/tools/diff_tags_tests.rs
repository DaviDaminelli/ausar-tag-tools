use super::*;
use crate::output::Format;
use crate::testdeploy::{BULLET, TestDeploy, WEAPON};

const MORE_0: &str = r"objects\test\more_0.weapon";
const MORE_1: &str = r"objects\test\more_1.weapon";

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
    assert!(diff(&mut d, MORE_0, MORE_0, None).unwrap().starts_with("total: 0 "));
    // path= narrows the comparison to that part.
    assert!(diff(&mut d, MORE_0, MORE_1, Some("entries")).unwrap().starts_with("total: 0 "));
    // A part neither tag has is a path error, not "no differences".
    assert!(diff(&mut d, MORE_0, MORE_1, Some("entries[9]")).is_err());
}

#[test]
fn both_tags_must_have_one_class() {
    let mut d = TestDeploy::new("diff-classes");
    let err = diff(&mut d, WEAPON, BULLET, None).unwrap_err();
    assert_eq!(err.message, format!("{WEAPON} is a weapon and {BULLET} is a projectile"));
    assert_eq!(err.hint.as_deref(), Some("diff_tags compares two tags of the same class"));
}
