use super::*;
use crate::output::Format;
use crate::testdeploy::{TestDeploy, WEAPON};

fn search(d: &mut TestDeploy, term: &str, class: Option<&str>) -> Result<String, ToolError> {
    let t = SearchValues {
        term: term.into(),
        class: class.map(Into::into),
        build: BuildArg::default(),
        page: PageArgs::default(),
    };
    t.run(&mut d.session).map(|o| o.render(Format::Text))
}

/// PRD §6.6: tags whose string table (string_id names, referenced tag names)
/// contains the term, ignoring case, with the strings that matched.
#[test]
fn values_are_found_in_string_tables() {
    let mut d = TestDeploy::new("values-search");
    let text = search(&mut d, "BULLET", None).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("total: 1  header scan built: 2 tags, "), "{text}");
    assert_eq!(lines[1], "class\tglobal ID\tdisplay name\tmatched");
    assert_eq!(lines[2], format!(r"weap	0x00000010	{WEAPON}	fx\impact\bullet_hit, objects\weapons\rifle\ar\bullet"));
    assert!(search(&mut d, "prim", None).unwrap().contains("\tprimary\n"));
    // class= narrows by the tag's own class.
    assert!(search(&mut d, "bullet", Some("proj")).unwrap().starts_with("total: 0\n"));
}

#[test]
fn an_empty_result_says_what_string_tables_hold() {
    let mut d = TestDeploy::new("values-none");
    let text = search(&mut d, "zzz", None).unwrap();
    assert!(text.contains("\nhint: no tag's string table contains 'zzz'"), "{text}");
    assert!(search(&mut d, "  ", None).is_err());
}
