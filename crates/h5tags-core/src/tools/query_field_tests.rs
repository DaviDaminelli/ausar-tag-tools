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

#[test]
fn a_path_must_end_at_a_value() {
    let mut d = deploy("query-struct");
    let err = query(&mut d, "entries", |_| {}).unwrap_err();
    assert_eq!(err.message, "entries is a tag block, not a value");
    assert_eq!(
        err.hint.as_deref(),
        Some("query one of its fields: entries[*].<field> (outline or get_definition lists them)")
    );
}
