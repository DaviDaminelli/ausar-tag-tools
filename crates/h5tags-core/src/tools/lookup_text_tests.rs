use super::*;
use crate::output::Format;
use crate::testdeploy::{TestDeploy, TestTexts};

const WEAPONS: TestTexts<'static> = &[
    ("mantis", [Some("Mantis"), Some("Mantis"), None]),
    ("battle_rifle", [Some("Battle Rifle"), Some("Kampfgewehr"), Some("Fusil de combat")]),
];
const PROMPTS: TestTexts<'static> = &[
    ("take_br", [Some("Take Battle Rifle"), None, None]),
    ("battle_rifle", [Some("Hold to take ally's Battle Rifle"), None, None]),
];

fn args() -> LookupText {
    LookupText {
        string_id: None,
        term: None,
        lang: None,
        tag: None,
        module: None,
        build: BuildArg::default(),
        page: PageArgs::default(),
    }
}

fn run(d: &mut TestDeploy, t: LookupText) -> Result<String, ToolError> {
    t.run(&mut d.session).map(|o| o.render(Format::Text))
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// D93: a string_id in every unic tag that has it, in tag order; the
/// string_id the tables disagree on is named in a hint.
#[test]
fn a_string_id_in_every_table() {
    let mut d = TestDeploy::with_texts("lookup-text-id", &[WEAPONS, PROMPTS]);
    let text = run(&mut d, LookupText { string_id: Some("BATTLE_RIFLE".into()), ..args() }).unwrap();
    assert_eq!(
        lines(&text),
        [
            "total: 2  tags: 2  searched: 2",
            r"in ui\strings\:",
            "global ID\tdisplay name\tstring id\ttext",
            "0x00000070\tlist_0.multilingual_unicode_string_list\tbattle_rifle\tBattle Rifle",
            "0x00000071\tlist_1.multilingual_unicode_string_list\tbattle_rifle\tHold to take ally's Battle Rifle",
            "hint: string_ids with different texts in different unic tags: battle_rifle (2 english texts); \
             the files don't say which table a given tag's text comes from",
        ]
    );
    let french = run(&mut d, LookupText { string_id: Some("mantis".into()), lang: Some("French".into()), ..args() });
    assert!(french.unwrap().ends_with("\tmantis\t(none)\n"));
}

/// A term in the language searched; lang=all searches every language and
/// adds the language column.
#[test]
fn a_term_in_one_language_or_all() {
    let mut d = TestDeploy::with_texts("lookup-text-term", &[WEAPONS, PROMPTS]);
    let english = run(&mut d, LookupText { term: Some("kampf".into()), ..args() }).unwrap();
    assert!(english.starts_with("total: 0  tags: 0  searched: 2\n"), "{english}");
    assert!(english.contains("\nhint: no english text holds 'kampf'; lang=all searches every language"), "{english}");
    let all = run(&mut d, LookupText { term: Some("kampf".into()), lang: Some("all".into()), ..args() }).unwrap();
    assert_eq!(
        lines(&all),
        [
            "total: 1  tags: 1  searched: 2",
            "global ID\tdisplay name\tstring id\tlanguage\ttext",
            r"0x00000070	ui\strings\list_0.multilingual_unicode_string_list	battle_rifle	german	Kampfgewehr",
        ]
    );
}

/// Line breaks and tabs are escaped in text, so a row stays one line, and
/// kept in JSON.
#[test]
fn line_breaks_are_escaped_in_text_only() {
    let list: TestTexts<'_> = &[("prompt", [Some("Hold\r\nto\ttake\nit"), None, None])];
    let mut d = TestDeploy::with_texts("lookup-text-breaks", &[list]);
    let t = LookupText { string_id: Some("prompt".into()), ..args() };
    let text = t.run(&mut d.session).unwrap();
    assert!(text.render(Format::Text).ends_with("\tprompt\tHold\\nto\\ttake\\nit\n"));
    let json: serde_json::Value = serde_json::from_str(&text.render(Format::Json)).unwrap();
    assert_eq!(json["rows"][0]["text"], "Hold\r\nto\ttake\nit");
    assert_eq!(json["rows"][0]["string_id"], "prompt");
}

/// tag= alone lists that tag's texts (missing ones too, in one language);
/// the latest layer by default, module= another; hits from a tag with other
/// copies say so.
#[test]
fn one_table_and_its_layers() {
    let updated: TestTexts<'_> = &[("mantis", [Some("Mantis Mk II"), None, None])];
    let mut d = TestDeploy::with_text_layers("lookup-text-layers", &[WEAPONS, PROMPTS], updated);
    let latest = run(&mut d, LookupText { string_id: Some("mantis".into()), ..args() }).unwrap();
    assert!(latest.contains("\tmantis\tMantis Mk II\n"), "{latest}");
    assert!(latest.contains("\nhint: unic tags listed with copies in other layers (they differ): 1;"), "{latest}");
    let base = LookupText {
        tag: Some("0x00000070".into()),
        module: Some("test-rtx-1.module".into()),
        lang: Some("german".into()),
        ..args()
    };
    let base = run(&mut d, base).unwrap();
    assert!(base.starts_with("total: 2  tags: 1  searched: 1\n"), "{base}");
    assert!(base.contains("\tmantis\tMantis\n") && base.contains("\tbattle_rifle\tKampfgewehr\n"), "{base}");
    assert!(base.contains("\nhint: "), "{base}");
}

#[test]
fn arguments_that_dont_fit() {
    let mut d = TestDeploy::with_texts("lookup-text-args", &[WEAPONS]);
    let both = LookupText { string_id: Some("a".into()), term: Some("b".into()), ..args() };
    assert_eq!(run(&mut d, both).unwrap_err().message, "pass string_id= or term=, not both");
    assert_eq!(
        run(&mut d, LookupText { term: Some("  ".into()), ..args() }).unwrap_err().message,
        "pass string_id= or term="
    );
    let module = LookupText { term: Some("a".into()), module: Some("x".into()), ..args() };
    assert_eq!(run(&mut d, module).unwrap_err().message, "module= needs tag=");
    let lang = run(&mut d, LookupText { term: Some("a".into()), lang: Some("klingon".into()), ..args() }).unwrap_err();
    assert_eq!(lang.message, "no language 'klingon'");
    assert_eq!(lang.hint.as_deref(), Some("languages: english, german, french, or all"));
    let weapon = run(&mut d, LookupText { tag: Some("0x00000010".into()), ..args() }).unwrap_err();
    assert!(weapon.message.starts_with("not a text tag"), "{}", weapon.message);
}
