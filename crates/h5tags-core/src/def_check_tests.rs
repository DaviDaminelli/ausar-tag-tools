use h5_module::{FourCc, GlobalId};

use super::*;
use crate::testdeploy::{SecondCopy, TestDeploy, WEAPON_DEFINITION, WEAPON_ID};

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
    let statuses = class_statuses(b, &check, &known);
    let w = status(&statuses, "weap");
    assert_eq!((w.checked, w.size_tags, w.guid_tags, w.unknown_guids, w.layouts), (1, 0, 0, 0, 1));
    let p = status(&statuses, "proj");
    assert!(p.class.definition.is_none());
    assert_eq!(p.checked, 0);
    // D80: no gaps; the layout (main struct GUID) is listed apart, not as a gap.
    assert_eq!(class_gaps(b, &check, weap(), &known), []);
    assert_eq!(class_layouts(&check, weap()), [(MAIN_GUID, 1)]);
}

/// D80: a definition file changed while the session runs is noticed on the
/// next call, and the modules using it are checked again; nothing changed,
/// nothing rebuilt.
#[test]
fn a_definition_changed_in_a_running_session_is_checked_again() {
    let d = TestDeploy::new("check-stale");
    let b = &d.session.builds()[0];
    let known = known_guids(b).unwrap();
    let (check, built) = definition_check(b);
    assert!(built.is_some());
    assert_eq!(status(&class_statuses(b, &check, &known), "weap").size_tags, 0);
    assert!(definition_check(b).1.is_none(), "kept while nothing changes");

    let xml = WEAPON_DEFINITION.replace("</plugin>", "\t<_field_long_integer name=\"extra\" />\n</plugin>");
    std::fs::write(d.dir.0.join("defs").join("(weap)weapon.xml"), xml).unwrap();
    let (check, built) = definition_check(b);
    assert_eq!(built.map(|x| x.tags), Some(1), "the weapon's module checked again");
    assert_eq!(status(&class_statuses(b, &check, &known), "weap").size_tags, 1);
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
    let statuses = class_statuses(b, &check, &known);
    let w = status(&statuses, "weap");
    assert_eq!((w.size_tags, w.guid_tags, w.unknown_guids), (0, 1, 1));
    let gaps = class_gaps(b, &check, weap(), &known);
    assert!(
        gaps.contains(&Gap {
            kind: GapKind::Guid { guid: [2; 16], known: false },
            path: "entries".into(),
            tags: 1,
            default_copies: 1,
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
    assert_eq!(status(&class_statuses(b, &check, &known), "weap").size_tags, 1);
    let gaps = class_gaps(b, &check, weap(), &known);
    let size =
        Gap { kind: GapKind::Size, path: String::new(), tags: 1, default_copies: 1, example: GlobalId(WEAPON_ID) };
    assert!(gaps.contains(&size), "{gaps:?}");
}

/// D80: a size gap only in copies that aren't read by default: here the
/// update layer's copy grew, and the definition fits it; the base copy doesn't.
#[test]
fn a_size_gap_in_older_layers_only_counts_no_default_copy() {
    let xml = WEAPON_DEFINITION.replace("</plugin>", "\t<_field_long_integer name=\"extra\" />\n</plugin>");
    let d = TestDeploy::with_layers_and_definition("check-layers", SecondCopy::Grown, &xml);
    let b = &d.session.builds()[0];
    let (check, _) = definition_check(b);
    let known = known_guids(b).unwrap();
    let gaps = class_gaps(b, &check, weap(), &known);
    let size =
        Gap { kind: GapKind::Size, path: String::new(), tags: 1, default_copies: 0, example: GlobalId(WEAPON_ID) };
    assert_eq!(gaps, [size]);
    let statuses = class_statuses(b, &check, &known);
    let w = status(&statuses, "weap");
    assert_eq!((w.checked, w.size_tags, w.size_in_default), (2, 1, 0));
}

/// D80: where reading stopped for another reason, or the definition and the
/// tag header disagree: named, with its field path. Here the entries' tag
/// reference is read as a real and padding, so the header's two tag
/// references in the entries' data block are reached by no field.
#[test]
fn other_problems_have_a_kind_and_a_path() {
    let xml = WEAPON_DEFINITION.replace(
        r#"<_field_tag_reference_64 name="target" />"#,
        r#"<_field_real name="a" /><_field_pad name="b" length="28" />"#,
    );
    let d = TestDeploy::with_definition("check-other", &xml);
    let b = &d.session.builds()[0];
    let (check, _) = definition_check(b);
    let known = known_guids(b).unwrap();
    assert_eq!(status(&class_statuses(b, &check, &known), "weap").other_tags, 1);
    let gaps = class_gaps(b, &check, weap(), &known);
    let unreached = Gap {
        kind: GapKind::Other(OtherKind::Unreached("tag reference".into())),
        path: String::new(),
        tags: 1,
        default_copies: 1,
        example: GlobalId(WEAPON_ID),
    };
    assert_eq!(gaps, [unreached]);
}

/// D80: the overview puts first what can be acted on: a definition that
/// doesn't fit the copies read by default, then classes without one, other
/// problems, unknown GUIDs, size gaps in older layers only, GUID-only
/// differences, and clean classes last.
#[test]
fn classes_are_ranked_by_what_can_be_acted_on() {
    let info = |definition: bool| ClassInfo {
        code: weap(),
        name: "weapon".into(),
        tag_count: 1,
        definition: definition.then(|| PathBuf::from("w.xml")),
    };
    let (with, without) = (info(true), info(false));
    let st = |class, size_tags, size_in_default, other_tags, unknown_guids, guid_tags| ClassStatus {
        class,
        checked: 1,
        unreadable: 0,
        size_tags,
        size_in_default,
        guid_tags,
        other_tags,
        unknown_guids,
        layouts: 1,
    };
    let ranks = [
        st(&with, 1, 1, 0, 0, 0).rank(),
        st(&without, 0, 0, 0, 0, 0).rank(),
        st(&with, 0, 0, 1, 0, 0).rank(),
        st(&with, 0, 0, 0, 1, 1).rank(),
        st(&with, 1, 0, 0, 0, 0).rank(),
        st(&with, 0, 0, 0, 0, 1).rank(),
        st(&with, 0, 0, 0, 0, 0).rank(),
    ];
    assert!(ranks.windows(2).all(|w| w[0] < w[1]), "{ranks:?}");
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
