use h5_module::{FourCc, GlobalId};

use super::*;
use crate::testdeploy::{TestDeploy, WEAPON_DEFINITION, WEAPON_ID};

fn weap() -> FourCc {
    FourCc::from_code("weap").unwrap()
}

fn status<'b>(statuses: &'b [ClassStatus<'_>], code: &str) -> &'b ClassStatus<'b> {
    statuses.iter().find(|s| s.class.code == FourCc::from_code(code).unwrap()).unwrap()
}

/// The sample tag's main struct GUID (`h5_module::testutil::sample_tag`).
const MAIN_GUID: [u8; 16] = [1; 16];

/// PRD §6.5: per class, whether it has a definition and what the check found.
#[test]
fn a_matching_definition_has_no_gaps() {
    let d = TestDeploy::new("check-clean");
    let b = &d.session.builds()[0];
    let (check, built) = definition_check(b);
    assert_eq!(built.map(|x| x.tags), Some(1), "only the weapon has a definition");
    let known = known_guids(b).unwrap();
    let statuses = class_statuses(b, check, &known);
    let w = status(&statuses, "weap");
    assert_eq!((w.checked, w.size_tags, w.guid_tags, w.unknown_guids, w.layouts), (1, 0, 0, 0, 1));
    let p = status(&statuses, "proj");
    assert!(p.class.definition.is_none());
    assert_eq!(p.checked, 0);
    assert_eq!(
        class_gaps(check, weap(), &known),
        [Gap { kind: GapKind::MainStruct(MAIN_GUID), path: String::new(), tags: 1, example: GlobalId(WEAPON_ID) }]
    );
}

/// D33: a struct GUID that differs from the definition's, with whether any
/// definition knows it.
#[test]
fn a_changed_struct_guid_is_a_guid_gap() {
    let xml = WEAPON_DEFINITION.replace("02020202-0202-0202-0202-020202020202", "03030303-0303-0303-0303-030303030303");
    let d = TestDeploy::with_definition("check-guid", &xml);
    let b = &d.session.builds()[0];
    let (check, _) = definition_check(b);
    let known = known_guids(b).unwrap();
    assert!(known.contains(&[3; 16]) && !known.contains(&[2; 16]));
    let statuses = class_statuses(b, check, &known);
    let w = status(&statuses, "weap");
    assert_eq!((w.size_tags, w.guid_tags, w.unknown_guids), (0, 1, 1));
    let gaps = class_gaps(check, weap(), &known);
    assert!(
        gaps.contains(&Gap {
            kind: GapKind::Guid { guid: [2; 16], known: false },
            path: "entries".into(),
            tags: 1,
            example: GlobalId(WEAPON_ID),
        }),
        "{gaps:?}"
    );
}

/// PRD §6.4.5: a definition whose size differs from the tag's, at the struct
/// where it starts (the main struct: empty path).
#[test]
fn a_size_mismatch_is_a_size_gap() {
    let xml = WEAPON_DEFINITION.replace("</plugin>", "\t<_field_long_integer name=\"extra\" />\n</plugin>");
    let d = TestDeploy::with_definition("check-size", &xml);
    let b = &d.session.builds()[0];
    let (check, _) = definition_check(b);
    let known = known_guids(b).unwrap();
    assert_eq!(status(&class_statuses(b, check, &known), "weap").size_tags, 1);
    let gaps = class_gaps(check, weap(), &known);
    assert!(
        gaps.contains(&Gap { kind: GapKind::Size, path: String::new(), tags: 1, example: GlobalId(WEAPON_ID) }),
        "{gaps:?}"
    );
}

/// PRD §6.9: kept on disk, reused while the module and the definitions it
/// used are unchanged; a changed definition file has the module checked again.
#[test]
fn the_check_is_kept_and_reused() {
    let d = TestDeploy::new("check-cache");
    let b = &d.session.builds()[0];
    let never = |_: &str| false;
    let (first, report) = DefinitionCheck::update(b, None, &never);
    assert_eq!((report.read, report.reused), (1, 0));
    let (_, report) = DefinitionCheck::update(b, Some(first.clone()), &never);
    assert_eq!((report.read, report.reused), (0, 1));
    let def = b.classes[&weap()].definition.clone().unwrap();
    std::fs::write(&def, WEAPON_DEFINITION.replace("</plugin>", "<!-- changed -->\n</plugin>")).unwrap();
    let (_, report) = DefinitionCheck::update(b, Some(first), &never);
    assert_eq!((report.read, report.reused), (1, 0));
}
