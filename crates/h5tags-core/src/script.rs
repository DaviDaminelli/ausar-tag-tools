//! Scripts (M7, decisions D85): the script an `hsc*` or `luas` tag holds, as
//! source text or compiled code (format-notes "Scripts"). Domain only.

use h5_meta::{Content, Value, parse_path, select};
use h5_module::FourCc;

use crate::catalog::{Build, Copy, Tag};
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

#[cfg(test)]
#[path = "script_tests.rs"]
mod tests;
