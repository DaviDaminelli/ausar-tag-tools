//! In-game text (M8, decisions D92, D93): the texts a `unic`
//! (multilingual_unicode_string_list) tag holds, by string_id and language
//! (format-notes "In-game text"). Domain only.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use h5_defs::definition::Definition;
use h5_meta::{Body, Content, StringIdValue, Value};
use h5_module::FourCc;

use crate::catalog::{Build, Copy, Tag};
use crate::query::choose_copy;
use crate::stored::{DefinitionLoadError, ReadError, load_class_definition, read_stored_tag};

/// The block of string references: a string_id and one offset per language.
const REFERENCES: &str = "string references";
/// The data the offsets point into: NUL-terminated UTF-8 texts.
const DATA: &str = "string data utf8";
/// A reference's field `<language> offset`.
const OFFSET: &str = " offset";
/// The offset of a language without a text (the only negative value, format-notes).
const NO_TEXT: i64 = -1;

/// The class holding in-game texts.
pub fn text_class() -> FourCc {
    FourCc::from_code("unic").expect("a four-character code")
}

#[derive(Debug, thiserror::Error)]
pub enum TextError {
    #[error("not a text tag: in-game texts are in unic (multilingual_unicode_string_list) tags")]
    NotText,
    #[error("no definition for class unic")]
    NoDefinition,
    #[error(transparent)]
    Definition(#[from] DefinitionLoadError),
    #[error(transparent)]
    Read(#[from] ReadError),
    /// The definition lacks the field, or the tag's field wasn't read.
    #[error("the definition's {field} isn't a field that could be read")]
    Field { field: &'static str },
}

/// The language a reference field holds the offset of (`english offset` → `english`).
fn language(field: &str) -> Option<&str> {
    field.strip_suffix(OFFSET)
}

/// The languages of `def`'s string references, in their order.
fn languages(def: &Definition) -> Result<impl Iterator<Item = &str>, TextError> {
    let block =
        def.fields.iter().find(|f| f.name.display == REFERENCES).ok_or(TextError::Field { field: REFERENCES })?;
    Ok(block.children.iter().filter_map(|f| language(&f.name.display)))
}

/// The languages `b`'s `unic` definition names, in its order: a language's
/// position is the `language` of [`TextList::text`] and [`search_texts`].
pub fn text_languages(b: &Build) -> Result<Vec<String>, TextError> {
    let def = load_class_definition(b, text_class())?.ok_or(TextError::NoDefinition)?;
    Ok(languages(&def)?.map(str::to_owned).collect())
}

/// One `unic` copy's string references and texts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextList {
    /// In stored order.
    pub string_ids: Vec<StringIdValue>,
    /// Offsets per string_id.
    languages: usize,
    /// Each string_id's offsets, one per language, back to back.
    offsets: Vec<i64>,
    data: Vec<u8>,
}

/// A string_id's text in one language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAt<'a> {
    Text(&'a str),
    /// No text in this language.
    Missing,
    /// The offset is outside the data, the text has no NUL, or it isn't UTF-8.
    Unreadable,
}

impl TextList {
    /// String_id `entry`'s text in `language` (positions from 0).
    pub fn text(&self, entry: usize, language: usize) -> TextAt<'_> {
        if language >= self.languages {
            return TextAt::Missing;
        }
        let Some(&offset) = self.offsets.get(entry * self.languages + language) else { return TextAt::Missing };
        if offset == NO_TEXT {
            return TextAt::Missing;
        }
        let Some(rest) = usize::try_from(offset).ok().and_then(|at| self.data.get(at..)) else {
            return TextAt::Unreadable;
        };
        let Some(end) = rest.iter().position(|&c| c == 0) else { return TextAt::Unreadable };
        std::str::from_utf8(&rest[..end]).map_or(TextAt::Unreadable, TextAt::Text)
    }
}

/// The string references and texts of `tag`'s copy `copy`.
pub fn read_text_list(b: &Build, tag: &Tag, copy: Copy) -> Result<TextList, TextError> {
    if tag.class != text_class() {
        return Err(TextError::NotText);
    }
    let def = load_class_definition(b, tag.class)?.ok_or(TextError::NoDefinition)?;
    let languages = languages(&def)?.count();
    let stored = read_stored_tag(b, copy)?;
    let read = h5_meta::read_tag(&def, &stored.header, &stored.tag_data);
    let (mut entries, mut target) = (None, None);
    for node in read.root.children {
        match (node.name, node.content) {
            (Some(REFERENCES), Content::Block(block)) if block.problem.is_none() => entries = Some(block.entries),
            (Some(DATA), Content::Value(Value::Data(d))) => target = Some(d.target),
            _ => {}
        }
    }
    let entries = entries.ok_or(TextError::Field { field: REFERENCES })?;
    let data = match target.ok_or(TextError::Field { field: DATA })? {
        Some(t) => stored.header.block_data(&stored.tag_data, t).ok_or(TextError::Field { field: DATA })?.to_vec(),
        None => Vec::new(),
    };
    let mut list = TextList {
        string_ids: Vec::with_capacity(entries.len()),
        languages,
        offsets: Vec::with_capacity(entries.len() * languages),
        data,
    };
    for entry in entries {
        list.push(entry)?;
    }
    Ok(list)
}

impl TextList {
    /// Adds one string reference: its first string_id and its offsets.
    fn push(&mut self, entry: Body<'_>) -> Result<(), TextError> {
        let (mut id, before) = (None, self.offsets.len());
        for node in entry.children {
            match node.content {
                Content::Value(Value::StringId(s)) if id.is_none() => id = Some(s),
                Content::Value(Value::Int(i)) if node.name.and_then(language).is_some() => self.offsets.push(i),
                _ => {}
            }
        }
        match id {
            Some(id) if self.offsets.len() - before == self.languages => {
                self.string_ids.push(id);
                Ok(())
            }
            _ => Err(TextError::Field { field: REFERENCES }),
        }
    }
}

/// Which `unic` copies [`search_texts`] reads.
#[derive(Clone, Copy, Debug)]
pub enum Scope<'b> {
    /// Every `unic` tag, its copy read by default (D58).
    Latest,
    One(&'b Tag, Copy),
}

/// What [`search_texts`] looks for.
pub enum Lookup<'q> {
    /// A string_id by name, ignoring case.
    StringId(&'q str),
    /// Texts this accepts.
    Term(&'q dyn Fn(&str) -> bool),
    /// Every string_id.
    Every,
}

/// A string_id's text in one language that a search found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextHit {
    list: usize,
    pub entry: usize,
    /// Position in [`text_languages`].
    pub language: usize,
}

/// What [`search_texts`] found.
#[derive(Debug, Default)]
pub struct TextSearch<'b> {
    /// The lists with hits.
    lists: Vec<(&'b Tag, TextList)>,
    /// In catalog order (by path), then stored order, then language order.
    pub hits: Vec<TextHit>,
    /// `unic` copies read.
    pub searched: usize,
    /// `unic` tags that couldn't be read.
    pub skipped: Vec<(&'b Tag, TextError)>,
    /// Texts looked at that couldn't be read ([`TextAt::Unreadable`]).
    pub unreadable: usize,
}

impl<'b> TextSearch<'b> {
    pub fn tag(&self, h: &TextHit) -> &'b Tag {
        self.lists[h.list].0
    }

    pub fn string_id(&self, h: &TextHit) -> &StringIdValue {
        &self.lists[h.list].1.string_ids[h.entry]
    }

    pub fn text(&self, h: &TextHit) -> TextAt<'_> {
        self.lists[h.list].1.text(h.entry, h.language)
    }

    /// The string_ids (by hash) the hits give different texts in one
    /// language: (string_id, language, different texts), in hit order.
    pub fn differing(&self) -> Vec<(&StringIdValue, usize, usize)> {
        /// A string_id's texts in one language: the first, then any other
        /// (allocated only when a second text turns up).
        struct Texts<'t> {
            id: &'t StringIdValue,
            language: usize,
            first: &'t str,
            others: Vec<&'t str>,
        }
        let mut groups: Vec<Texts<'_>> = Vec::new();
        let mut at: HashMap<(u32, usize), usize> = HashMap::with_capacity(self.hits.len());
        for h in &self.hits {
            let TextAt::Text(text) = self.text(h) else { continue };
            let id = self.string_id(h);
            match at.entry((id.hash, h.language)) {
                Entry::Vacant(v) => {
                    v.insert(groups.len());
                    groups.push(Texts { id, language: h.language, first: text, others: Vec::new() });
                }
                Entry::Occupied(o) => {
                    let g = &mut groups[*o.get()];
                    if g.first != text && !g.others.contains(&text) {
                        g.others.push(text);
                    }
                }
            }
        }
        groups.into_iter().filter(|g| !g.others.is_empty()).map(|g| (g.id, g.language, 1 + g.others.len())).collect()
    }
}

/// The texts of the `unic` copies in `scope` that `lookup` finds, in
/// `language` (a position in [`text_languages`]) or every language (`None`).
/// With one language a missing text is a hit too, except for a term.
pub fn search_texts<'b>(
    b: &'b Build,
    scope: Scope<'b>,
    lookup: &Lookup<'_>,
    language: Option<usize>,
) -> TextSearch<'b> {
    let copies: Vec<(&Tag, Copy)> = match scope {
        Scope::Latest => b
            .tags
            .iter()
            .filter(|t| t.class == text_class())
            .filter_map(|t| choose_copy(b, t, None).map(|c| (t, c)))
            .collect(),
        Scope::One(t, c) => vec![(t, c)],
    };
    let mut out = TextSearch::default();
    for (tag, copy) in copies {
        let list = match read_text_list(b, tag, copy) {
            Ok(l) => l,
            Err(e) => {
                out.skipped.push((tag, e));
                continue;
            }
        };
        out.searched += 1;
        let before = out.hits.len();
        for (entry, id) in list.string_ids.iter().enumerate() {
            if let Lookup::StringId(name) = lookup
                && !id.text.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name))
            {
                continue;
            }
            for lang in language.map_or(0..list.languages, |l| l..l + 1) {
                let text = list.text(entry, lang);
                out.unreadable += usize::from(text == TextAt::Unreadable);
                let hit = match (lookup, text) {
                    (Lookup::Term(accepts), TextAt::Text(t)) => accepts(t),
                    (Lookup::Term(_), _) => false,
                    (_, TextAt::Missing) => language.is_some(),
                    _ => true,
                };
                if hit {
                    out.hits.push(TextHit { list: out.lists.len(), entry, language: lang });
                }
            }
        }
        if out.hits.len() > before {
            out.lists.push((tag, list));
        }
    }
    out
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
