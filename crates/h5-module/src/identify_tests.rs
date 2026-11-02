use std::fs;

use super::*;
use crate::testutil::{TempDir, TestEntry, build_module, build_tag, sample_tag};

/// A module of `version` holding one real tag (its header is version 27).
fn module_with_a_tag(version: Version) -> Vec<u8> {
    let (header, data) = build_tag(&sample_tag());
    let mut tag = TestEntry::tag(r"objects\test\test.weapon", 0x10, "weap", &data);
    tag.sections.0 = header;
    build_module(version, &[tag], &[])
}

/// D63: each module's format (read even when unsupported), a sample tag
/// header's format, and the content version in its name.
#[test]
fn a_folder_s_formats_and_versions() {
    let dir = TempDir::new("identify");
    fs::create_dir_all(dir.0.join("levels")).unwrap();
    fs::write(dir.0.join("levels").join("globals-rtx-1.module"), module_with_a_tag(Version::V23)).unwrap();
    fs::write(dir.0.join("levels").join("globals-rtx-97230-1.module"), module_with_a_tag(Version::V23)).unwrap();
    let mut beta = module_with_a_tag(Version::V27);
    beta[4] = 53;
    fs::write(dir.0.join("beta-rtx-1.module"), beta).unwrap();
    fs::write(dir.0.join("notes-rtx-1.module"), b"not a module at all").unwrap();

    let f = identify(&dir.0).unwrap();
    assert_eq!(f.modules.len(), 4);
    assert_eq!(f.module_formats(), BTreeMap::from([(23, 2), (53, 1)]));
    assert_eq!(f.tag_formats(), BTreeMap::from([(27, 2)]));
    assert_eq!(f.latest_layer(), Some(Layer { version: 97230, revision: 1 }));
    let unsupported: Vec<&str> = f
        .modules
        .iter()
        .filter(|m| m.format.as_ref().is_ok_and(|v| !v.supported))
        .map(|m| m.rel_path.as_str())
        .collect();
    assert_eq!(unsupported, ["beta-rtx-1.module"]);
    let unreadable: Vec<&str> = f.modules.iter().filter(|m| m.format.is_err()).map(|m| m.rel_path.as_str()).collect();
    assert_eq!(unreadable, ["notes-rtx-1.module"]);
}
