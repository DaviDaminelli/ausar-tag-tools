//! Module layout (AusarDocs `FileFormats/Module.md` for v27; v23 verified on
//! Xbox files, see `docs/format-notes.md`).

use std::borrow::Cow;
use std::fs::File;
use std::path::Path;

use flate2::{Decompress, FlushDecompress, Status};
use memmap2::Mmap;

use crate::fourcc::FourCc;
use crate::global_id::GlobalId;
use crate::le::{i32_at, i64_at, u16_at, u32_at, u64_at};

/// `'dhom'` as a little-endian `u32` (bytes on disk: `mohd`).
pub const MAGIC: u32 = 0x6468_6F6D;

const FILE_ENTRY_SIZE: usize = 0x58;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    /// Halo 5: Guardians (Xbox One).
    V23,
    /// Halo 5: Forge (PC).
    V27,
}

impl Version {
    pub fn from_raw(raw: u32) -> Option<Version> {
        match raw {
            23 => Some(Version::V23),
            27 => Some(Version::V27),
            _ => None,
        }
    }

    pub fn raw(self) -> u32 {
        match self {
            Version::V23 => 23,
            Version::V27 => 27,
        }
    }

    pub fn header_size(self) -> usize {
        match self {
            Version::V23 => 0x30,
            Version::V27 => 0x38,
        }
    }

    pub fn block_entry_size(self) -> usize {
        match self {
            Version::V23 => 0x14,
            Version::V27 => 0x20,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Header {
    pub version: Version,
    pub id: u64,
    pub file_count: u32,
    pub loaded_tag_count: u32,
    pub first_resource_index: u32,
    pub file_names_size: u32,
    pub resource_count: u32,
    pub block_count: u32,
    pub build_version_id: u64,
    /// v27 only.
    pub checksum: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileFlags(pub u8);

impl FileFlags {
    pub const COMPRESSED: u8 = 1;
    pub const HAS_BLOCKS: u8 = 2;
    pub const RAW_FILE: u8 = 4;

    pub fn compressed(self) -> bool {
        self.0 & Self::COMPRESSED != 0
    }
    pub fn has_blocks(self) -> bool {
        self.0 & Self::HAS_BLOCKS != 0
    }
    pub fn raw_file(self) -> bool {
        self.0 & Self::RAW_FILE != 0
    }
}

/// What a file entry is (PRD §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// Global ID ≠ -1.
    Tag,
    /// Not a tag, owned by another entry (`parentFileIndex` ≠ -1).
    Resource,
    /// Anything else, e.g. `loadmanifest`.
    File,
}

impl EntryKind {
    /// The one place this rule is written.
    pub fn of(global_id: GlobalId, parent: Option<u32>) -> EntryKind {
        match (global_id.is_none(), parent) {
            (false, _) => EntryKind::Tag,
            (true, Some(_)) => EntryKind::Resource,
            (true, None) => EntryKind::File,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::Tag => "tag",
            EntryKind::Resource => "resource",
            EntryKind::File => "file",
        }
    }
}

/// One entry of the file list (0x58 bytes in v23 and v27).
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name_offset: u32,
    pub parent_file_index: i32,
    pub resource_count: u32,
    pub first_resource_index: u32,
    pub block_count: u32,
    pub first_block_index: u32,
    pub data_offset: u64,
    pub total_compressed_size: u32,
    pub total_uncompressed_size: u32,
    pub header_alignment: u8,
    pub tag_data_alignment: u8,
    pub resource_data_alignment: u8,
    pub flags: FileFlags,
    pub global_id: GlobalId,
    pub asset_id: i64,
    pub asset_checksum: u64,
    pub class_code: FourCc,
    pub header_size: u32,
    pub tag_data_size: u32,
    pub resource_data_size: u32,
    pub header_block_count: u16,
    pub tag_data_block_count: u16,
    pub resource_block_count: u16,
}

impl FileEntry {
    fn parse(rec: &[u8]) -> FileEntry {
        FileEntry {
            name_offset: u32_at(rec, 0x00),
            parent_file_index: i32_at(rec, 0x04),
            resource_count: u32_at(rec, 0x08),
            first_resource_index: u32_at(rec, 0x0C),
            block_count: u32_at(rec, 0x10),
            first_block_index: u32_at(rec, 0x14),
            data_offset: u64_at(rec, 0x18),
            total_compressed_size: u32_at(rec, 0x20),
            total_uncompressed_size: u32_at(rec, 0x24),
            header_alignment: rec[0x28],
            tag_data_alignment: rec[0x29],
            resource_data_alignment: rec[0x2A],
            flags: FileFlags(rec[0x2B]),
            global_id: GlobalId(i32_at(rec, 0x2C)),
            asset_id: i64_at(rec, 0x30),
            asset_checksum: u64_at(rec, 0x38),
            class_code: FourCc(u32_at(rec, 0x40)),
            header_size: u32_at(rec, 0x44),
            tag_data_size: u32_at(rec, 0x48),
            resource_data_size: u32_at(rec, 0x4C),
            header_block_count: u16_at(rec, 0x50),
            tag_data_block_count: u16_at(rec, 0x52),
            resource_block_count: u16_at(rec, 0x54),
        }
    }

    /// The owning entry of a resource; `None` for `-1`.
    pub fn parent(&self) -> Option<u32> {
        u32::try_from(self.parent_file_index).ok()
    }

    pub fn kind(&self) -> EntryKind {
        EntryKind::of(self.global_id, self.parent())
    }

    /// Where this entry's stored data ends, as an absolute file offset.
    fn data_end(&self, data_start: u64) -> u64 {
        data_start.saturating_add(self.data_offset).saturating_add(self.total_compressed_size as u64)
    }

    fn section_sum(&self) -> u64 {
        self.header_size as u64 + self.tag_data_size as u64 + self.resource_data_size as u64
    }

    /// The entry describes data (section sizes, blocks) but its totals are 0:
    /// the data isn't stored in this module (format-notes "Entries not stored here").
    pub fn is_stored_elsewhere(&self) -> bool {
        self.total_compressed_size == 0 && self.total_uncompressed_size == 0 && self.section_sum() > 0
    }

    /// Uncompressed size of the entry's data, wherever it is stored.
    pub fn data_size(&self) -> u64 {
        if self.is_stored_elsewhere() { self.section_sum() } else { self.total_uncompressed_size as u64 }
    }
}

/// One compression block of a file entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockEntry {
    /// v27 only: the first half (h1) of MurmurHash3 x64 128 (seed 0) of the
    /// block's decompressed bytes (format-notes).
    pub checksum: Option<u64>,
    /// Relative to the file entry's data start.
    pub compressed_offset: u32,
    pub compressed_size: u32,
    pub uncompressed_offset: u32,
    pub uncompressed_size: u32,
    pub compressed: bool,
}

impl BlockEntry {
    fn parse(rec: &[u8], version: Version) -> BlockEntry {
        match version {
            Version::V27 => BlockEntry {
                checksum: Some(u64_at(rec, 0x00)),
                compressed_offset: u32_at(rec, 0x08),
                compressed_size: u32_at(rec, 0x0C),
                uncompressed_offset: u32_at(rec, 0x10),
                uncompressed_size: u32_at(rec, 0x14),
                compressed: rec[0x18] != 0,
            },
            Version::V23 => BlockEntry {
                checksum: None,
                compressed_offset: u32_at(rec, 0x00),
                compressed_size: u32_at(rec, 0x04),
                uncompressed_offset: u32_at(rec, 0x08),
                uncompressed_size: u32_at(rec, 0x0C),
                compressed: u32_at(rec, 0x10) != 0,
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    #[error("not a module file (magic 0x{0:08X}, expected 'dhom')")]
    BadMagic(u32),
    #[error("unsupported module version {0} (supported: 23, 27)")]
    UnsupportedVersion(u32),
    #[error("file too short for the {what}: needs {needed} bytes, file has {len}")]
    Truncated { what: &'static str, needed: u64, len: u64 },
    #[error("file entry {index}: {reason}")]
    BadEntry { index: u32, reason: String },
    #[error("file entry {index}, block {block}: {reason}")]
    BadBlock { index: u32, block: u32, reason: String },
    #[error("file entry {index}: its data ({size} bytes) is not stored in this module")]
    NotStored { index: u32, size: u64 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A parsed module: header and every list, borrowing the file's bytes.
/// Entry data is only read by [`Module::read_entry`].
pub struct Module<'a> {
    bytes: &'a [u8],
    pub header: Header,
    pub entries: Vec<FileEntry>,
    names: &'a [u8],
    pub resources: Vec<u32>,
    pub blocks: Vec<BlockEntry>,
    /// Absolute offset of the data area (right after the block list).
    pub data_start: u64,
}

/// Memory-maps a module file. Only the pages actually read are loaded.
pub fn map_file(path: &Path) -> Result<Mmap, ModuleError> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    if len < 8 {
        return Err(ModuleError::Truncated { what: "header", needed: 8, len });
    }
    // SAFETY: the map is read-only. If another process changes the file while
    // it is mapped, reads may see inconsistent data; every read is bounds-checked
    // against the map's length, so the result is a parse error, not memory unsafety
    // beyond what memmap2 documents.
    Ok(unsafe { Mmap::map(&file)? })
}

fn section(bytes: &[u8], start: u64, len: u64, what: &'static str) -> Result<std::ops::Range<usize>, ModuleError> {
    let end = start.checked_add(len).filter(|&e| e <= bytes.len() as u64).ok_or(ModuleError::Truncated {
        what,
        needed: start.saturating_add(len),
        len: bytes.len() as u64,
    })?;
    Ok(start as usize..end as usize)
}

impl<'a> Module<'a> {
    /// Parses the header and all lists. Reads no entry data.
    pub fn parse(bytes: &'a [u8]) -> Result<Module<'a>, ModuleError> {
        let len = bytes.len() as u64;
        if bytes.len() < 8 {
            return Err(ModuleError::Truncated { what: "header", needed: 8, len });
        }
        let magic = u32_at(bytes, 0);
        if magic != MAGIC {
            return Err(ModuleError::BadMagic(magic));
        }
        let raw_version = u32_at(bytes, 4);
        let version = Version::from_raw(raw_version).ok_or(ModuleError::UnsupportedVersion(raw_version))?;

        let h = &bytes[section(bytes, 0, version.header_size() as u64, "header")?];
        let header = Header {
            version,
            id: u64_at(h, 0x08),
            file_count: u32_at(h, 0x10),
            loaded_tag_count: u32_at(h, 0x14),
            first_resource_index: u32_at(h, 0x18),
            file_names_size: u32_at(h, 0x1C),
            resource_count: u32_at(h, 0x20),
            block_count: u32_at(h, 0x24),
            build_version_id: u64_at(h, 0x28),
            checksum: (version == Version::V27).then(|| u64_at(h, 0x30)),
        };

        let mut pos = version.header_size() as u64;
        let files = section(bytes, pos, header.file_count as u64 * FILE_ENTRY_SIZE as u64, "file list")?;
        pos = files.end as u64;
        let names = section(bytes, pos, header.file_names_size as u64, "file name table")?;
        pos = names.end as u64;
        let res = section(bytes, pos, header.resource_count as u64 * 4, "resource list")?;
        pos = res.end as u64;
        let bes = version.block_entry_size();
        let blocks = section(bytes, pos, header.block_count as u64 * bes as u64, "block list")?;
        let data_start = blocks.end as u64;

        let entries = bytes[files].as_chunks::<FILE_ENTRY_SIZE>().0.iter().map(|r| FileEntry::parse(r)).collect();
        let resources = bytes[res].as_chunks::<4>().0.iter().map(|r| u32::from_le_bytes(*r)).collect();
        let blocks = bytes[blocks].chunks_exact(bes).map(|r| BlockEntry::parse(r, version)).collect();

        Ok(Module { bytes, header, entries, names: &bytes[names], resources, blocks, data_start })
    }

    pub fn file_len(&self) -> u64 {
        self.bytes.len() as u64
    }

    /// Where the entry data ends, according to the file list. Equals
    /// [`Module::file_len`] for a complete module; more means it was cut short.
    pub fn data_end(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.total_compressed_size > 0)
            .map(|e| e.data_end(self.data_start))
            .max()
            .unwrap_or(self.data_start)
    }

    /// Absolute file offset where entry `e`'s data ends.
    pub fn entry_data_end(&self, e: &FileEntry) -> u64 {
        e.data_end(self.data_start)
    }

    /// The file name of entry `index`.
    pub fn name(&self, index: u32) -> Result<&'a str, ModuleError> {
        let bad = |reason: String| ModuleError::BadEntry { index, reason };
        let entry = self.entry(index)?;
        let start = entry.name_offset as usize;
        let rest = self
            .names
            .get(start..)
            .ok_or_else(|| bad(format!("name offset {start} is outside the file name table")))?;
        let end = rest.iter().position(|&b| b == 0).ok_or_else(|| bad("file name is not null-terminated".into()))?;
        std::str::from_utf8(&rest[..end]).map_err(|_| bad("file name is not valid text".into()))
    }

    pub fn entry(&self, index: u32) -> Result<&FileEntry, ModuleError> {
        self.entries.get(index as usize).ok_or_else(|| ModuleError::BadEntry {
            index,
            reason: format!("no such entry ({} entries)", self.entries.len()),
        })
    }

    /// File indices of the resources owned by entry `index`.
    pub fn entry_resources(&self, index: u32) -> Result<&[u32], ModuleError> {
        let e = self.entry(index)?;
        if e.resource_count == 0 {
            return Ok(&[]);
        }
        let start = e.first_resource_index as usize;
        let end = start.checked_add(e.resource_count as usize);
        end.and_then(|end| self.resources.get(start..end)).ok_or_else(|| ModuleError::BadEntry {
            index,
            reason: format!(
                "resources {start}..+{} are outside the resource list ({} entries)",
                e.resource_count,
                self.resources.len()
            ),
        })
    }

    /// The compression blocks of entry `index`. Entries without the `HasBlocks`
    /// flag have one block, described by the entry itself.
    pub fn entry_blocks(&self, index: u32) -> Result<Cow<'_, [BlockEntry]>, ModuleError> {
        let e = self.entry(index)?;
        if !e.flags.has_blocks() {
            return Ok(Cow::Owned(vec![BlockEntry {
                checksum: None,
                compressed_offset: 0,
                compressed_size: e.total_compressed_size,
                uncompressed_offset: 0,
                uncompressed_size: e.total_uncompressed_size,
                compressed: e.flags.compressed(),
            }]));
        }
        let start = e.first_block_index as usize;
        let end = start.checked_add(e.block_count as usize);
        end.and_then(|end| self.blocks.get(start..end)).map(Cow::Borrowed).ok_or_else(|| ModuleError::BadEntry {
            index,
            reason: format!(
                "blocks {start}..+{} are outside the block list ({} entries)",
                e.block_count,
                self.blocks.len()
            ),
        })
    }

    /// Decompresses the whole of entry `index`.
    pub fn read_entry(&self, index: u32) -> Result<Vec<u8>, ModuleError> {
        let e = self.entry(index)?;
        if e.is_stored_elsewhere() {
            return Err(ModuleError::NotStored { index, size: e.data_size() });
        }
        let mut out = vec![0u8; e.total_uncompressed_size as usize];
        if out.is_empty() {
            return Ok(out);
        }
        let blocks = self.entry_blocks(index)?;
        for (b, block) in blocks.iter().enumerate() {
            let bad = |reason: String| ModuleError::BadBlock { index, block: b as u32, reason };
            let src_start = self
                .data_start
                .checked_add(e.data_offset)
                .and_then(|s| s.checked_add(block.compressed_offset as u64))
                .ok_or_else(|| bad("data offset overflows".into()))?;
            let src = section(self.bytes, src_start, block.compressed_size as u64, "block data")
                .map_err(|err| bad(err.to_string()))?;
            let src = &self.bytes[src];
            let dst_start = block.uncompressed_offset as usize;
            let dst = dst_start
                .checked_add(block.uncompressed_size as usize)
                .and_then(|end| out.get_mut(dst_start..end))
                .ok_or_else(|| {
                    bad(format!("uncompressed range is outside the entry's {} bytes", e.total_uncompressed_size))
                })?;
            if block.compressed {
                inflate(src, dst).map_err(bad)?;
            } else if src.len() == dst.len() {
                dst.copy_from_slice(src);
            } else {
                return Err(bad(format!(
                    "uncompressed block has {} stored bytes but {} expected",
                    src.len(),
                    dst.len()
                )));
            }
        }
        Ok(out)
    }
}

/// Inflates one zlib stream into `dst`, which must be filled exactly.
fn inflate(src: &[u8], dst: &mut [u8]) -> Result<(), String> {
    let mut z = Decompress::new(true);
    let status = z.decompress(src, dst, FlushDecompress::Finish).map_err(|e| format!("zlib: {e}"))?;
    let produced = z.total_out() as usize;
    if status != Status::StreamEnd {
        return Err(if produced == dst.len() {
            format!("zlib stream holds more than the stated {} bytes", dst.len())
        } else {
            format!("zlib stream ended early: {produced} of {} bytes", dst.len())
        });
    }
    if produced != dst.len() {
        return Err(format!("decompressed to {produced} bytes, expected {}", dst.len()));
    }
    Ok(())
}
