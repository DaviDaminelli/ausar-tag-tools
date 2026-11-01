//! Builds small synthetic modules for tests (no game files in the repo).

use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::fourcc::FourCc;
use crate::module::{MAGIC, Version};

pub struct TestEntry {
    pub name: String,
    pub global_id: i32,
    pub class: &'static str,
    pub parent: i32,
    /// Indices into the built module's file list.
    pub resources: Vec<u32>,
    /// (header, tag data, resource data); for raw files only `.0` is used.
    pub sections: (Vec<u8>, Vec<u8>, Vec<u8>),
    pub raw_file: bool,
    /// One compression block per non-empty section (`HasBlocks`), or a single
    /// block described by the entry itself.
    pub has_blocks: bool,
    pub compressed: bool,
}

impl TestEntry {
    pub fn tag(name: &str, global_id: i32, class: &'static str, tag_data: &[u8]) -> TestEntry {
        TestEntry {
            name: name.into(),
            global_id,
            class,
            parent: -1,
            resources: vec![],
            sections: (b"hscu-header".to_vec(), tag_data.to_vec(), vec![]),
            raw_file: false,
            has_blocks: true,
            compressed: true,
        }
    }

    pub fn resource(name: &str, parent: i32, data: &[u8]) -> TestEntry {
        TestEntry {
            name: name.into(),
            global_id: -1,
            class: "",
            parent,
            resources: vec![],
            sections: (b"res-header".to_vec(), vec![], data.to_vec()),
            raw_file: false,
            has_blocks: true,
            compressed: true,
        }
    }

    pub fn raw(name: &str, data: &[u8]) -> TestEntry {
        TestEntry {
            name: name.into(),
            global_id: -1,
            class: "",
            parent: -1,
            resources: vec![],
            sections: (data.to_vec(), vec![], vec![]),
            raw_file: true,
            has_blocks: false,
            compressed: true,
        }
    }
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// Builds a module of `version` from `entries`, laid out as the real files are.
pub fn build_module(version: Version, entries: &[TestEntry], loaded_tags: &[i32]) -> Vec<u8> {
    let mut names = Vec::new();
    let mut resource_list: Vec<u32> = Vec::new();
    let mut blocks = Vec::new(); // (compressed offset, compressed size, uncompressed offset, size, compressed)
    let mut data = Vec::new();
    let mut file_list = Vec::new();

    for e in entries {
        let name_offset = names.len() as u32;
        names.extend_from_slice(e.name.as_bytes());
        names.push(0);

        let first_resource = resource_list.len() as u32;
        resource_list.extend(&e.resources);

        let sections: Vec<&[u8]> =
            if e.raw_file { vec![&e.sections.0] } else { vec![&e.sections.0, &e.sections.1, &e.sections.2] };
        let total_uncompressed: usize = sections.iter().map(|s| s.len()).sum();
        let data_offset = data.len() as u64;
        let first_block = blocks.len() as u32;
        let mut block_counts = [0u16; 3];
        let mut uoff = 0u32;
        if e.has_blocks {
            for (k, s) in sections.iter().enumerate() {
                if s.is_empty() {
                    continue;
                }
                let stored = if e.compressed { zlib(s) } else { s.to_vec() };
                blocks.push((
                    (data.len() as u64 - data_offset) as u32,
                    stored.len() as u32,
                    uoff,
                    s.len() as u32,
                    e.compressed,
                ));
                data.extend(stored);
                uoff += s.len() as u32;
                block_counts[k] += 1;
            }
        } else if total_uncompressed > 0 {
            let all: Vec<u8> = sections.concat();
            data.extend(if e.compressed { zlib(&all) } else { all });
        }
        let total_compressed = (data.len() as u64 - data_offset) as u32;

        let mut flags = 0u8;
        if e.compressed {
            flags |= 1;
        }
        if e.has_blocks {
            flags |= 2;
        }
        if e.raw_file {
            flags |= 4;
        }
        let class = FourCc::from_code(e.class).unwrap_or(FourCc::NONE);
        let (h, t, r) = if e.raw_file {
            (0, 0, 0)
        } else {
            (e.sections.0.len() as u32, e.sections.1.len() as u32, e.sections.2.len() as u32)
        };

        let mut rec = Vec::with_capacity(0x58);
        rec.extend(name_offset.to_le_bytes());
        rec.extend(e.parent.to_le_bytes());
        rec.extend((e.resources.len() as u32).to_le_bytes());
        rec.extend(first_resource.to_le_bytes());
        rec.extend((if e.has_blocks { blocks.len() as u32 - first_block } else { 0 }).to_le_bytes());
        rec.extend((if e.has_blocks { first_block } else { 0 }).to_le_bytes());
        rec.extend(data_offset.to_le_bytes());
        rec.extend(total_compressed.to_le_bytes());
        rec.extend((total_uncompressed as u32).to_le_bytes());
        rec.extend([0, 0, 0, flags]);
        rec.extend(e.global_id.to_le_bytes());
        rec.extend((if e.global_id == -1 { -1i64 } else { e.global_id as i64 * 7 }).to_le_bytes());
        rec.extend(0x1234_5678_9ABC_DEF0u64.wrapping_add(e.global_id as u64).to_le_bytes());
        rec.extend(class.0.to_le_bytes());
        rec.extend(h.to_le_bytes());
        rec.extend(t.to_le_bytes());
        rec.extend(r.to_le_bytes());
        for c in block_counts {
            rec.extend(c.to_le_bytes());
        }
        rec.extend(0u16.to_le_bytes());
        assert_eq!(rec.len(), 0x58);
        file_list.extend(rec);
    }

    let resource_count = resource_list.len() as u32;
    let mut out = Vec::new();
    out.extend(MAGIC.to_le_bytes());
    out.extend(version.raw().to_le_bytes());
    out.extend(0xAAAA_BBBB_CCCC_DDDDu64.to_le_bytes());
    out.extend((entries.len() as u32).to_le_bytes());
    out.extend((loaded_tags.len() as u32).to_le_bytes());
    out.extend((entries.len() as u32 - resource_count).to_le_bytes());
    out.extend((names.len() as u32).to_le_bytes());
    out.extend(resource_count.to_le_bytes());
    out.extend((blocks.len() as u32).to_le_bytes());
    out.extend(0x20A9_32F3_A085_5914u64.to_le_bytes());
    if version == Version::V27 {
        out.extend(0u64.to_le_bytes());
    }
    assert_eq!(out.len(), version.header_size());
    out.extend(file_list);
    out.extend(names);
    for r in resource_list {
        out.extend(r.to_le_bytes());
    }
    for (coff, csize, uoff, usize, compressed) in blocks {
        match version {
            Version::V27 => {
                out.extend(0u64.to_le_bytes());
                out.extend(coff.to_le_bytes());
                out.extend(csize.to_le_bytes());
                out.extend(uoff.to_le_bytes());
                out.extend(usize.to_le_bytes());
                out.extend([compressed as u8, 0, 0, 0]);
                out.extend(0u32.to_le_bytes());
            }
            Version::V23 => {
                out.extend(coff.to_le_bytes());
                out.extend(csize.to_le_bytes());
                out.extend(uoff.to_le_bytes());
                out.extend(usize.to_le_bytes());
                out.extend((compressed as u32).to_le_bytes());
            }
        }
    }
    out.extend(data);
    out
}

/// A small module like the real ones: a load manifest, two tags, one owning two resources.
pub fn sample_entries() -> Vec<TestEntry> {
    let manifest: Vec<u8> = [0x0040_1140i32, 0x0040_2000].iter().flat_map(|id| id.to_le_bytes()).collect();
    let mut bsp = TestEntry::tag(r"levels\test\test.scenario_structure_bsp", 0x0040_1140, "sbsp", &[7u8; 5000]);
    bsp.resources = vec![3, 4];
    vec![
        TestEntry::raw("loadmanifest", &manifest),
        bsp,
        TestEntry::tag(
            r"objects\weapons\rifle\assault_rifle\assault_rifle.weapon",
            0x0040_2000,
            "weap",
            b"weapon data",
        ),
        TestEntry::resource(r"levels\test\test.scenario_structure_bsp[0:mesh resource!*]", 1, &[1u8; 70_000]),
        TestEntry::resource(r"levels\test\test.scenario_structure_bsp[1:tag_resources]", 1, b"more"),
    ]
}

pub const SAMPLE_LOADED_TAGS: &[i32] = &[0x0040_1140, 0x0040_2000];
