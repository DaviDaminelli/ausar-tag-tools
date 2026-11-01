use std::fs;

use crate::index::Index;
use crate::testutil::{SAMPLE_LOADED_TAGS, TempDir, TestEntry, build_module, sample_entries};
use crate::{EntryKind, FourCc, Module, ModuleError, Section, Version, checks};

fn sample(version: Version) -> Vec<u8> {
    build_module(version, &sample_entries(), SAMPLE_LOADED_TAGS)
}

fn expected_data(e: &TestEntry) -> Vec<u8> {
    if e.raw_file { e.sections.0.clone() } else { [e.sections.0.as_slice(), &e.sections.1, &e.sections.2].concat() }
}

#[test]
fn parses_both_versions() {
    for version in [Version::V23, Version::V27] {
        let bytes = sample(version);
        let m = Module::parse(&bytes).unwrap();
        assert_eq!(m.header.version, version);
        assert_eq!(m.header.file_count, 5);
        assert_eq!(m.header.resource_count, 2);
        assert_eq!(m.header.loaded_tag_count, 2);
        assert_eq!(m.header.checksum.is_some(), version == Version::V27);
        assert_eq!(m.name(0).unwrap(), "loadmanifest");
        assert_eq!(m.name(2).unwrap(), r"objects\weapons\rifle\assault_rifle\assault_rifle.weapon");
        assert_eq!(m.entries[1].class_code, FourCc::from_code("sbsp").unwrap());
        assert_eq!(m.entries[0].class_code, FourCc::NONE);
        assert_eq!(
            [m.entries[0].kind(), m.entries[1].kind(), m.entries[3].kind()],
            [EntryKind::File, EntryKind::Tag, EntryKind::Resource]
        );
        assert_eq!(m.entry_resources(1).unwrap(), &[3, 4]);
        let expected_start = version.header_size()
            + 5 * 0x58
            + m.header.file_names_size as usize
            + 2 * 4
            + m.header.block_count as usize * version.block_entry_size();
        assert_eq!(m.data_start, expected_start as u64);
    }
}

#[test]
fn sample_passes_every_check() {
    for version in [Version::V23, Version::V27] {
        let bytes = sample(version);
        let failures = Module::parse(&bytes).unwrap().check();
        assert!(failures.is_empty(), "{version:?}: {failures:?}");
    }
}

#[test]
fn every_entry_decompresses_to_its_data() {
    let entries = sample_entries();
    for version in [Version::V23, Version::V27] {
        let bytes = sample(version);
        let m = Module::parse(&bytes).unwrap();
        for (i, e) in entries.iter().enumerate() {
            assert_eq!(m.read_entry(i as u32).unwrap(), expected_data(e), "{version:?} entry {i}");
        }
    }
}

#[test]
fn uncompressed_blocks_are_copied() {
    let mut e = TestEntry::tag(r"a\b.weapon", 1, "weap", b"plain tag data");
    e.compressed = false;
    let bytes = build_module(Version::V27, &[e], &[]);
    let m = Module::parse(&bytes).unwrap();
    assert!(m.check().is_empty());
    assert_eq!(m.read_entry(0).unwrap(), b"hscu-headerplain tag data");
}

#[test]
fn entries_stored_elsewhere() {
    // As seen in fo06_deepsea (v23): totals are 0, sections and blocks describe the data.
    let bytes = sample(Version::V23);
    let m = Module::parse(&bytes).unwrap();
    let blocks_end = m.entry_blocks(3).unwrap().iter().map(|b| b.uncompressed_size as u64).sum::<u64>();
    drop(m);
    let mut bytes = bytes;
    let rec = Version::V23.header_size() + 3 * 0x58;
    bytes[rec + 0x20..rec + 0x28].fill(0); // total compressed and uncompressed sizes
    let m = Module::parse(&bytes).unwrap();
    let e = &m.entries[3];
    assert!(e.is_stored_elsewhere());
    assert_eq!(e.data_size(), blocks_end);
    assert!(matches!(m.read_entry(3), Err(ModuleError::NotStored { index: 3, .. })));
    // Its blocks still tile its sections, so no check fails except the file's
    // data end (the entry's real data is still in this synthetic file).
    let failed: Vec<_> = m.check().into_iter().map(|f| f.check).collect();
    assert!(
        !failed.contains(&checks::BLOCKS_TOTAL) && !failed.contains(&checks::BLOCKS_COMPRESSED_RANGE),
        "{failed:?}"
    );
}

#[test]
fn rejects_bad_magic_and_versions() {
    let mut bytes = sample(Version::V27);
    bytes[0] = b'x';
    assert!(matches!(Module::parse(&bytes), Err(ModuleError::BadMagic(_))));

    let mut bytes = sample(Version::V27);
    bytes[4] = 53;
    assert!(matches!(Module::parse(&bytes), Err(ModuleError::UnsupportedVersion(53))));
}

#[test]
fn rejects_truncated_lists() {
    let bytes = sample(Version::V23);
    for cut in [10, 0x30 + 20, 0x30 + 5 * 0x58 + 3] {
        assert!(matches!(Module::parse(&bytes[..cut]), Err(ModuleError::Truncated { .. })), "cut at {cut}");
    }
}

#[test]
fn detects_trailing_bytes() {
    let mut bytes = sample(Version::V27);
    bytes.extend([0u8; 16]);
    let failures = Module::parse(&bytes).unwrap().check();
    assert_eq!(failures.iter().map(|f| f.check).collect::<Vec<_>>(), [checks::DATA_END]);
}

#[test]
fn detects_wrong_section_sizes() {
    let mut bytes = sample(Version::V27);
    // Entry 2's tag data size field, +1.
    let off = Version::V27.header_size() + 2 * 0x58 + 0x48;
    bytes[off] += 1;
    let failures = Module::parse(&bytes).unwrap().check();
    assert!(
        failures.iter().any(|f| f.check == checks::SECTION_SIZES && f.item == Some(crate::check::Item::entry(2))),
        "{failures:?}"
    );
}

#[test]
fn detects_misread_entry_size() {
    // Reading entries with the wrong layout version shifts every list; the
    // cross-checks must notice (this is what a wrong layout looks like).
    let mut bytes = sample(Version::V23);
    bytes[4] = 27;
    match Module::parse(&bytes) {
        Err(_) => {}
        Ok(m) => assert!(!m.check().is_empty()),
    }
}

#[test]
fn detects_duplicate_ids_and_manifest_size() {
    let mut entries = sample_entries();
    entries[2].global_id = entries[1].global_id;
    let bytes = build_module(Version::V23, &entries, &[1, 2, 3]);
    let failed: Vec<_> = Module::parse(&bytes).unwrap().check().into_iter().map(|f| f.check).collect();
    assert!(failed.contains(&checks::DUPLICATE_GLOBAL_ID), "{failed:?}");
    assert!(failed.contains(&checks::LOADMANIFEST_SIZE), "{failed:?}");
}

#[test]
fn corrupt_zlib_is_an_error() {
    let entries = sample_entries();
    let bytes = sample(Version::V27);
    let m = Module::parse(&bytes).unwrap();
    let e = &m.entries[2];
    let block = &m.entry_blocks(2).unwrap()[1];
    let at = (m.data_start + e.data_offset + block.compressed_offset as u64) as usize + 4;
    drop(m);
    let mut bytes = bytes;
    bytes[at] ^= 0xFF;
    let m = Module::parse(&bytes).unwrap();
    assert!(matches!(m.read_entry(2), Err(ModuleError::BadBlock { index: 2, .. })));
    // Other entries are unaffected.
    assert_eq!(m.read_entry(1).unwrap(), expected_data(&entries[1]));
}

#[test]
fn index_reads_reuses_and_drops_modules() {
    let tmp = TempDir::new("index");
    let deploy = tmp.0.join("deploy");
    fs::create_dir_all(deploy.join("any").join("levels")).unwrap();
    fs::write(deploy.join("any").join("levels").join("b.module"), sample(Version::V27)).unwrap();
    fs::write(deploy.join("A.module"), sample(Version::V23)).unwrap();
    fs::write(deploy.join("not_a_module.txt"), b"ignored").unwrap();
    fs::write(deploy.join("broken.module"), b"garbage!").unwrap();

    let never = |_: &str| false;
    let (index, report) = Index::update(&deploy, None, &never).unwrap();
    assert_eq!((report.read, report.reused, report.failed), (3, 0, 1));
    let paths: Vec<_> = index.modules.iter().map(|m| m.rel_path.as_str()).collect();
    assert_eq!(paths, ["A.module", r"any\levels\b.module"]);
    assert_eq!(index.failed[0].rel_path, "broken.module");
    assert!(index.failed[0].error.contains("not a module file"));

    let m = &index.modules[0];
    assert_eq!(m.version, 23);
    assert_eq!(m.tag_count(), 2);
    assert!(m.has_loadmanifest());
    let kinds: Vec<_> = m.entries.iter().map(|e| e.kind()).collect();
    use EntryKind::*;
    assert_eq!(kinds, [File, Tag, Tag, Resource, Resource]);
    assert_eq!(m.entries[1].resources, [3, 4]);

    // Cache round trip.
    let cache = tmp.0.join("cache").join("default.index");
    index.save(&cache).unwrap();
    let loaded = Index::load(&cache).unwrap();
    assert_eq!(loaded, index);

    // Unchanged modules are reused; a removed one is dropped; force re-reads.
    fs::remove_file(deploy.join("A.module")).unwrap();
    let force = |p: &str| p.ends_with("b.module");
    let (index2, report) = Index::update(&deploy, Some(loaded.clone()), &force).unwrap();
    assert_eq!((report.read, report.reused, report.removed), (2, 0, 1));
    assert_eq!(index2.modules.len(), 1);
    let (_, report) = Index::update(&deploy, Some(index2), &never).unwrap();
    assert_eq!((report.read, report.reused), (1, 1)); // broken.module is retried each time
}

#[test]
fn index_reports_progress() {
    use crate::index::Progress;
    let tmp = TempDir::new("progress");
    for name in ["a", "b", "c"] {
        fs::write(tmp.0.join(format!("{name}.module")), sample(Version::V27)).unwrap();
    }
    let seen = std::sync::Mutex::new(Vec::new());
    let record = |p: Progress| seen.lock().unwrap().push(p);
    let (index, _) = Index::update_with_progress(&tmp.0, None, &|_: &str| false, &record).unwrap();
    let mut seen = seen.into_inner().unwrap();
    assert_eq!(seen.first(), Some(&Progress { stage: Progress::READING_MODULES, done: 0, total: 3 }));
    seen.sort_by_key(|p| p.done); // workers report in any order
    assert_eq!(seen.iter().map(|p| p.done).collect::<Vec<_>>(), [0, 1, 2, 3]);
    // Nothing to read on a warm start: nothing reported.
    let quiet = std::sync::Mutex::new(0);
    let count = |_: Progress| *quiet.lock().unwrap() += 1;
    Index::update_with_progress(&tmp.0, Some(index), &|_: &str| false, &count).unwrap();
    assert_eq!(quiet.into_inner().unwrap(), 0);
}

#[test]
fn index_records_issues_of_incomplete_modules() {
    let tmp = TempDir::new("incomplete");
    let full = sample(Version::V27);
    fs::write(tmp.0.join("cut.module"), &full[..full.len() - 1]).unwrap();
    fs::write(tmp.0.join("ok.module"), &full).unwrap();
    let (index, _) = Index::update(&tmp.0, None, &|_: &str| false).unwrap();
    let cut = &index.modules[0];
    let ok = &index.modules[1];
    assert_eq!(cut.rel_path, "cut.module");
    assert!(ok.issues.is_empty(), "{:?}", ok.issues);
    assert_eq!(ok.data_end, ok.stamp.size);
    assert_eq!(cut.entries_past_end(), 1);
    assert_eq!(cut.data_end, full.len() as u64);
    assert!(cut.issues.iter().any(|i| i.check == checks::DATA_END));
}

#[test]
fn index_cache_rejects_other_formats() {
    let tmp = TempDir::new("cache");
    let path = tmp.0.join("x.index");
    assert!(Index::load(&path).is_none());
    fs::write(&path, b"H5TAGIDX\x63\x00\x00\x00rest").unwrap();
    assert!(Index::load(&path).is_none());
    fs::write(&path, b"H5TAGIDX\x01\x00\x00\x00\xFF\xFF").unwrap();
    assert!(Index::load(&path).is_none());
}

fn sections_of(e: &TestEntry) -> [&[u8]; 3] {
    [&e.sections.0, &e.sections.1, &e.sections.2]
}

const SECTIONS: [Section; 3] = [Section::Header, Section::TagData, Section::ResourceData];

#[test]
fn reads_each_section_alone() {
    let entries = sample_entries();
    for version in [Version::V23, Version::V27] {
        let bytes = sample(version);
        let m = Module::parse(&bytes).unwrap();
        for (i, e) in entries.iter().enumerate().filter(|(_, e)| !e.raw_file) {
            for (section, expected) in SECTIONS.iter().zip(sections_of(e)) {
                assert_eq!(m.read_section(i as u32, *section).unwrap(), expected, "{version:?} entry {i} {section:?}");
            }
        }
    }
}

#[test]
fn sections_from_one_block_holding_all_three() {
    let mut e = TestEntry::tag(r"a\b.weapon", 1, "weap", b"the tag data");
    e.sections.2 = b"and its resource data".to_vec();
    e.has_blocks = false;
    let bytes = build_module(Version::V27, &[e], &[]);
    let m = Module::parse(&bytes).unwrap();
    assert_eq!(m.read_section(0, Section::Header).unwrap(), b"hscu-header");
    assert_eq!(m.read_section(0, Section::TagData).unwrap(), b"the tag data");
    assert_eq!(m.read_section(0, Section::ResourceData).unwrap(), b"and its resource data");
}

#[test]
fn reading_a_section_skips_the_other_blocks() {
    let mut e = TestEntry::tag(r"a\b.bitmap", 1, "bitm", b"tag data");
    e.sections.2 = vec![9u8; 20_000];
    let bytes = build_module(Version::V27, &[e], &[]);
    // Damage the resource data block (the entry's third block).
    let m = Module::parse(&bytes).unwrap();
    let block = &m.entry_blocks(0).unwrap()[2];
    let at = (m.data_start + m.entries[0].data_offset + block.compressed_offset as u64) as usize + 8;
    let mut bytes = bytes.clone();
    bytes[at] ^= 0xFF;
    let m = Module::parse(&bytes).unwrap();
    assert_eq!(m.read_section(0, Section::Header).unwrap(), b"hscu-header");
    assert_eq!(m.read_section(0, Section::TagData).unwrap(), b"tag data");
    assert!(matches!(m.read_section(0, Section::ResourceData), Err(ModuleError::BadBlock { index: 0, block: 2, .. })));
}

#[test]
fn raw_files_and_data_not_stored_here_have_no_sections() {
    let bytes = sample(Version::V23);
    let m = Module::parse(&bytes).unwrap();
    assert!(matches!(m.read_section(0, Section::Header), Err(ModuleError::BadEntry { index: 0, .. })));

    let mut bytes = sample(Version::V23);
    let rec = Version::V23.header_size() + 3 * 0x58;
    bytes[rec + 0x20..rec + 0x28].fill(0); // totals 0: not stored here
    let m = Module::parse(&bytes).unwrap();
    assert!(matches!(m.read_section(3, Section::Header), Err(ModuleError::NotStored { index: 3, .. })));
}

#[test]
fn an_empty_section_is_empty() {
    let bytes = sample(Version::V27);
    let m = Module::parse(&bytes).unwrap();
    assert_eq!(m.read_section(1, Section::ResourceData).unwrap(), Vec::<u8>::new());
}
