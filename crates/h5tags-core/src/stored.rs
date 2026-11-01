//! Reading what a module stores for one tag copy (PRD §6.2: only the header
//! and tag data sections; resource data stays untouched). Domain only.

use h5_module::index::FileStamp;
use h5_module::tag::{TagError, TagHeader};
use h5_module::{Module, ModuleError, Section, map_file};

use h5_defs::definition::Definition;

use crate::catalog::{Build, Copy, Tag};

/// A tag copy as stored: its parsed header and its tag data section.
#[derive(Debug)]
pub struct StoredTag {
    pub header: TagHeader,
    pub tag_data: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("can't open {module}: {error}")]
    Open { module: String, error: std::io::Error },
    /// The file is not the one indexed (size or time differ): its entry
    /// numbers may point elsewhere now.
    #[error("{module} changed since it was indexed")]
    Changed { module: String },
    #[error("{module}: {error}")]
    Module { module: String, error: ModuleError },
    #[error("{module}: tag header: {error}")]
    Header { module: String, error: TagError },
}

/// Maps `copy`'s module file, refusing one changed since it was indexed (D28).
fn open(build: &Build, copy: Copy) -> Result<impl std::ops::Deref<Target = [u8]>, ReadError> {
    let record = build.module(copy);
    let module = || record.rel_path.clone();
    let path = build.module_file(record);
    let meta = std::fs::metadata(&path).map_err(|error| ReadError::Open { module: module(), error })?;
    if FileStamp::of(&meta) != record.stamp {
        return Err(ReadError::Changed { module: module() });
    }
    map_file(&path).map_err(|error| ReadError::Module { module: module(), error })
}

fn in_module(build: &Build, copy: Copy) -> impl Fn(ModuleError) -> ReadError + '_ {
    move |error| ReadError::Module { module: build.module(copy).rel_path.clone(), error }
}

/// Reads the header and tag data of `copy`, from its module file as indexed.
pub fn read_stored_tag(build: &Build, copy: Copy) -> Result<StoredTag, ReadError> {
    let map = open(build, copy)?;
    let parsed = Module::parse(&map).map_err(in_module(build, copy))?;
    let read = |s| parsed.read_section(copy.entry, s).map_err(in_module(build, copy));
    let header = TagHeader::parse(&read(Section::Header)?)
        .map_err(|error| ReadError::Header { module: build.module(copy).rel_path.clone(), error })?;
    Ok(StoredTag { header, tag_data: read(Section::TagData)? })
}

/// Whether a tag's copies hold the same data (PRD §6.4.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    Single,
    Identical,
    Differ,
}

/// Compares every copy with the first by stored data: the compression block
/// checksums when every block has one (v27, no decompression), else the
/// decompressed header and tag data (v23 and blockless entries).
pub fn copy_identity(build: &Build, tag: &Tag) -> Result<Identity, ReadError> {
    if tag.copies.len() < 2 {
        return Ok(Identity::Single);
    }
    let checksums = |copy: Copy| -> Result<Option<Vec<(u64, u32)>>, ReadError> {
        let map = open(build, copy)?;
        let m = Module::parse(&map).map_err(in_module(build, copy))?;
        let blocks = m.entry_blocks(copy.entry).map_err(in_module(build, copy))?;
        Ok(blocks.iter().map(|b| Some((b.checksum?, b.uncompressed_size))).collect())
    };
    let sums: Vec<Option<Vec<(u64, u32)>>> = tag.copies.iter().map(|&c| checksums(c)).collect::<Result<_, _>>()?;
    let same = if sums.iter().all(Option::is_some) {
        sums.windows(2).all(|w| w[0] == w[1])
    } else {
        let data: Vec<(Vec<u8>, Vec<u8>)> = tag
            .copies
            .iter()
            .map(|&c| {
                let map = open(build, c)?;
                let m = Module::parse(&map).map_err(in_module(build, c))?;
                let read = |s| m.read_section(c.entry, s).map_err(in_module(build, c));
                Ok((read(Section::Header)?, read(Section::TagData)?))
            })
            .collect::<Result<_, ReadError>>()?;
        data.windows(2).all(|w| w[0] == w[1])
    };
    Ok(if same { Identity::Identical } else { Identity::Differ })
}

#[derive(Debug, thiserror::Error)]
pub enum DefinitionLoadError {
    #[error("can't read the definition {path}: {error}")]
    Read { path: std::path::PathBuf, error: std::io::Error },
    #[error("definition {path}: {error}")]
    Parse { path: std::path::PathBuf, error: h5_defs::definition::DefinitionError },
}

/// The parsed definition of `tag`'s class; `None` if the class has none.
pub fn load_definition(build: &Build, tag: &Tag) -> Result<Option<Definition>, DefinitionLoadError> {
    let Some(path) = build.classes.get(&tag.class).and_then(|c| c.definition.as_ref()) else { return Ok(None) };
    let xml = std::fs::read_to_string(path).map_err(|error| DefinitionLoadError::Read { path: path.clone(), error })?;
    Definition::parse(&xml).map(Some).map_err(|error| DefinitionLoadError::Parse { path: path.clone(), error })
}

#[cfg(test)]
mod tests {
    use h5_module::GlobalId;

    use super::*;
    use crate::testdeploy::{SecondCopy, TestDeploy, WEAPON_ID};

    fn weapon_copy(d: &TestDeploy) -> Copy {
        d.session.builds()[0].tag_by_id(GlobalId(WEAPON_ID)).unwrap().copies[0]
    }

    #[test]
    fn reads_the_header_and_the_tag_data() {
        let d = TestDeploy::new("stored-read");
        let tag = read_stored_tag(&d.session.builds()[0], weapon_copy(&d)).unwrap();
        assert_eq!(tag.header.dependencies.len(), 2);
        assert_eq!(tag.tag_data.len(), 0xB0);
        assert!(tag.header.check_tag_data(&tag.tag_data).is_empty());
    }

    #[test]
    fn refuses_a_module_changed_since_indexing() {
        let d = TestDeploy::new("stored-changed");
        let mut bytes = std::fs::read(d.module_file()).unwrap();
        bytes.extend([0; 16]);
        std::fs::write(d.module_file(), bytes).unwrap();
        let err = read_stored_tag(&d.session.builds()[0], weapon_copy(&d)).unwrap_err();
        assert!(matches!(err, ReadError::Changed { .. }), "{err}");
    }

    #[test]
    fn a_missing_module_is_an_error() {
        let d = TestDeploy::new("stored-missing");
        std::fs::remove_file(d.module_file()).unwrap();
        let err = read_stored_tag(&d.session.builds()[0], weapon_copy(&d)).unwrap_err();
        assert!(matches!(err, ReadError::Open { .. }), "{err}");
    }

    fn weapon(d: &TestDeploy) -> &crate::catalog::Tag {
        d.session.builds()[0].tag_by_id(GlobalId(WEAPON_ID)).unwrap()
    }

    #[test]
    fn loads_the_definition_of_a_tags_class() {
        let d = TestDeploy::new("stored-definition");
        let b = &d.session.builds()[0];
        let def = load_definition(b, weapon(&d)).unwrap().expect("a weapon definition");
        assert_eq!(def.fields[1].name.display, "entries");
    }

    /// PRD §6.4.6: copies are compared by their stored data (v27: block checksums).
    #[test]
    fn tells_identical_copies_from_different_ones() {
        let d = TestDeploy::new("stored-one-copy");
        assert_eq!(copy_identity(&d.session.builds()[0], weapon(&d)).unwrap(), Identity::Single);
        let d = TestDeploy::with("stored-identical", SecondCopy::Identical);
        assert_eq!(copy_identity(&d.session.builds()[0], weapon(&d)).unwrap(), Identity::Identical);
        let d = TestDeploy::with("stored-changed", SecondCopy::Changed);
        assert_eq!(copy_identity(&d.session.builds()[0], weapon(&d)).unwrap(), Identity::Differ);
    }
}
