//! Scripts (M7, decisions D85): the script an `hsc*` or `luas` tag holds, as
//! source text or compiled code (format-notes "Scripts"). Domain only.

use h5_meta::{Content, Value, parse_path, select};
use h5_module::FourCc;

use crate::catalog::{Build, Copy, Tag, TagLookupError, TagSpec};
use crate::query::choose_copy;
use crate::stored::{DefinitionLoadError, ReadError, load_definition, read_stored_tag};

/// Each script class and the data field that holds its script.
pub const SCRIPT_FIELDS: [(&str, &str); 2] = [("hsc*", "source file.source"), ("luas", "luaFileData")];
/// Compiled Lua, Havok Script included, starts with ESC `Lua`.
pub const COMPILED_SIGNATURE: &[u8] = b"\x1bLua";
/// The header bytes every compiled copy in both samples shares (format-notes).
const HEADER_SIZE: usize = 16;
/// The longest string taken from compiled code.
const MAX_STRING: usize = 4096;

/// The data field holding a class's script, if it is a script class.
pub fn script_field(class: FourCc) -> Option<&'static str> {
    SCRIPT_FIELDS.iter().find(|(code, _)| FourCc::from_code(code) == Some(class)).map(|&(_, field)| field)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Form {
    /// Source text, its trailing NULs removed; `not_utf8`: bytes that aren't
    /// UTF-8 are shown as U+FFFD.
    Text { text: String, not_utf8: bool },
    /// Compiled code: its first bytes, and the strings it holds, in order.
    Compiled { header: Vec<u8>, strings: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Script {
    /// Bytes as stored.
    pub size: usize,
    pub form: Form,
}

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("not a script tag: scripts are in hsc* (lua) and luas (lua_script) tags")]
    NotAScript,
    #[error("no definition for class {0}")]
    NoDefinition(String),
    #[error(transparent)]
    Definition(#[from] DefinitionLoadError),
    #[error(transparent)]
    Read(#[from] ReadError),
    /// The definition has no data field where scripts are kept, or it wasn't read.
    #[error("the definition's {field} isn't a data field that could be read")]
    Field { field: &'static str },
}

/// The script of `tag`'s copy `copy`.
pub fn read_script(b: &Build, tag: &Tag, copy: Copy) -> Result<Script, ScriptError> {
    let field = script_field(tag.class).ok_or(ScriptError::NotAScript)?;
    let def = load_definition(b, tag)?.ok_or_else(|| ScriptError::NoDefinition(b.class_name(tag.class).into()))?;
    let stored = read_stored_tag(b, copy)?;
    let read = h5_meta::read_tag(&def, &stored.header, &stored.tag_data);
    let path = parse_path(&def.fields, field).map_err(|_| ScriptError::Field { field })?;
    let sels = select(&read.root, &path).map_err(|_| ScriptError::Field { field })?;
    let Some(Content::Value(Value::Data(d))) = sels.first().map(|s| &s.node.content) else {
        return Err(ScriptError::Field { field });
    };
    let bytes = match d.target {
        Some(t) => stored.header.block_data(&stored.tag_data, t).ok_or(ScriptError::Field { field })?,
        None => &[],
    };
    Ok(parse(bytes))
}

/// A script's stored bytes as text or compiled code.
pub fn parse(bytes: &[u8]) -> Script {
    let form = if bytes.starts_with(COMPILED_SIGNATURE) {
        Form::Compiled { header: bytes[..bytes.len().min(HEADER_SIZE)].to_vec(), strings: compiled_strings(bytes) }
    } else {
        let end = bytes.len() - bytes.iter().rev().take_while(|&&c| c == 0).count();
        let body = &bytes[..end];
        match std::str::from_utf8(body) {
            Ok(text) => Form::Text { text: text.to_owned(), not_utf8: false },
            Err(_) => Form::Text { text: String::from_utf8_lossy(body).into_owned(), not_utf8: true },
        }
    };
    Script { size: bytes.len(), form }
}

/// The strings compiled code holds, stored as an 8-byte big-endian length
/// (the bytes plus a NUL), the bytes, a NUL (format-notes "Scripts"): taken
/// wherever such a length is followed by that many printable bytes and the NUL.
fn compiled_strings(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(len_bytes) = bytes.get(i..i + 8) {
        let len = u64::from_be_bytes(len_bytes.try_into().expect("8 bytes")) as usize;
        let body = (2..=MAX_STRING + 1).contains(&len).then(|| bytes.get(i + 8..i + 8 + len)).flatten();
        match body {
            Some(b) if b[len - 1] == 0 && b[..len - 1].iter().all(|&c| (0x20..0x7F).contains(&c) || c == b'\t') => {
                out.push(String::from_utf8_lossy(&b[..len - 1]).into_owned());
                i += 8 + len;
            }
            _ => i += 1,
        }
    }
    out
}

/// A source line or compiled string that matched a search.
#[derive(Clone, Debug)]
pub struct ScriptHit<'b> {
    pub tag: &'b Tag,
    /// Source line (from 1); `None` for a compiled string.
    pub line: Option<usize>,
    /// The line without its indentation, or the string.
    pub text: String,
}

/// What [`search_scripts`] found.
#[derive(Debug, Default)]
pub struct ScriptSearch<'b> {
    /// In catalog order (by path), then by line.
    pub hits: Vec<ScriptHit<'b>>,
    /// Script tags read.
    pub searched: usize,
    /// Script tags that couldn't be read.
    pub skipped: Vec<(&'b Tag, ScriptError)>,
}

/// Every script tag of `b` (of `class` only, if given), its copy read by
/// default (D58): the source lines, or compiled strings, `matches` accepts.
pub fn search_scripts<'b>(b: &'b Build, class: Option<FourCc>, matches: &dyn Fn(&str) -> bool) -> ScriptSearch<'b> {
    let mut out = ScriptSearch::default();
    let wanted = |t: &&Tag| script_field(t.class).is_some() && class.is_none_or(|c| c == t.class);
    for tag in b.tags.iter().filter(wanted) {
        let Some(copy) = choose_copy(b, tag, None) else { continue };
        let script = match read_script(b, tag, copy) {
            Ok(s) => s,
            Err(e) => {
                out.skipped.push((tag, e));
                continue;
            }
        };
        out.searched += 1;
        match &script.form {
            Form::Text { text, .. } => {
                for (i, line) in text.lines().enumerate().filter(|(_, l)| matches(l)) {
                    out.hits.push(ScriptHit { tag, line: Some(i + 1), text: line.trim().to_owned() });
                }
            }
            Form::Compiled { strings, .. } => {
                for s in strings.iter().filter(|s| matches(s)) {
                    out.hits.push(ScriptHit { tag, line: None, text: s.clone() });
                }
            }
        }
    }
    out
}

/// A string a script holds that names a tag by its path (D85 step 4).
#[derive(Clone, Debug)]
pub struct TagName<'b> {
    /// The line it is on, in source text (from 1); `None` in compiled code.
    pub line: Option<usize>,
    /// As written in the script.
    pub text: String,
    /// As looked up: one path however it is written (`\\`, `/`, case).
    pub path: String,
    /// The tags with that path: one, several (the same path in several
    /// classes), or none (no indexed tag has it).
    pub tags: Vec<&'b Tag>,
}

/// Every string in `script` that looks like a tag path, in order, each with
/// the tags of build `b` it names (format-notes "Scripts"): source string
/// literals outside comments, or compiled strings but the `@…` source file
/// name. A string looks like a path if it has a `\` or `/`, at least 3
/// characters and only letters, digits and `\/_.-`.
pub fn tag_names<'b>(b: &'b Build, script: &Script) -> Vec<TagName<'b>> {
    let candidates: Vec<(Option<usize>, &str)> = match &script.form {
        Form::Text { text, .. } => source_strings(text).into_iter().map(|(line, s)| (Some(line), s)).collect(),
        Form::Compiled { strings, .. } => {
            strings.iter().filter(|s| !s.starts_with('@')).map(|s| (None, s.as_str())).collect()
        }
    };
    candidates
        .into_iter()
        .filter(|(_, s)| looks_like_path(s))
        .map(|(line, s)| {
            let path = as_tag_path(s);
            TagName { line, text: s.to_owned(), tags: tags_at(b, &path), path }
        })
        .collect()
}

fn looks_like_path(s: &str) -> bool {
    (s.contains('\\') || s.contains('/'))
        && s.len() >= 3
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '\\' | '/' | '_' | '.' | '-'))
}

/// A script's string as a tag path: `\\` read as `\`, `/` as `\`, lowercase,
/// without a leading `\`.
fn as_tag_path(s: &str) -> String {
    s.replace("\\\\", "\\").replace('/', "\\").to_ascii_lowercase().trim_start_matches('\\').to_owned()
}

/// The tags at `path`: as written (a class suffix picks the class), else
/// without a `.lua` source extension (compiled code names scripts so).
fn tags_at<'b>(b: &'b Build, path: &str) -> Vec<&'b Tag> {
    let find = |p: &str| match b.find_tag(&TagSpec::Name(p.to_owned())) {
        Ok(t) => vec![t],
        Err(TagLookupError::Ambiguous(tags)) => tags,
        Err(_) => Vec::new(),
    };
    match find(path) {
        found if found.is_empty() && path.ends_with(".lua") => find(path.trim_end_matches(".lua")),
        found => found,
    }
}

/// The string literals of Lua source with their line (from 1), comments
/// and long strings (`[[…]]`) skipped. A backslash and the character after
/// it stay in the literal as written; only a quote after a backslash doesn't
/// end it.
fn source_strings(text: &str) -> Vec<(usize, &str)> {
    let bytes = text.as_bytes();
    let (mut out, mut i, mut line) = (Vec::new(), 0, 1);
    // Where the next `]]` ends (a long string or block comment), or the end.
    let long_end = |from: usize| text[from..].find("]]").map_or(bytes.len(), |k| from + k + 2);
    while i < bytes.len() {
        let start = i;
        i = match bytes[i] {
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                if bytes[i + 2..].starts_with(b"[[") {
                    long_end(i + 4)
                } else {
                    text[i..].find('\n').map_or(bytes.len(), |k| i + k)
                }
            }
            b'[' if bytes.get(i + 1) == Some(&b'[') => long_end(i + 2),
            q @ (b'\'' | b'"') => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != q && bytes[j] != b'\n' {
                    j += if bytes[j] == b'\\' { 2 } else { 1 };
                }
                let end = j.min(bytes.len());
                out.push((line, &text[i + 1..end]));
                end + 1
            }
            _ => i + 1,
        };
        line += bytes[start..i.min(bytes.len())].iter().filter(|&&c| c == b'\n').count();
    }
    out
}

#[cfg(test)]
#[path = "script_tests.rs"]
mod tests;
