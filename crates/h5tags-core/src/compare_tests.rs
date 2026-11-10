use h5_module::{FourCc, GlobalId};

use super::*;
use crate::references::header_scan;
use crate::testdeploy::{BULLET_ID, MORE_WEAPON_ID, TestDeploy, TwoBuilds};

/// `forge` with more weapons 36, 12, 7 and `xbox` with 50, 12: `more_0`
/// differs, `more_1` and the first weapon are the same, `more_2` is in forge
/// only; the bullet's data is no tag, so it can't be scanned in either.
fn builds(name: &str) -> TwoBuilds {
    TestDeploy::two_builds(name, &[Some(36), Some(12), Some(7)], &[Some(50), Some(12)])
}

fn more(i: i32) -> GlobalId {
    GlobalId(MORE_WEAPON_ID + i)
}

fn rows(c: &Comparison<'_>) -> Vec<(Status, GlobalId)> {
    c.rows.iter().map(|r| (r.status, r.tag.global_id)).collect()
}

/// M6 (D82): tags matched by global ID, each build's copy read by default
/// compared by its tag data fingerprint (D81).
#[test]
fn tags_by_global_id_and_data() {
    let d = builds("compare-both");
    let [a, b] = [&d.session.builds()[0], &d.session.builds()[1]];
    let (sa, sb) = (header_scan(a).0, header_scan(b).0);
    let c = compare_builds(a, sa, b, sb, None);
    assert_eq!(rows(&c), [(Status::Different, more(0)), (Status::OnlyInA, more(2))]);
    assert_eq!(c.count(Status::Same), 2, "the first weapon and more_1");
    assert_eq!(c.not_compared.iter().map(|t| t.global_id).collect::<Vec<_>>(), [GlobalId(BULLET_ID)]);

    // The other way round, the tag only one build has is only in b.
    let c = compare_builds(b, sb, a, sa, None);
    assert_eq!(rows(&c), [(Status::Different, more(0)), (Status::OnlyInB, more(2))]);
}

/// With a class, only its tags are compared and counted.
#[test]
fn a_class_narrows_the_comparison() {
    let d = builds("compare-class");
    let [a, b] = [&d.session.builds()[0], &d.session.builds()[1]];
    let (sa, sb) = (header_scan(a).0, header_scan(b).0);
    let proj = compare_builds(a, sa, b, sb, FourCc::from_code("proj"));
    assert!(proj.rows.is_empty() && proj.count(Status::Same) == 0);
    assert_eq!(proj.not_compared.len(), 1);
    let weap = compare_builds(a, sa, b, sb, FourCc::from_code("weap"));
    assert_eq!(weap.rows.len(), 2);
    assert!(weap.not_compared.is_empty());
}
