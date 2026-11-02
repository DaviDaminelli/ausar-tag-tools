use super::*;
use crate::output::Format;
use crate::testdeploy::{SecondCopy, TestDeploy, WEAPON};

fn copies(d: &mut TestDeploy) -> String {
    TagCopies { tag: WEAPON.into(), build: BuildArg::default() }.run(&mut d.session).unwrap().render(Format::Text)
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
