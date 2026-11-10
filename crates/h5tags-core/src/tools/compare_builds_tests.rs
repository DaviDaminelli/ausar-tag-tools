use super::*;
use crate::output::Format;
use crate::testdeploy::{TestDeploy, TwoBuilds};

/// `forge` with more weapons 36, 12, 7 and `xbox` with 50, 12 (see compare_tests).
fn builds(name: &str) -> TwoBuilds {
    TestDeploy::two_builds(name, &[Some(36), Some(12), Some(7)], &[Some(50), Some(12)])
}

fn compare(d: &mut TwoBuilds, f: impl FnOnce(&mut CompareBuilds)) -> Result<String, ToolError> {
    let mut t = CompareBuilds {
        build_a: "forge".into(),
        build_b: "xbox".into(),
        class: None,
        status: None,
        page: PageArgs::default(),
    };
    f(&mut t);
    t.run(&mut d.session).map(|o| o.render(Format::Text))
}

/// D82: counts first (both builds named), then the rows that aren't the same:
/// different, then only in a; a tag no scan could read is said, not hidden.
#[test]
fn counts_then_the_tags_that_differ() {
    let mut d = builds("tool-compare");
    let text = compare(&mut d, |_| {}).unwrap();
    let first = text.lines().next().unwrap();
    assert!(
        first.starts_with(
            "total: 2  a: forge  b: xbox  different: 1  only in a: 1  only in b: 0  same: 2  not compared: 1"
        ),
        "{text}"
    );
    assert!(text.contains("\nstatus\tclass\tglobal ID\tdisplay name\n"), "{text}");
    let rows: Vec<&str> = text.lines().filter(|l| l.contains("\tweap\t")).collect();
    assert_eq!(rows.len(), 2, "{text}");
    assert!(rows[0].starts_with("different\tweap\t0x00000030\t") && rows[0].ends_with("more_0.weapon"), "{text}");
    assert!(rows[1].starts_with("only in a\tweap\t0x00000032\t") && rows[1].ends_with("more_2.weapon"), "{text}");
    assert!(text.contains("\nhint: 1 tag in both builds couldn't be compared"), "{text}");
}

/// status= keeps one kind of row; the counts stay whole.
#[test]
fn status_keeps_one_kind_of_row() {
    let mut d = builds("tool-compare-status");
    let text = compare(&mut d, |t| t.status = Some(RowStatus::OnlyInA)).unwrap();
    assert!(text.starts_with("total: 1  a: forge  b: xbox  different: 1  only in a: 1"), "{text}");
    assert!(!text.contains("different\tweap"), "{text}");
}

/// Two builds are needed, and they must exist.
#[test]
fn two_existing_builds() {
    let mut d = builds("tool-compare-builds");
    let err = compare(&mut d, |t| t.build_b = "forge".into()).unwrap_err();
    assert_eq!(err.message, "build_a and build_b are the same build (forge)");
    let err = compare(&mut d, |t| t.build_b = "pc".into()).unwrap_err();
    assert!(err.message.contains("unknown build 'pc'"), "{}", err.message);
}
