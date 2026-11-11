use super::*;
use crate::output::Format;
use crate::testdeploy::{BULLET, TestDeploy, WEAPON};

fn args(tag: &str) -> ReadScript {
    ReadScript { tag: tag.into(), names: false, module: None, build: BuildArg::default(), page: PageArgs::default() }
}

fn read(d: &mut TestDeploy, tag: &str, limit: Option<usize>, format: Format) -> Result<String, ToolError> {
    let t = ReadScript { page: PageArgs { offset: 0, limit }, ..args(tag) };
    t.run(&mut d.session).map(|o| o.render(format))
}

const SCRIPT: &str = r"scripts\script_0.lua";

/// D85 step 4: a line under the tag line counts the tags the script names
/// by path (each path once), and names=true lists them, with the tags.
#[test]
fn the_tags_a_script_names() {
    let src = b"local a = 'objects\\weapons\\rifle\\ar\\bullet'\r\nlocal b = 'objects/weapons/rifle/ar/bullet'\r\nlocal c = 'sound\\missing\\thing'\r\n\0";
    let mut d = TestDeploy::with_scripts("read-script-names", &[src]);
    let text = read(&mut d, SCRIPT, None, Format::Text).unwrap();
    assert!(text.contains("\n  tag paths: 2 (1 indexed, 1 not in indexed modules); names=true lists them\n"), "{text}");
    let t = ReadScript { names: true, ..args(SCRIPT) };
    let text = t.run(&mut d.session).unwrap().render(Format::Text);
    let lines: Vec<&str> = text.lines().collect();
    assert!(!text.contains("names=true lists them"), "{text}");
    assert_eq!(
        lines[1..],
        [
            "total: 2  indexed: 1  not indexed: 1",
            "line\ttext\tclass\tglobal ID\tdisplay name",
            &format!("1\tobjects\\weapons\\rifle\\ar\\bullet\tproj\t0x00401000\t{BULLET}"),
            "3\tsound\\missing\\thing\t\t\t(not in indexed modules)",
        ]
    );
}

/// D85: the tag line, then the source lines, numbered and paged.
#[test]
fn source_lines_numbered_and_paged() {
    let mut d = TestDeploy::with_scripts("read-script-text", &[b"local a = 1\r\nreturn a\r\n-- end\r\n\0"]);
    let text = read(&mut d, SCRIPT, Some(2), Format::Text).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], format!("# hsc* {SCRIPT}  0x00000050  build=forge"));
    assert_eq!(lines[1], "total: 3  form: source text  size: 32 B");
    assert_eq!(lines[2..], ["line\ttext", "1\tlocal a = 1", "2\treturn a", "… 1 more (offset=2)"]);
    let json: serde_json::Value = serde_json::from_str(&read(&mut d, SCRIPT, Some(1), Format::Json).unwrap()).unwrap();
    assert_eq!(json["class"], "hsc*");
    assert_eq!(json["form"], "source text");
    assert_eq!(json["rows"], serde_json::json!([{ "line": 1, "text": "local a = 1" }]));
}

/// Source that isn't UTF-8 (3 copies per sample) says so above the lines.
#[test]
fn source_that_isnt_utf8_says_so() {
    let mut d = TestDeploy::with_scripts("read-script-latin", &[b"-- caf\xe9\r\n\0"]);
    let text = read(&mut d, SCRIPT, None, Format::Text).unwrap();
    assert!(text.contains("  encoding: not UTF-8 (other bytes shown as \u{fffd})\n"), "{text}");
    assert!(text.contains("\n1\t-- caf\u{fffd}\n"), "{text}");
}

/// Compiled code: its header, then the strings it holds, with a hint that
/// the code itself isn't decompiled.
#[test]
fn compiled_code_lists_its_strings() {
    let mut bytes = b"\x1bLua\x51\x0e\x00\x04\x08\x04\x04\x00\x03\x00\x00\x00".to_vec();
    for s in [r"@data\ui\menu.lua", "OnLoad"] {
        bytes.extend((s.len() as u64 + 1).to_be_bytes());
        bytes.extend(s.as_bytes());
        bytes.push(0);
    }
    let mut d = TestDeploy::with_scripts("read-script-compiled", &[&bytes]);
    let text = read(&mut d, SCRIPT, None, Format::Text).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[1],
        "total: 2  form: compiled  size: 57 B  header: 1b 4c 75 61 51 0e 00 04 08 04 04 00 03 00 00 00"
    );
    assert_eq!(lines[2..5], ["text", r"@data\ui\menu.lua", "OnLoad"]);
    assert!(lines[5].starts_with("hint: compiled Havok Script: "), "{text}");
}

#[test]
fn another_class_is_no_script() {
    let mut d = TestDeploy::with_scripts("read-script-weapon", &[b"return 1\0"]);
    let err = read(&mut d, WEAPON, None, Format::Text).unwrap_err();
    assert_eq!(err.message, "not a script tag: scripts are in hsc* (lua) and luas (lua_script) tags");
    assert!(err.hint.unwrap().contains("search_tags"));
}
