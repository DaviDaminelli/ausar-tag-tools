use super::*;
use crate::output::Format;
use crate::testdeploy::{SecondCopy, TestDeploy, WEAPON, WEAPON_DEFINITION};

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

/// D33, D80: with class=, the gap list: each mismatch with its field path,
/// GUID, how many of the class's copies have it, and an example tag; the
/// class's layouts (main struct GUIDs) above, not as gaps.
#[test]
fn a_class_lists_its_gaps() {
    let xml = WEAPON_DEFINITION.replace("02020202-0202-0202-0202-020202020202", "03030303-0303-0303-0303-030303030303");
    let mut d = TestDeploy::with_definition("status-class", &xml);
    let text = status(&mut d, Some("weap"));
    let lines: Vec<&str> = text.lines().collect();
    let main = h5_defs::definition::guid_text(&[1; 16]);
    assert!(lines[0].starts_with(&format!("total: 1  tag copies checked: 1  layouts: {main} (1 copy)")), "{text}");
    assert_eq!(lines[1], "gap\tfield path\tGUID\tcopies\texample");
    let guid = h5_defs::definition::guid_text(&[2; 16]);
    assert_eq!(lines[2], format!("GUID (in no definition)\tentries\t{guid}\t1 of 1\t{WEAPON}"));
    assert_eq!(lines.len(), 3, "{text}");
}

/// D80: a class with nothing wrong says so, instead of a one-row table.
#[test]
fn a_class_without_gaps_says_so() {
    let mut d = TestDeploy::new("status-clean");
    let text = status(&mut d, Some("weap"));
    assert!(text.starts_with("total: 0  tag copies checked: 1  layouts: "), "{text}");
    assert!(text.contains("\nhint: no gaps: every tag copy of weapon read with its definition\n"), "{text}");
}

/// D80: a gap no copy read by default has (an older layer's) says so.
#[test]
fn a_gap_in_older_layers_only_says_so() {
    let xml = WEAPON_DEFINITION.replace("</plugin>", "\t<_field_long_integer name=\"extra\" />\n</plugin>");
    let mut d = TestDeploy::with_layers_and_definition("status-layers", SecondCopy::Grown, &xml);
    let text = status(&mut d, Some("weap"));
    assert!(text.contains(&format!("\nsize (older layers only)\t\t\t1 of 2\t{WEAPON}\n")), "{text}");
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

/// A class with a definition and no tags here: nothing to check, said; it
/// used to panic (D98).
#[test]
fn a_class_with_only_a_definition_has_nothing_to_check() {
    let object = "<plugin>\n\t<_field_custom name=\"OBJECT\" />\n\t<_field_long_integer name=\"count\" />\n</plugin>";
    let mut d = TestDeploy::with_more_definitions("status-definition-only", &[("(obje)object.xml", object)]);
    let text = status(&mut d, Some("object"));
    assert!(text.starts_with("total: 0"), "{text}");
    assert!(
        text.contains("\nhint: no tags of class 'obje' (object) in the indexed modules of build 'forge'."),
        "{text}"
    );
}
