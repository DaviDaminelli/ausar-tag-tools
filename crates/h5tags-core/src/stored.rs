//! Reading what a module stores for one tag copy (PRD §6.2: only the header
//! and tag data sections; resource data stays untouched). Domain only.

use h5_module::index::FileStamp;
use h5_module::tag::{TagError, TagHeader};
use h5_module::{Module, ModuleError, Section, map_file};

use crate::catalog::{Build, Copy};

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

/// Reads the header and tag data of `copy`, from its module file as indexed.
pub fn read_stored_tag(build: &Build, copy: Copy) -> Result<StoredTag, ReadError> {
    let record = build.module(copy);
    let module = || record.rel_path.clone();
    let path = build.module_file(record);
    let in_module = |error| ReadError::Module { module: module(), error };
    let meta = std::fs::metadata(&path).map_err(|error| ReadError::Open { module: module(), error })?;
    if FileStamp::of(&meta) != record.stamp {
        return Err(ReadError::Changed { module: module() });
    }
    let map = map_file(&path).map_err(in_module)?;
    let parsed = Module::parse(&map).map_err(in_module)?;
    let read = |s| parsed.read_section(copy.entry, s).map_err(in_module);
    let header =
        TagHeader::parse(&read(Section::Header)?).map_err(|error| ReadError::Header { module: module(), error })?;
    Ok(StoredTag { header, tag_data: read(Section::TagData)? })
}

#[cfg(test)]
mod tests {
    use h5_module::GlobalId;

    use super::*;
    use crate::testdeploy::{TestDeploy, WEAPON_ID};

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
}
