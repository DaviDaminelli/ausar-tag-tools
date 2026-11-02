use super::*;
use crate::output::Format;
use crate::testdeploy::{BULLET, TestDeploy, WEAPON};

fn find(d: &mut TestDeploy, f: impl FnOnce(&mut FindReferences)) -> Result<String, ToolError> {
    let mut t = FindReferences {
        tag: WEAPON.into(),
        direction: Direction::Out,
        recursive: false,
        max_depth: 3,
        class: None,
        module: None,
        build: BuildArg::default(),
        page: PageArgs::default(),
    };
    f(&mut t);
    t.run(&mut d.session).map(|o| o.render(Format::Text))
}

/// PRD §6.6 `out`: every non-null reference with its field path, by class;
/// a target that isn't indexed by the name the tag stores (Q12).
#[test]
fn out_lists_what_a_tag_references() {
    let mut d = TestDeploy::new("refs-out");
    let text = find(&mut d, |_| {}).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("total: 2  by class: effe 1, proj 1  header scan built: 2 tags, "), "{text}");
    assert_eq!(lines[1], "field path\tclass\tglobal ID\tdisplay name");
    assert_eq!(lines[2], r"entries[1].target	effe	0x00402000	fx\impact\bullet_hit (not in indexed modules)");
    assert_eq!(lines[3], format!("projectile\tproj\t0x00401000\t{BULLET}"));
    // The scan is built once.
    assert!(!find(&mut d, |_| {}).unwrap().contains("header scan built"));
}

#[test]
fn in_lists_the_tags_pointing_here() {
    let mut d = TestDeploy::new("refs-in");
    let text = find(&mut d, |t| {
        t.tag = BULLET.into();
        t.direction = Direction::In;
    })
    .unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "class\tglobal ID\tdisplay name\tcopies\tcount");
    assert_eq!(lines[2], format!("weap\t0x00000010\t{WEAPON}\t1 of 1\t1"));
    // A copy that couldn't be scanned (the bullet's data is no tag) is said, not hidden.
    assert!(text.contains("\nhint: 1 tag copy couldn't be scanned"), "{text}");
}

#[test]
fn both_gives_two_parts() {
    let mut d = TestDeploy::new("refs-both");
    let text = find(&mut d, |t| t.direction = Direction::Both).unwrap();
    assert!(text.starts_with("## references out\n"), "{text}");
    assert!(text.contains("\n## references in\ntotal: 0"), "{text}");
}

/// `recursive`: the walk in tree order with depth and the tag pointing to it;
/// with `class=` only the matching rows, though the walk follows every class.
#[test]
fn recursive_walks_out_with_depth_and_via() {
    let mut d = TestDeploy::new("refs-walk");
    let text = find(&mut d, |t| t.recursive = true).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "depth\tclass\tglobal ID\tdisplay name\tvia");
    assert_eq!(lines[2], format!("1\tproj\t0x00401000\t{BULLET}\t{WEAPON}"));
    assert_eq!(lines[3], format!(r"1	effe	0x00402000	fx\impact\bullet_hit (not in indexed modules)	{WEAPON}"));
    let text = find(&mut d, |t| {
        t.recursive = true;
        t.class = Some("proj".into());
    })
    .unwrap();
    assert!(text.starts_with("total: 1\n"), "{text}");
    let err = find(&mut d, |t| {
        t.recursive = true;
        t.direction = Direction::In;
    })
    .unwrap_err();
    assert_eq!(err.message, "recursive=true works with direction=out only");
}
