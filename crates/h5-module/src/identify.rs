//! What a folder of modules is (D63): each module's format, a sample tag
//! header's format, and the content version in its name. Reads the first
//! bytes of each module and one tag header per supported module; works on
//! formats this crate can't read, to say so.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use crate::index::find_modules;
use crate::le::u32_at;
use crate::module::{EntryKind, MAGIC, Module, Section, Version, map_file};
use crate::names::{Layer, module_layer};
use crate::tag::TAG_MAGIC;

/// A module file's format version, as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Format {
    pub version: u32,
    /// Whether this crate reads it (`Version`).
    pub supported: bool,
}

/// What one module file shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleFacts {
    /// Relative to the folder.
    pub rel_path: String,
    /// The module format, or why the file isn't a module.
    pub format: Result<Format, String>,
    /// The tag header format of its first tag (supported modules with a tag).
    pub tag_format: Option<u32>,
    /// From its name (`…-rtx-<version>-<revision>.module`).
    pub layer: Layer,
}

/// Every module file under a folder, in path order.
#[derive(Clone, Debug, Default)]
pub struct Folder {
    pub modules: Vec<ModuleFacts>,
}

impl Folder {
    /// Module format version → how many modules have it.
    pub fn module_formats(&self) -> BTreeMap<u32, usize> {
        let mut out = BTreeMap::new();
        for m in &self.modules {
            if let Ok(f) = &m.format {
                *out.entry(f.version).or_default() += 1;
            }
        }
        out
    }

    /// Tag header format version → how many modules' sample tag has it.
    pub fn tag_formats(&self) -> BTreeMap<u32, usize> {
        let mut out = BTreeMap::new();
        for v in self.modules.iter().filter_map(|m| m.tag_format) {
            *out.entry(v).or_default() += 1;
        }
        out
    }

    /// The highest update layer in the module names (`None`: base modules only).
    pub fn latest_layer(&self) -> Option<Layer> {
        crate::names::latest_layer(self.modules.iter().map(|m| m.rel_path.as_str()))
    }
}

/// The format version stored after the magic (`None`: not a module).
fn module_format(bytes: &[u8]) -> Result<Format, String> {
    if bytes.len() < 8 || u32_at(bytes, 0) != MAGIC {
        return Err("not a module file (no module magic)".into());
    }
    let version = u32_at(bytes, 4);
    Ok(Format { version, supported: Version::from_raw(version).is_some() })
}

/// The tag header format of the module's first tag, if it can be read.
fn first_tag_format(bytes: &[u8]) -> Option<u32> {
    let module = Module::parse(bytes).ok()?;
    let entry = module.entries.iter().position(|e| e.kind() == EntryKind::Tag)?;
    let header = module.read_section(entry as u32, Section::Header).ok()?;
    (header.len() >= 8 && u32_at(&header, 0) == TAG_MAGIC).then(|| u32_at(&header, 4))
}

/// Every `.module` file under `folder` (recursively, any layout).
pub fn identify(folder: &Path) -> io::Result<Folder> {
    let mut out = Folder::default();
    for found in find_modules(folder)? {
        let layer = module_layer(&found.rel_path);
        let (format, tag_format) = match map_file(&found.path) {
            Ok(bytes) => {
                let format = module_format(&bytes);
                let tag = match &format {
                    Ok(f) if f.supported => first_tag_format(&bytes),
                    _ => None,
                };
                (format, tag)
            }
            Err(e) => (Err(e.to_string()), None),
        };
        out.modules.push(ModuleFacts { rel_path: found.rel_path, format, tag_format, layer });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "identify_tests.rs"]
mod tests;
