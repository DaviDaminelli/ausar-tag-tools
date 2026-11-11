use super::*;
use crate::output::Format;
use crate::testdeploy::TestDeploy;

/// A compiled script holding `strings` (format-notes "Scripts").
fn compiled(strings: &[&str]) -> Vec<u8> {
    let mut out = b"\x1bLua\x51\x0e\x00\x04\x08\x04\x04\x00\x03\x00\x00\x00".to_vec();
    for s in strings {
        out.extend((s.len() as u64 + 1).to_be_bytes());
        out.extend(s.as_bytes());
        out.push(0);
    }
    out
}

fn search(d: &mut TestDeploy, term: &str, class: Option<&str>) -> Result<String, ToolError> {
    let t = SearchScripts {
        term: term.into(),
        class: class.map(Into::into),
        build: BuildArg::default(),
        page: PageArgs::default(),
    };
    t.run(&mut d.session).map(|o| o.render(Format::Text))
}

/// D85 step 5: every script's lines (source) or strings (compiled) holding
/// the term, case-insensitive; one row per line, indentation left out.
#[test]
fn lines_and_strings_holding_the_term() {
    let source = b"local x = 'Flyby'\r\n\tif x then return FLYBY_NEAR end\r\nreturn 0\r\n\0";
    let code = compiled(&["flyby_sound", "Other"]);
    let mut d = TestDeploy::with_scripts("search-scripts", &[source, &code, b"return 1\0"]);
    let text = search(&mut d, "flyby", None).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        [
            "total: 3  scripts: 2  searched: 3",
            r"in scripts\:",
            "class\tglobal ID\tdisplay name\tline\ttext",
            "hsc*\t0x00000050\tscript_0.lua\t1\tlocal x = 'Flyby'",
            "hsc*\t0x00000050\tscript_0.lua\t2\tif x then return FLYBY_NEAR end",
            "hsc*\t0x00000051\tscript_1.lua\t\tflyby_sound",
        ]
    );
    let none = search(&mut d, "nowhere", Some("lua")).unwrap();
    assert!(none.starts_with("total: 0  scripts: 0  searched: 3\n"), "{none}");
    assert!(none.contains("\nhint: no script holds 'nowhere'"), "{none}");
}

#[test]
fn only_script_classes_and_a_term() {
    let mut d = TestDeploy::with_scripts("search-scripts-args", &[b"return 1\0"]);
    let err = search(&mut d, "x", Some("weap")).unwrap_err();
    assert_eq!(err.message, "weap isn't a script class: scripts are in hsc* (lua) and luas (lua_script) tags");
    assert!(search(&mut d, "  ", None).is_err());
}
