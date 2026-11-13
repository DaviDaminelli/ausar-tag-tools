use super::*;
use crate::query::choose_copy;
use crate::testdeploy::{TEXT_ID, TestDeploy, TestTexts, WEAPON_ID};

const WEAPONS: TestTexts<'static> = &[
    ("mantis", [Some("Mantis"), Some("Mantis"), None]),
    ("battle_rifle", [Some("Battle Rifle"), Some("Kampfgewehr"), Some("Fusil de combat")]),
];
const PROMPTS: TestTexts<'static> = &[
    ("take_br", [Some("Take Battle Rifle"), None, None]),
    ("battle_rifle", [Some("Hold to take ally's Battle Rifle"), None, None]),
];

fn deploy(name: &str) -> TestDeploy {
    TestDeploy::with_texts(name, &[WEAPONS, PROMPTS])
}

fn tag(b: &Build, id: i32) -> &Tag {
    b.tags.iter().find(|t| t.global_id.0 == id).unwrap()
}

fn shown(t: TextAt<'_>) -> String {
    match t {
        TextAt::Text(s) => s.into(),
        TextAt::Missing => "(missing)".into(),
        TextAt::Unreadable => "(unreadable)".into(),
    }
}

/// What a search found, in order: (global ID, string_id, language, text).
fn rows(s: &TextSearch<'_>, languages: &[String]) -> Vec<(i32, String, String, String)> {
    s.hits
        .iter()
        .map(|h| {
            let name = s.string_id(h).text.clone().unwrap_or_default();
            (s.tag(h).global_id.0, name, languages[h.language].clone(), shown(s.text(h)))
        })
        .collect()
}

#[test]
fn the_languages_are_the_definitions_offset_fields() {
    let d = deploy("text-languages");
    assert_eq!(text_languages(&d.session.builds()[0]).unwrap(), ["english", "german", "french"]);
    let none = TestDeploy::new("text-no-definition");
    assert!(matches!(text_languages(&none.session.builds()[0]), Err(TextError::NoDefinition)));
}

#[test]
fn a_list_reads_each_string_id_and_its_texts() {
    let d = deploy("text-list");
    let b = &d.session.builds()[0];
    let t = tag(b, TEXT_ID);
    let list = read_text_list(b, t, choose_copy(b, t, None).unwrap()).unwrap();
    let names: Vec<_> = list.string_ids.iter().map(|s| s.text.as_deref()).collect();
    assert_eq!(names, [Some("mantis"), Some("battle_rifle")]);
    assert_eq!(list.text(0, 0), TextAt::Text("Mantis"));
    assert_eq!(list.text(1, 2), TextAt::Text("Fusil de combat"));
    assert_eq!(list.text(0, 2), TextAt::Missing);
}

#[test]
fn a_tag_of_another_class_is_not_a_text_list() {
    let d = deploy("text-not-unic");
    let b = &d.session.builds()[0];
    let t = tag(b, WEAPON_ID);
    assert!(matches!(read_text_list(b, t, t.copies[0]), Err(TextError::NotText)));
}

/// A string_id is matched by name, ignoring case, in every list; where the
/// lists give it different texts, `differing` names it once per language.
#[test]
fn a_string_id_is_found_in_every_list() {
    let d = deploy("text-by-id");
    let b = &d.session.builds()[0];
    let langs = text_languages(b).unwrap();
    let s = search_texts(b, Scope::Latest, &Lookup::StringId("BATTLE_RIFLE"), Some(0));
    assert_eq!(
        rows(&s, &langs),
        [
            (TEXT_ID, "battle_rifle".into(), "english".into(), "Battle Rifle".into()),
            (TEXT_ID + 1, "battle_rifle".into(), "english".into(), "Hold to take ally's Battle Rifle".into()),
        ]
    );
    assert_eq!((s.searched, s.skipped.len(), s.unreadable), (2, 0, 0));
    let differ: Vec<_> = s.differing().into_iter().map(|d| (d.id.text.clone(), d.languages, d.texts)).collect();
    assert_eq!(differ, [(Some("battle_rifle".to_owned()), vec![0], 2)]);
}

/// A string_id with different texts in several languages is one item, with
/// those languages; a language where the texts agree isn't among them.
#[test]
fn differing_texts_are_one_item_per_string_id() {
    let a: TestTexts<'_> = &[("x", [Some("One"), Some("Eins"), Some("Un")]), ("y", [Some("Y"), None, None])];
    let b: TestTexts<'_> = &[("x", [Some("One!"), Some("Eins!"), Some("Un")]), ("y", [Some("Y2"), None, None])];
    let three: TestTexts<'_> = &[("x", [Some("One?"), None, None])];
    let d = TestDeploy::with_texts("text-differing", &[a, b, three]);
    let s = search_texts(&d.session.builds()[0], Scope::Latest, &Lookup::Every, None);
    let differ: Vec<_> = s.differing().into_iter().map(|d| (d.id.text.clone(), d.languages, d.texts)).collect();
    assert_eq!(differ, [(Some("x".to_owned()), vec![0, 1], 3), (Some("y".to_owned()), vec![0], 2)]);
}

/// Every language: one row per language with a text; one language: a
/// missing text is a row too.
#[test]
fn every_language_lists_the_texts_there_are() {
    let d = deploy("text-all-languages");
    let b = &d.session.builds()[0];
    let langs = text_languages(b).unwrap();
    let all = search_texts(b, Scope::Latest, &Lookup::StringId("mantis"), None);
    assert_eq!(
        rows(&all, &langs),
        [
            (TEXT_ID, "mantis".into(), "english".into(), "Mantis".into()),
            (TEXT_ID, "mantis".into(), "german".into(), "Mantis".into()),
        ]
    );
    assert!(all.differing().is_empty());
    let french = search_texts(b, Scope::Latest, &Lookup::StringId("mantis"), Some(2));
    assert_eq!(rows(&french, &langs), [(TEXT_ID, "mantis".into(), "french".into(), "(missing)".into())]);
}

/// A term is looked for in the language searched, or in every language.
#[test]
fn a_term_matches_texts_in_the_languages_searched() {
    let d = deploy("text-term");
    let b = &d.session.builds()[0];
    let langs = text_languages(b).unwrap();
    let kampf = |t: &str| t.to_lowercase().contains("kampf");
    let lookup = Lookup::Term(&kampf);
    assert!(search_texts(b, Scope::Latest, &lookup, Some(0)).hits.is_empty());
    let expected = [(TEXT_ID, "battle_rifle".to_owned(), "german".to_owned(), "Kampfgewehr".to_owned())];
    assert_eq!(rows(&search_texts(b, Scope::Latest, &lookup, Some(1)), &langs), expected);
    assert_eq!(rows(&search_texts(b, Scope::Latest, &lookup, None), &langs), expected);
}

/// One list, every string_id: in one language, missing texts included.
#[test]
fn one_list_lists_all_its_string_ids() {
    let d = deploy("text-one-list");
    let b = &d.session.builds()[0];
    let langs = text_languages(b).unwrap();
    let t = tag(b, TEXT_ID + 1);
    let s = search_texts(b, Scope::One(t, choose_copy(b, t, None).unwrap()), &Lookup::Every, Some(1));
    assert_eq!(
        rows(&s, &langs),
        [
            (TEXT_ID + 1, "take_br".into(), "german".into(), "(missing)".into()),
            (TEXT_ID + 1, "battle_rifle".into(), "german".into(), "(missing)".into()),
        ]
    );
    assert_eq!(s.searched, 1);
}

/// Offsets past the data, a text without its NUL, bytes that aren't UTF-8:
/// unreadable, never an empty or partial text.
#[test]
fn a_text_that_cant_be_read_says_so() {
    let list = |offsets: Vec<i64>, data: &[u8]| TextList {
        string_ids: vec![StringIdValue { hash: 1, text: None }; offsets.len()],
        languages: 1,
        offsets,
        data: data.to_vec(),
    };
    assert_eq!(list(vec![9], b"ab\0").text(0, 0), TextAt::Unreadable);
    assert_eq!(list(vec![0], b"ab").text(0, 0), TextAt::Unreadable);
    assert_eq!(list(vec![0], b"\xff\0").text(0, 0), TextAt::Unreadable);
    assert_eq!(list(vec![1], b"ab\0").text(0, 0), TextAt::Text("b"));
}

/// The `unic` definition is found when no `unic` tag is indexed (D98).
#[test]
fn the_languages_need_no_unic_tag() {
    let defs = [("(unic)multilingual_unicode_string_list.xml", crate::testdeploy::TEXT_DEFINITION)];
    let d = TestDeploy::with_more_definitions("text-definition-only", &defs);
    let b = &d.session.builds()[0];
    assert_eq!(text_languages(b).unwrap(), ["english", "german", "french"]);
    let s = search_texts(b, Scope::Latest, &Lookup::StringId("x"), Some(0));
    assert_eq!((s.searched, s.hits.len()), (0, 0));
}
