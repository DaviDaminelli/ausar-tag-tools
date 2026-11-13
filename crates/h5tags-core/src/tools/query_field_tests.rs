use super::*;
use crate::output::Format;
use crate::testdeploy::{TestDeploy, WEAPON};

/// The test weapon (count 0), three more (36, 12, 50) and one that can't be read.
fn deploy(name: &str) -> TestDeploy {
    TestDeploy::with_weapons(name, &[Some(36), Some(12), Some(50), None])
}

fn query(d: &mut TestDeploy, path: &str, f: impl FnOnce(&mut QueryField)) -> Result<String, ToolError> {
    let mut t = QueryField {
        class: "weap".into(),
        path: path.into(),
        filter: None,
        sort: Sort::None,
        also: Vec::new(),
        build: BuildArg::default(),
        page: PageArgs::default(),
    };
    f(&mut t);
    t.run(&mut d.session).map(|o| o.render(Format::Text))
}

fn rows(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.starts_with("objects\\")).collect()
}

/// PRD §6.6: one row per tag, in path order; tags that can't be read are
/// listed, not dropped.
#[test]
fn one_field_across_a_class() {
    let mut d = deploy("query-plain");
    let text = query(&mut d, "count", |_| {}).unwrap();
    assert!(text.starts_with("total: 4\ndisplay name\tvalue\n"), "{text}");
    assert_eq!(
        rows(&text),
        [
            r"objects\test\more_0.weapon	36",
            r"objects\test\more_1.weapon	12",
            r"objects\test\more_2.weapon	50",
            &format!("{WEAPON}\t0")
        ]
    );
    assert!(
        text.contains(r"hint: 1 tag skipped: objects\test\more_3.weapon (any\levels\test-rtx-1.module: tag header:"),
        "{text}"
    );
}

#[test]
fn filters_and_sorting() {
    let mut d = deploy("query-filter");
    let text = query(&mut d, "count", |t| {
        t.filter = Some(">= 12".into());
        t.sort = Sort::Desc;
    })
    .unwrap();
    assert_eq!(
        rows(&text),
        [r"objects\test\more_2.weapon	50", r"objects\test\more_0.weapon	36", r"objects\test\more_1.weapon	12"]
    );
    let err = query(&mut d, "count", |t| t.filter = Some("contains 3".into())).unwrap_err();
    assert_eq!(err.message, "count is a number: contains doesn't apply to it");
    assert_eq!(err.hint.as_deref(), Some("for a number: == != > >= < <= <number>, is set, is empty"));
}

/// D68 step 1: `!= (none)` on a tag reference kept every row (a null
/// reference has no name, so it differs from any text). Each test weapon's
/// `entries[0].target` is null, `entries[1].target` isn't.
#[test]
fn none_is_a_null_reference() {
    let mut d = deploy("query-none");
    let set = query(&mut d, "entries[*].target", |t| t.filter = Some("!= (none)".into())).unwrap();
    assert!(set.starts_with("total: 4\n"), "{set}");
    assert!(rows(&set).iter().all(|r| r.contains("\tentries[1].target\t")), "{set}");
    assert_eq!(query(&mut d, "entries[*].target", |t| t.filter = Some("is set".into())).unwrap(), set);
    let null = query(&mut d, "entries[*].target", |t| t.filter = Some("== (none)".into())).unwrap();
    assert!(null.starts_with("total: 4\n"), "{null}");
    assert!(rows(&null).iter().all(|r| r.contains("\tentries[0].target\t")), "{null}");
}

/// D69: `is set` / `is empty` on a number (zero is empty) and on data.
#[test]
fn is_set_and_is_empty() {
    let mut d = deploy("query-set");
    let empty = query(&mut d, "count", |t| t.filter = Some("is empty".into())).unwrap();
    assert_eq!(rows(&empty), [format!("{WEAPON}\t0")]);
    let set = query(&mut d, "count", |t| t.filter = Some("is set".into())).unwrap();
    assert!(set.starts_with("total: 3\n"), "{set}");
    let data = query(&mut d, "data", |t| t.filter = Some("is set".into())).unwrap();
    assert!(data.starts_with("total: 4\n"), "{data}");
    let none = query(&mut d, "data", |t| t.filter = Some("is empty".into())).unwrap();
    assert!(none.starts_with("total: 0\n"), "{none}");
    let err = query(&mut d, "pad", |t| t.filter = Some("is set".into())).unwrap_err();
    assert_eq!(err.message, "pad is padding: a filter doesn't apply to it");
}

/// `[*]`: one row per entry, with its field path; tag references by name.
#[test]
fn every_entry_with_its_path() {
    let mut d = deploy("query-entries");
    let text = query(&mut d, "entries[*].target", |t| t.filter = Some("contains bullet_hit".into())).unwrap();
    assert!(text.starts_with("total: 4\ndisplay name\tfield path\tvalue\n"), "{text}");
    assert!(rows(&text)[0].starts_with(r"objects\test\more_0.weapon	entries[1].target	fx\impact\bullet_hit"), "{text}");
}

/// An entry of a tag block isn't one value: the error says which fields to query.
#[test]
fn a_path_must_end_at_a_value() {
    let mut d = deploy("query-struct");
    let err = query(&mut d, "entries[*]", |_| {}).unwrap_err();
    assert_eq!(err.message, "entries[*] is a tag block entry, not a value");
    assert_eq!(
        err.hint.as_deref(),
        Some("query one of its fields: entries[*].<field> (outline or get_definition lists them)")
    );
}

/// D75: a tag block without an index gives each tag's entry count, a number
/// (filter and sort work on it).
#[test]
fn a_tag_block_gives_its_entry_count() {
    let mut d = deploy("query-count");
    let text = query(&mut d, "entries", |t| t.filter = Some(">= 2".into())).unwrap();
    assert!(text.starts_with("total: 4\ndisplay name\tvalue\n"), "{text}");
    assert_eq!(rows(&text)[0], r"objects\test\more_0.weapon	2");
    assert!(query(&mut d, "entries", |t| t.filter = Some("> 2".into())).unwrap().starts_with("total: 0\n"));
}

/// D75: `also` paths, one column each (named by the path), from the same read;
/// a `[*]` the main path has takes the row's entry. Filter and sort stay on `path`.
#[test]
fn also_shows_more_fields_per_row() {
    let mut d = deploy("query-also");
    let text = query(&mut d, "count", |t| {
        t.also = vec!["entries".into(), "projectile".into()];
        t.sort = Sort::Desc;
    })
    .unwrap();
    assert!(text.starts_with("total: 4\ndisplay name\tvalue\tentries\tprojectile\n"), "{text}");
    assert!(
        rows(&text)[0].starts_with(r"objects\test\more_2.weapon	50	2	objects\weapons\rifle\ar\bullet.projectile"),
        "{text}"
    );

    let text =
        query(&mut d, "entries[*].target", |t| t.also = vec!["count".into(), "entries[*].target".into()]).unwrap();
    assert!(text.starts_with("total: 8\ndisplay name\tfield path\tvalue\tcount\tentries[*].target\n"), "{text}");
    assert_eq!(rows(&text)[0], r"objects\test\more_0.weapon	entries[0].target	(none) []	36	(none) []");
    // entries[1]'s also value is entries[1]'s, not entries[0]'s.
    let second: Vec<&str> = rows(&text)[1].split('\t').collect();
    assert_eq!((second[1], second[3]), ("entries[1].target", "36"), "{text}");
    assert!(second[2].starts_with(r"fx\impact\bullet_hit") && second[4] == second[2], "{text}");
}

/// D75: an `also` path with a `[*]` the main path doesn't have has no one value per row.
#[test]
fn also_needs_the_rows_entries() {
    let mut d = deploy("query-also-star");
    let err = query(&mut d, "count", |t| t.also = vec!["entries[*].target".into()]).unwrap_err();
    assert_eq!(err.message, "also: 'entries[*].target' has a [*] that 'count' doesn't have");
    assert_eq!(err.hint.as_deref(), Some("give it an index (entries[0]), or put the same [*] in path"));
    let err = query(&mut d, "count", |t| t.also = vec!["nothing".into()]).unwrap_err();
    assert!(err.message.contains("nothing"), "{}", err.message);
}

/// A class with a definition and no tags here (a parent such as `object`):
/// no rows, with the hint that its tags may be in modules not indexed, never
/// "no definition" (D98).
#[test]
fn a_class_with_only_a_definition_has_no_tags_here() {
    let object = "<plugin>\n\t<_field_custom name=\"OBJECT\" />\n\t<_field_long_integer name=\"count\" />\n</plugin>";
    let mut d = TestDeploy::with_more_definitions("query-definition-only", &[("(obje)object.xml", object)]);
    for class in ["obje", "object"] {
        let text = query(&mut d, "count", |t| t.class = class.into()).unwrap();
        assert!(text.starts_with("total: 0\n"), "{text}");
        assert!(
            text.contains("\nhint: no tags of class 'obje' (object) in the indexed modules of build 'forge'. The class exists (it has a definition)"),
            "{text}"
        );
    }
}

/// D101: a parent class gives the field in every class inheriting it: a class
/// column, rows by class, the classes named in the summary.
#[test]
fn a_parent_class_queries_every_class_inheriting_it() {
    let mut d = TestDeploy::with_inheritance("query-parent");
    let text = query(&mut d, "entries[*].target", |t| t.class = "object".into()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "total: 4  inherited by: hsc*, weap  by class: hsc* 2, weap 2", "{text}");
    assert_eq!(lines[1], "class\tdisplay name\tfield path\tvalue", "{text}");
    let rows: Vec<(&str, &str)> =
        lines[2..].iter().filter_map(|l| l.split('\t').next().zip(l.split('\t').nth(2))).collect();
    assert_eq!(
        rows,
        [
            ("hsc*", "entries[0].target"),
            ("hsc*", "entries[1].target"),
            ("weap", "entries[0].target"),
            ("weap", "entries[1].target")
        ]
    );
    assert!(!text.contains("hint: no tags of class"), "{text}");
    // also= is read in each class with its own definition.
    let also = query(&mut d, "entries[*].target", |t| {
        t.class = "object".into();
        t.also = vec!["entries".into()];
    })
    .unwrap();
    assert!(also.lines().nth(1).unwrap().ends_with("\tvalue\tentries"), "{also}");
    assert!(also.lines().skip(2).all(|l| l.ends_with("\t2")), "{also}");
}

/// Rows past one page: a hint names the class most come from and the
/// narrower calls; JSON lists the classes (D101).
#[test]
fn a_parent_result_past_one_page_points_to_narrower_calls() {
    let mut d = TestDeploy::with_inheritance("query-parent-pages");
    let text = query(&mut d, "entries[*].target", |t| {
        t.class = "object".into();
        t.page.limit = Some(1);
    })
    .unwrap();
    assert!(
        text.contains(
            "\nhint: 4 rows, 2 of them from hsc*: narrow with filter=, sort= to mix the classes, \
             or ask one class (query_field(class=…)) instead of paging"
        ),
        "{text}"
    );
    let whole = query(&mut d, "entries[*].target", |t| t.class = "object".into()).unwrap();
    assert!(!whole.contains("instead of paging"), "{whole}");
    let t = QueryField {
        class: "object".into(),
        path: "entries[*].target".into(),
        filter: None,
        sort: Sort::None,
        also: Vec::new(),
        build: BuildArg::default(),
        page: PageArgs::default(),
    };
    let json: serde_json::Value = serde_json::from_str(&t.run(&mut d.session).unwrap().render(Format::Json)).unwrap();
    assert_eq!(json["inherited_by"], serde_json::json!(["hsc*", "weap"]));
}

/// A path the parent doesn't have, or that a class defines otherwise, is
/// refused, naming the classes; never merged.
#[test]
fn a_parent_refuses_fields_that_arent_inherited_alike() {
    let mut d = TestDeploy::with_inheritance("query-parent-refused");
    let own = query(&mut d, "count", |t| t.class = "object".into()).unwrap_err();
    assert_eq!(
        own.message,
        "'count' isn't a field of object, the parent; these classes each have a field of their own by that name \
         (not one shared field): hsc*, weap"
    );
    assert_eq!(
        own.hint.as_deref(),
        Some("query_field(class=hsc*) reads it; outline(class=object) lists object's fields")
    );
    let differs = query(&mut d, "projectile", |t| t.class = "object".into()).unwrap_err();
    assert_eq!(
        differs.message,
        "'projectile' isn't defined the same way in every class inheriting object: hsc* differs"
    );
    assert_eq!(differs.hint.as_deref(), Some("ask those classes one at a time: query_field(class=hsc*)"));
}
