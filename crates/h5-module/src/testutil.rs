//! Builds small synthetic modules for tests (no game files in the repo).

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::fourcc::FourCc;
use crate::module::{MAGIC, Version};

/// A fresh folder under the system temp folder, deleted when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("h5tags-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

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

/// (name hash, [(global ID, name hash)], [resource index]).
pub type TestZoneset = (u32, Vec<(i32, u32)>, Vec<u32>);

/// A synthetic tag: header lists plus the tag data blocks they describe.
#[derive(Clone, Default)]
pub struct TestTag {
    /// (class code, tag path without the class, global ID): real tags list
    /// names without the class suffix (format-notes "Tags").
    pub deps: Vec<(&'static str, String, i32)>,
    /// Tag data blocks (section 1), stored back to back.
    pub blocks: Vec<Vec<u8>>,
    /// (kind, target block, field block, field offset, GUID).
    pub structs: Vec<(u16, i32, i32, u32, [u8; 16])>,
    /// (parent struct, target block, field block, field offset).
    pub data_refs: Vec<(i32, i32, i32, u32)>,
    /// (field block, field offset, dependency index or None).
    pub tag_refs: Vec<(i32, u32, Option<usize>)>,
    pub string_ids: Vec<String>,
    pub zonesets: Vec<TestZoneset>,
    pub trailing: Vec<u8>,
}

/// A small but complete tag: a main struct with a tag block field (2 entries
/// in block 1), a tag reference and a data reference (to block 2).
pub fn sample_tag() -> TestTag {
    use crate::murmur3::murmur3_x86_32;
    let default = murmur3_x86_32(b"__default__", 0);
    TestTag {
        deps: vec![
            ("proj", r"objects\weapons\rifle\ar\bullet".into(), 0x0040_1000),
            ("effe", r"fx\impact\bullet_hit".into(), 0x0040_2000),
        ],
        blocks: vec![vec![0u8; 0x60], vec![0u8; 0x40], vec![0xABu8; 0x10]],
        structs: vec![(0, 0, -1, 0, [1; 16]), (1, 1, 0, 0x00, [2; 16])],
        data_refs: vec![(0, 2, 0, 0x40)],
        tag_refs: vec![(0, 0x20, Some(0)), (1, 0x00, None), (1, 0x20, Some(1))],
        string_ids: vec!["__default__".into(), "primary".into()],
        zonesets: vec![(default, vec![(0x0040_1000, default), (0x0040_2000, default)], vec![])],
        trailing: Vec::new(),
    }
}

/// Builds `(header section, tag data section)` for `t`, writing the fields the
/// lists describe into the tag data: tag references get their dependency's
/// global ID (+0x0C) and class (+0x18); data references the target's size (+0x18).
pub fn build_tag(t: &TestTag) -> (Vec<u8>, Vec<u8>) {
    let mut strings = Vec::new();
    let mut add_string = |s: &str| {
        let at = strings.len() as u32;
        strings.extend_from_slice(s.as_bytes());
        strings.push(0);
        at
    };
    let dep_names: Vec<u32> = t.deps.iter().map(|(_, name, _)| add_string(name)).collect();
    let empty = add_string("");
    let sid_offsets: Vec<u32> = t.string_ids.iter().map(|s| add_string(s)).collect();

    // Tag data: blocks back to back, with the described fields filled in.
    let mut blocks = t.blocks.clone();
    for &(field_block, field_offset, dep) in &t.tag_refs {
        // Fields that don't fit their block (deliberately broken tags) are left out.
        let Some(field) = blocks
            .get_mut(field_block as usize)
            .and_then(|b| b.get_mut(field_offset as usize..field_offset as usize + 0x20))
        else {
            continue;
        };
        let (gid, class, name_len) = match dep {
            Some(d) => (t.deps[d].2, FourCc::from_code(t.deps[d].0).unwrap().0, t.deps[d].1.len() as u32),
            None => (-1, u32::MAX, 0),
        };
        field[0x08..0x0C].copy_from_slice(&name_len.to_le_bytes());
        field[0x0C..0x10].copy_from_slice(&gid.to_le_bytes());
        field[0x18..0x1C].copy_from_slice(&class.to_le_bytes());
    }
    for &(_, target, field_block, field_offset) in &t.data_refs {
        let size = if target >= 0 { t.blocks[target as usize].len() as u32 } else { 0 };
        let Some(field) = blocks
            .get_mut(field_block as usize)
            .and_then(|b| b.get_mut(field_offset as usize..field_offset as usize + 0x1C))
        else {
            continue;
        };
        field[0x18..0x1C].copy_from_slice(&size.to_le_bytes());
    }
    let mut block_offsets = Vec::new();
    let mut tag_data = Vec::new();
    for b in &blocks {
        block_offsets.push(tag_data.len() as u64);
        tag_data.extend_from_slice(b);
    }

    // Zoneset info as real tags lay it out: header, entries, each entry's
    // resource list, then each entry's tag list.
    let mut zoneset = Vec::new();
    zoneset.extend((t.zonesets.len() as u64).to_le_bytes());
    zoneset.extend(0x10u64.to_le_bytes());
    let mut resources_at = 0x10 + t.zonesets.len() * 0x28;
    let mut tags_at = resources_at + t.zonesets.iter().map(|z| z.2.len() * 4).sum::<usize>();
    for (name, tags, resources) in &t.zonesets {
        zoneset.extend(name.to_le_bytes());
        zoneset.extend(0i32.to_le_bytes());
        zoneset.extend((resources.len() as u64).to_le_bytes());
        zoneset.extend((resources_at as u64).to_le_bytes());
        zoneset.extend((tags.len() as u64).to_le_bytes());
        zoneset.extend((tags_at as u64).to_le_bytes());
        resources_at += resources.len() * 4;
        tags_at += tags.len() * 8;
    }
    for (_, _, resources) in &t.zonesets {
        for r in resources {
            zoneset.extend(r.to_le_bytes());
        }
    }
    for (_, tags, _) in &t.zonesets {
        for (gid, name) in tags {
            zoneset.extend(gid.to_le_bytes());
            zoneset.extend(name.to_le_bytes());
        }
    }

    let mut lists = Vec::new();
    for (i, (class, _, gid)) in t.deps.iter().enumerate() {
        lists.extend(FourCc::from_code(class).unwrap().0.to_le_bytes());
        lists.extend(dep_names[i].to_le_bytes());
        lists.extend((*gid as i64 * 3).to_le_bytes());
        lists.extend(gid.to_le_bytes());
        lists.extend((-1i32).to_le_bytes());
    }
    for (b, block) in blocks.iter().enumerate() {
        lists.extend((block.len() as u32).to_le_bytes());
        lists.extend(0u16.to_le_bytes());
        lists.extend(1u16.to_le_bytes());
        lists.extend(block_offsets[b].to_le_bytes());
    }
    for (kind, target, field_block, field_offset, guid) in &t.structs {
        lists.extend(guid);
        lists.extend(kind.to_le_bytes());
        lists.extend(0u16.to_le_bytes());
        lists.extend(target.to_le_bytes());
        lists.extend(field_block.to_le_bytes());
        lists.extend(field_offset.to_le_bytes());
    }
    for (parent, target, field_block, field_offset) in &t.data_refs {
        lists.extend(parent.to_le_bytes());
        lists.extend(0i32.to_le_bytes());
        lists.extend(target.to_le_bytes());
        lists.extend(field_block.to_le_bytes());
        lists.extend(field_offset.to_le_bytes());
    }
    for (field_block, field_offset, dep) in &t.tag_refs {
        lists.extend(field_block.to_le_bytes());
        lists.extend(field_offset.to_le_bytes());
        lists.extend(dep.map_or(empty, |d| dep_names[d]).to_le_bytes());
        lists.extend(dep.map_or(-1, |d| d as i32).to_le_bytes());
    }
    for offset in &sid_offsets {
        lists.extend(0u32.to_le_bytes());
        lists.extend(offset.to_le_bytes());
    }

    let header_size = 0x50 + lists.len() + strings.len() + zoneset.len() + t.trailing.len();
    let mut h = Vec::with_capacity(header_size);
    h.extend(u32::from_le_bytes(*b"ucsh").to_le_bytes());
    h.extend(27u32.to_le_bytes());
    h.extend(0x1122_3344_5566_7788u64.to_le_bytes());
    h.extend(0x0BAD_C0DE_0BAD_C0DEu64.to_le_bytes());
    h.extend(0x9999_9999u32.to_le_bytes());
    for count in [t.deps.len(), blocks.len(), t.structs.len(), t.data_refs.len(), t.tag_refs.len(), t.string_ids.len()]
    {
        h.extend((count as u32).to_le_bytes());
    }
    for size in [strings.len(), zoneset.len(), header_size, tag_data.len(), 0] {
        h.extend((size as u32).to_le_bytes());
    }
    h.extend([4, 4, 4, 0]);
    h.extend(0u32.to_le_bytes());
    assert_eq!(h.len(), 0x50);
    h.extend(lists);
    h.extend(strings);
    h.extend(zoneset);
    h.extend(&t.trailing);
    (h, tag_data)
}
