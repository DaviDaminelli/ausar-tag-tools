use super::*;
use crate::output::Format;
use crate::testdeploy::{BULLET, TestDeploy, WEAPON};

const UPDATE: &str = r"any\levels\menu\menu-rtx-11587-1.module";

fn map_tags(d: &mut TestDeploy, module: &str, class: Option<&str>) -> Result<String, ToolError> {
    let t = MapTags {
        module: module.into(),
        class: class.map(Into::into),
        build: BuildArg::default(),
        page: PageArgs::default(),
    };
    t.run(&mut d.session).map(|o| o.render(Format::Text))
}

fn which_maps(d: &mut TestDeploy, tag: &str) -> String {
    which_maps_in(d, tag, |_| {})
}

fn which_maps_in(d: &mut TestDeploy, tag: &str, f: impl FnOnce(&mut WhichMaps)) -> String {
    let mut t = WhichMaps { tag: tag.into(), module: None, build: BuildArg::default(), page: PageArgs::default() };
    f(&mut t);
    t.run(&mut d.session).unwrap().render(Format::Text)
}

/// D76: `module=` and paging, as `map_tags` has them.
#[test]
fn which_maps_in_some_modules_by_pages() {
    let mut d = TestDeploy::with_map("tool-which-maps-module");
    let text = which_maps_in(&mut d, BULLET, |t| t.module = Some("menu".into()));
    assert!(text.starts_with("total: 1\n") && text.contains(&format!("{UPDATE}\t11587-1\t2")), "{text}");
    let text = which_maps_in(&mut d, BULLET, |t| t.module = Some("warzone".into()));
    assert!(text.starts_with("total: 0\n"), "{text}");
    assert!(text.contains("hint: no map's module path contains 'warzone'"), "{text}");
    let text = which_maps_in(&mut d, BULLET, |t| t.page = PageArgs { offset: 1, limit: Some(5) });
    assert!(text.starts_with("total: 1\n") && !text.contains(UPDATE), "{text}");
}

/// PRD §6.6, D59: the latest layer's manifest, its tags by class, and how many
/// of its IDs aren't indexed tags.
#[test]
fn the_tags_a_map_loads() {
    let mut d = TestDeploy::with_map("tool-map-tags");
    let text = map_tags(&mut d, "menu", None).unwrap();
    let first = text.lines().next().unwrap();
    // One class: no `by class` breakdown (present::by_class).
    assert_eq!(first, format!("total: 1  manifest: {UPDATE}  not in indexed modules: 1"));
    assert!(text.contains("global ID\tclass\tdisplay name"), "{text}");
    assert!(text.contains("0x00401000\tproj"), "{text}");
    // Naming the base module reads its own manifest, which still lists the weapon.
    let text = map_tags(&mut d, "menu-rtx-1.module", Some("weap")).unwrap();
    assert!(text.starts_with("total: 1  manifest: any\\levels\\menu\\menu-rtx-1.module"), "{text}");
    assert!(text.contains(WEAPON), "{text}");
}

#[test]
fn a_module_without_a_manifest_is_an_error() {
    let mut d = TestDeploy::with_map("tool-map-none");
    let err = map_tags(&mut d, "test-rtx", None).unwrap_err();
    assert_eq!(err.message, "no module matching 'test-rtx' has a load manifest");
    assert!(err.hint.unwrap().contains("list_modules"));
}

/// PRD §6.6: the maps whose manifest in effect lists the tag; none says how
/// many were looked at.
#[test]
fn the_maps_that_load_a_tag() {
    let mut d = TestDeploy::with_map("tool-which-maps");
    let text = which_maps(&mut d, BULLET);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "total: 1");
    assert_eq!(lines[1], "module\tlayer\ttags");
    assert_eq!(lines[2], format!("{UPDATE}\t11587-1\t2"));
    // The update dropped the weapon from the manifest.
    let text = which_maps(&mut d, WEAPON);
    assert!(text.starts_with("total: 0"), "{text}");
    assert!(text.contains("hint: the load manifest in effect of 1 map doesn't list this tag"), "{text}");
}
