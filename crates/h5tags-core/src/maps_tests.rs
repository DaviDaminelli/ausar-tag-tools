use h5_module::GlobalId;

use super::*;
use crate::testdeploy::{BULLET_ID, MAP, NOT_INDEXED_ID, TestDeploy, WEAPON_ID};

fn module_paths(b: &Build, modules: &[u32]) -> Vec<String> {
    modules.iter().map(|&m| b.index.modules[m as usize].rel_path.clone()).collect()
}

const BASE: &str = r"any\levels\menu\menu-rtx-1.module";
const UPDATE: &str = r"any\levels\menu\menu-rtx-11587-1.module";

/// D59: per map, the manifest in the latest layer is the one in effect (D58).
#[test]
fn the_latest_manifest_of_each_map() {
    let d = TestDeploy::with_map("maps-latest");
    let b = &d.session.builds()[0];
    assert_eq!(module_paths(b, &manifests_in_effect(b)), [UPDATE]);
}

/// Folders don't matter: modules are found anywhere in the deploy folder, and
/// a map's layers are the same name in the same folder.
#[test]
fn a_flat_deploy_folder_works_the_same() {
    let d = TestDeploy::with_flat_map("maps-flat");
    let b = &d.session.builds()[0];
    assert_eq!(module_paths(b, &manifests_in_effect(b)), ["menu-rtx-11587-1.module"]);
    let found = which_maps(b, GlobalId(BULLET_ID));
    assert_eq!(module_paths(b, &found.modules), ["menu-rtx-11587-1.module"]);
}

/// A module filter matching layers of one map picks the latest; one naming a
/// module picks it; none, or several maps, are errors.
#[test]
fn picking_a_manifest() {
    let d = TestDeploy::with_map("maps-pick");
    let b = &d.session.builds()[0];
    let pick = |text: &str| pick_manifest(b, |path| path.contains(text)).map(|m| module_paths(b, &[m]));
    assert_eq!(pick("menu").unwrap(), [UPDATE]);
    assert_eq!(pick("menu-rtx-1.module").unwrap(), [BASE]);
    assert!(matches!(pick("test-rtx"), Err(PickError::NoManifest)));
    assert!(matches!(pick("nowhere"), Err(PickError::NoManifest)));
}

/// PRD §6.6: the manifest's indexed tags by class, then path; the IDs that
/// aren't indexed tags are counted.
#[test]
fn tags_of_a_manifest() {
    let d = TestDeploy::with_map("maps-tags");
    let b = &d.session.builds()[0];
    let base = pick_manifest(b, |p| p == BASE).unwrap();
    let found = map_tags(b, base, None).unwrap();
    let ids: Vec<GlobalId> = found.tags.iter().map(|t| t.global_id).collect();
    // proj before weap.
    assert_eq!(ids, [GlobalId(BULLET_ID), GlobalId(WEAPON_ID)]);
    assert_eq!(found.not_indexed, 1);
    let weap = FourCc::from_code("weap").unwrap();
    assert_eq!(map_tags(b, base, Some(weap)).unwrap().tags.len(), 1);
}

/// PRD §6.6: the maps whose manifest in effect lists the tag.
#[test]
fn maps_of_a_tag() {
    let d = TestDeploy::with_map("maps-which");
    let b = &d.session.builds()[0];
    let found = which_maps(b, GlobalId(BULLET_ID));
    assert_eq!(module_paths(b, &found.modules), [UPDATE]);
    assert!(found.failed.is_empty());
    // The update dropped the weapon from the map's manifest.
    assert!(which_maps(b, GlobalId(WEAPON_ID)).modules.is_empty());
    assert!(which_maps(b, GlobalId(NOT_INDEXED_ID)).modules.len() == 1);
    assert_eq!(module_stack(UPDATE), MAP);
}
