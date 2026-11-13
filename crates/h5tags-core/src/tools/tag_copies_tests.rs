use super::*;
use crate::output::Format;
use crate::testdeploy::{SecondCopy, TestDeploy, WEAPON};

fn copies(d: &mut TestDeploy) -> String {
    TagCopies { tag: WEAPON.into(), path: None, build: BuildArg::default() }
        .run(&mut d.session)
        .unwrap()
        .render(Format::Text)
}

/// PRD §6.6: every copy, its module and sizes, and which copies hold the same data.
#[test]
fn copies_show_their_modules_and_versions() {
    let mut d = TestDeploy::with("copies-differ", SecondCopy::Changed);
    let text = copies(&mut d);
    let lines: Vec<&str> = text.lines().collect();
    // Each copy's layer, and the copy the other tools read (the latest layer, D58).
    assert_eq!(lines[0], r"total: 2  versions: 2  read by default: any\levels\test-rtx-11587-1.module");
    assert_eq!(lines[1], "module\tlayer\tdata\tasset checksum\tsize\tcompressed");
    assert!(lines[2].starts_with(r"any\levels\test-rtx-1.module	base	A	"), "{text}");
    assert!(lines[3].starts_with(r"any\levels\test-rtx-11587-1.module	11587-1	B	"), "{text}");

    let mut d = TestDeploy::with("copies-same", SecondCopy::Identical);
    let text = copies(&mut d);
    assert!(text.starts_with("total: 2  versions: 1  read by default: "), "{text}");
    assert!(text.lines().skip(2).all(|l| l.split('\t').nth(2) == Some("A")), "{text}");
}

fn field(d: &mut TestDeploy, path: &str) -> Result<String, ToolError> {
    let t = TagCopies { tag: WEAPON.into(), path: Some(path.into()), build: BuildArg::default() };
    t.run(&mut d.session).map(|o| o.render(Format::Text))
}

/// D102: path= gives that field's value in each copy, in layer order (base
/// first), written as query_field writes values.
#[test]
fn a_field_in_each_copy() {
    let mut d = TestDeploy::with("copies-field", SecondCopy::Changed);
    let text = field(&mut d, "count").unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], r"total: 2  versions: 2  read by default: any\levels\test-rtx-11587-1.module", "{text}");
    assert_eq!(lines[1], "module\tlayer\tdata\tvalue", "{text}");
    assert_eq!(lines[2], r"any\levels\test-rtx-1.module	base	A	0", "{text}");
    assert_eq!(lines[3], r"any\levels\test-rtx-11587-1.module	11587-1	B	9", "{text}");
}

/// A [*] path: one row per entry per copy, with its field path.
#[test]
fn every_entry_in_each_copy() {
    let mut d = TestDeploy::with("copies-entries", SecondCopy::Identical);
    let text = field(&mut d, "entries[*].target").unwrap();
    let paths: Vec<(&str, &str)> = text
        .lines()
        .skip(2)
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            Some((c.get(1).copied()?, c.get(3).copied()?))
        })
        .collect();
    assert_eq!(
        paths,
        [
            ("base", "entries[0].target"),
            ("base", "entries[1].target"),
            ("11587-1", "entries[0].target"),
            ("11587-1", "entries[1].target")
        ]
    );
    assert_eq!(text.lines().nth(1), Some("module\tlayer\tdata\tfield path\tvalue"), "{text}");
}

/// A path that isn't a value, or isn't in the definition, is an error, as in query_field.
#[test]
fn a_field_path_is_checked() {
    let mut d = TestDeploy::with("copies-field-errors", SecondCopy::Changed);
    assert!(field(&mut d, "nothing").unwrap_err().message.starts_with("no field 'nothing'"));
    assert_eq!(field(&mut d, "entries[0]").unwrap_err().message, "entries[0] is a tag block entry, not a value");
}
