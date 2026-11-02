use super::*;
use crate::output::Format;
use crate::testdeploy::{TestDeploy, WEAPON, WEAPON_DEFINITION};

fn status(d: &mut TestDeploy, class: Option<&str>) -> String {
    let t = DefinitionStatus { class: class.map(Into::into), build: BuildArg::default(), page: PageArgs::default() };
    t.run(&mut d.session).unwrap().render(Format::Text)
}

/// PRD §6.5: one row per class; classes with gaps (here: no definition) first.
#[test]
fn every_class_with_what_the_check_found() {
    let mut d = TestDeploy::new("status-overview");
    let text = status(&mut d, None);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with(
            "total: 2  tag copies checked: 1  classes without a definition: 1  definition check built: 1 tags, "
        ),
        "{text}"
    );
    assert_eq!(
        lines[1],
        "class\tclass name\ttags\tdefinition\tchecked\tsize mismatches\tGUID mismatches\tunknown GUIDs\tother problems\tlayouts"
    );
    assert_eq!(lines[2], "proj\tprojectile\t1\tno\t0\t0\t0\t0\t0\t0");
    assert_eq!(lines[3], "weap\tweapon\t1\tyes\t1\t0\t0\t0\t0\t1");
    // The second call reuses the check: no "built" line.
    assert!(status(&mut d, None).starts_with("total: 2  tag copies checked: 1  classes without a definition: 1\n"));
}

/// D33: with class=, the gap list: each mismatch with its field path, GUID,
/// tag count and an example tag; then the main struct GUIDs.
#[test]
fn a_class_lists_its_gaps() {
    let xml = WEAPON_DEFINITION.replace("02020202-0202-0202-0202-020202020202", "03030303-0303-0303-0303-030303030303");
    let mut d = TestDeploy::with_definition("status-class", &xml);
    let text = status(&mut d, Some("weap"));
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("total: 2  tag copies checked: 1"), "{text}");
    assert_eq!(lines[1], "gap\tfield path\tGUID\ttags\texample");
    let guid = h5_defs::definition::guid_text(&[2; 16]);
    assert_eq!(lines[2], format!("GUID (in no definition)\tentries\t{guid}\t1\t{WEAPON}"));
    let main = h5_defs::definition::guid_text(&[1; 16]);
    assert_eq!(lines[3], format!("main struct\t\t{main}\t1\t{WEAPON}"));
}

/// A class without a definition has nothing checked: said, not an error.
#[test]
fn a_class_without_a_definition() {
    let mut d = TestDeploy::new("status-nodef");
    let text = status(&mut d, Some("proj"));
    assert!(text.starts_with("total: 0"), "{text}");
    assert!(
        text.contains(
            "hint: class proj (projectile) has no definition: its tags can't be read field by field or checked"
        ),
        "{text}"
    );
}
