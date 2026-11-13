use super::*;
use crate::field_query::{Sort, query_field};
use crate::testdeploy::TestDeploy;

fn code(c: &str) -> FourCc {
    FourCc::from_code(c).unwrap()
}

fn parent(b: &Build) -> Arc<Definition> {
    load_class_definition(b, code("obje")).unwrap().unwrap()
}

/// D100: the classes with tags whose definition has a section named after
/// the parent (`OBJECT` for `object`); a class nothing inherits has none.
#[test]
fn the_classes_that_inherit_a_parent() {
    let d = TestDeploy::with_inheritance("inherit-classes");
    let b = &d.session.builds()[0];
    let found = inheriting_classes(b, code("obje"));
    assert_eq!(found.classes, [code("hsc*"), code("weap")]);
    assert!(found.unreadable.is_empty());
    assert!(inheriting_classes(b, code("weap")).classes.is_empty());
    let plain = TestDeploy::new("inherit-none");
    assert!(inheriting_classes(&plain.session.builds()[0], code("obje")).classes.is_empty());
}

/// A section label matches the class name ignoring case, and `_` or a space.
#[test]
fn a_section_label_names_its_class() {
    assert!(names_class("OBJECT", "object"));
    assert!(names_class("SOUND SCENERY", "sound_scenery"));
    assert!(!names_class("OBJECT", "objects"));
}

/// A parent's field that every class defines the same way: one path per class.
#[test]
fn an_inherited_field_in_each_class() {
    let d = TestDeploy::with_inheritance("inherit-field");
    let b = &d.session.builds()[0];
    let paths = inherited_field(b, &parent(b), &[code("hsc*"), code("weap")], "entries[*].target").unwrap();
    let classes: Vec<FourCc> = paths.iter().map(|p| p.class).collect();
    assert_eq!(classes, [code("hsc*"), code("weap")]);
}

/// A field the parent doesn't have is refused, naming the classes whose own
/// field it is; one a class defines otherwise is refused, naming it.
#[test]
fn fields_that_arent_inherited_alike() {
    let d = TestDeploy::with_inheritance("inherit-refused");
    let b = &d.session.builds()[0];
    let classes = [code("hsc*"), code("weap")];
    match inherited_field(b, &parent(b), &classes, "count") {
        Err(InheritError::NotInParent { classes, .. }) => assert_eq!(classes, [code("hsc*"), code("weap")]),
        other => panic!("{other:?}"),
    }
    match inherited_field(b, &parent(b), &classes, "projectile") {
        Err(InheritError::Differs(classes)) => assert_eq!(classes, [code("hsc*")]),
        other => panic!("{other:?}"),
    }
}

/// One query over the inherited field reads each class with its own
/// definition: rows grouped by class (code order), then by tag path.
#[test]
fn a_query_over_several_classes() {
    let d = TestDeploy::with_inheritance("inherit-query");
    let b = &d.session.builds()[0];
    let paths = inherited_field(b, &parent(b), &[code("hsc*"), code("weap")], "entries[*].target").unwrap();
    let rows = query_field(b, &paths, "entries[*].target", None, Sort::None).unwrap();
    let found: Vec<(String, usize)> = rows.rows.iter().map(|r| (r.tag.class.to_string(), r.selection)).collect();
    assert_eq!(found, [("hsc*".into(), 0), ("hsc*".into(), 1), ("weap".into(), 0), ("weap".into(), 1)]);
    assert!(rows.skipped.is_empty());
}
