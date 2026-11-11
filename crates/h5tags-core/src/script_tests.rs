use super::*;
use crate::query::choose_copy;
use crate::testdeploy::{BULLET_ID, SCRIPT_ID, TestDeploy, WEAPON_ID};

/// Each name found: (line, text as written, global IDs of the tags it names).
fn found(b: &Build, script: &Script) -> Vec<(Option<usize>, String, Vec<i32>)> {
    tag_names(b, script).into_iter().map(|n| (n.line, n.text, n.tags.iter().map(|t| t.global_id.0).collect())).collect()
}

/// D85 step 4: string literals that look like tag paths, looked up exactly:
/// single or escaped backslashes, `/`, a class suffix or none; comments and
/// strings with spaces skipped; a name no indexed tag has listed without tags.
#[test]
fn source_names_tags_by_path() {
    let d = TestDeploy::with_scripts("script-names", &[]);
    let b = &d.session.builds()[0];
    let src = concat!(
        "-- 'objects\\weapons\\rifle\\ar\\bullet' in a comment\r\n",
        "local a = 'objects\\weapons\\rifle\\ar\\bullet'\r\n",
        "local b = \"objects\\\\test\\\\test.weapon\" --[[ 'objects\\test\\test' ]]\r\n",
        "local c = 'objects/weapons/rifle/ar/bullet'; local s = 'it\\'s'\r\n",
        "local d = 'sound\\missing\\thing'\r\n",
        "local e = 'two words\\here'\r\n",
    );
    let script = parse(format!("{src}\0").as_bytes());
    assert_eq!(
        found(b, &script),
        [
            (Some(2), r"objects\weapons\rifle\ar\bullet".into(), vec![BULLET_ID]),
            (Some(3), r"objects\\test\\test.weapon".into(), vec![WEAPON_ID]),
            (Some(4), "objects/weapons/rifle/ar/bullet".into(), vec![BULLET_ID]),
            (Some(5), r"sound\missing\thing".into(), vec![]),
        ]
    );
}

/// Compiled code names other scripts with a leading `\` and the `.lua`
/// source extension; its `@…` source file name isn't a tag name.
#[test]
fn compiled_strings_name_scripts() {
    let d = TestDeploy::with_scripts("script-names-compiled", &[b"return 1\0"]);
    let b = &d.session.builds()[0];
    let script =
        parse(&compiled(&[r"@data\scripts\script_0.lua", r"\scripts\script_0", r"\scripts\script_0.lua", "OnLoad"]));
    assert_eq!(
        found(b, &script),
        [
            (None, r"\scripts\script_0".into(), vec![SCRIPT_ID]),
            (None, r"\scripts\script_0.lua".into(), vec![SCRIPT_ID])
        ]
    );
}

/// A compiled script as the survey found them stored (format-notes "Scripts"):
/// the 16 header bytes, then strings as an 8-byte big-endian length (bytes +
/// NUL), the bytes and a NUL, among other bytes.
fn compiled(strings: &[&str]) -> Vec<u8> {
    let mut out = b"\x1bLua\x51\x0e\x00\x04\x08\x04\x04\x00\x03\x00\x00\x00".to_vec();
    for s in strings {
        out.extend([0x02, 0x00, 0x01]); // bytes that aren't a string
        out.extend((s.len() as u64 + 1).to_be_bytes());
        out.extend(s.as_bytes());
        out.push(0);
    }
    out
}

#[test]
fn source_text_without_its_trailing_nul() {
    let s = parse(b"local a = 'x'\r\nreturn a\r\n\0");
    assert_eq!(s.size, 26);
    assert_eq!(s.form, Form::Text { text: "local a = 'x'\r\nreturn a\r\n".into(), not_utf8: false });
}

/// Three source copies in each sample aren't UTF-8: shown with replacement
/// characters, and said so.
#[test]
fn source_text_that_isnt_utf8() {
    let s = parse(b"-- caf\xe9\r\n\0");
    assert_eq!(s.form, Form::Text { text: "-- caf\u{fffd}\r\n".into(), not_utf8: true });
}

#[test]
fn compiled_code_its_header_and_strings() {
    let bytes = compiled(&[r"@data\ui\wpf\menu.lua", r"\ui\wpf\anubisui\luatemplates\menuscreen", "OnLoad"]);
    let s = parse(&bytes);
    let Form::Compiled { header, strings } = s.form else { panic!("{:?}", s.form) };
    assert_eq!(header, bytes[..16]);
    assert_eq!(strings, [r"@data\ui\wpf\menu.lua", r"\ui\wpf\anubisui\luatemplates\menuscreen", "OnLoad"]);
}

/// A script tag's script comes from its class's data field, read through
/// the definition; another class has none.
#[test]
fn a_script_tags_script() {
    let d = TestDeploy::with_scripts("script-read", &[b"return 1\r\n\0"]);
    let b = &d.session.builds()[0];
    let tag = b.tag_by_id(h5_module::GlobalId(SCRIPT_ID)).unwrap();
    let s = read_script(b, tag, choose_copy(b, tag, None).unwrap()).unwrap();
    assert_eq!(s.form, Form::Text { text: "return 1\r\n".into(), not_utf8: false });
    let weapon = b.tag_by_id(h5_module::GlobalId(WEAPON_ID)).unwrap();
    let err = read_script(b, weapon, weapon.copies[0]).unwrap_err();
    assert!(matches!(err, ScriptError::NotAScript), "{err}");
}
