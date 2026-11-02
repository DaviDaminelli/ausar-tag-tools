use h5_module::GlobalId;
use h5_module::scan::HeaderScan;

use super::*;
use crate::testdeploy::{TestDeploy, WEAPON_ID};

const BULLET_ID: i32 = 0x0040_1000;
/// The weapon's second dependency: not an indexed tag.
const BULLET_HIT_ID: i32 = 0x0040_2000;

#[test]
fn the_scan_is_built_once_and_finds_references_both_ways() {
    let d = TestDeploy::new("refs-scan");
    let b = &d.session.builds()[0];
    let (scan, built) = header_scan(b);
    // Both tag copies were read; the bullet's data is no tag header.
    assert_eq!(built.map(|x| x.tags), Some(2));
    assert_eq!(scan.problems().count(), 1);

    let weapon = b.tag_by_id(GlobalId(WEAPON_ID)).unwrap().copies[0];
    let out: Vec<GlobalId> = scan.outgoing(weapon).unwrap().references.iter().map(|r| r.target).collect();
    assert_eq!(out, [GlobalId(BULLET_ID), GlobalId(BULLET_HIT_ID)]);
    let into_bullet: Vec<Copy> = scan.incoming(GlobalId(BULLET_ID)).map(|(c, _)| c).collect();
    assert_eq!(into_bullet, [weapon]);
    assert_eq!(scan.incoming(GlobalId(WEAPON_ID)).count(), 0);

    // Later calls reuse it.
    assert!(header_scan(b).1.is_none());
}

#[test]
fn the_scan_is_cached_next_to_the_index() {
    let d = TestDeploy::new("refs-scan-cache");
    let b = &d.session.builds()[0];
    let (scan, _) = header_scan(b);
    let file = b.scan_cache().expect("the test deploy has a cache folder");
    assert_eq!(HeaderScan::load(file).as_ref(), Some(&scan.scan));
}

/// `reindex(what=header_scan)`: the next call scans the dropped modules again.
#[test]
fn a_forgotten_scan_is_built_again() {
    let mut d = TestDeploy::new("refs-scan-forget");
    header_scan(&d.session.builds()[0]);
    let dropped = d.session.forget_header_scan(0, &|_| true);
    assert_eq!(dropped, 1);
    let (_, built) = header_scan(&d.session.builds()[0]);
    assert_eq!(built.map(|x| x.tags), Some(2));
}
