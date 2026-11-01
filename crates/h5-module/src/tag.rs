//! The tag file inside a tag entry: its header section (AusarDocs
//! `FileFormats/CachedTag.md`; verified on every sample tag, see
//! `docs/format-notes.md` "Tags"). Reads no tag data except where a check
//! compares the header's lists with the fields they describe.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use crate::check::{CheckFailure, Failures, Item, checks};
use crate::fourcc::FourCc;
use crate::global_id::GlobalId;
use crate::le::{i32_at, i64_at, u16_at, u32_at, u64_at};
use crate::module::{FileEntry, Section};
use crate::murmur3::murmur3_x86_32;
use crate::text::decode_cp1252;

/// `'hscu'` as a little-endian `u32` (bytes on disk: `ucsh`).
pub const TAG_MAGIC: u32 = u32::from_le_bytes(*b"ucsh");
/// The only tag header version seen (both builds).
pub const TAG_VERSION: u32 = 27;

const HEADER_SIZE: usize = 0x50;
const DEPENDENCY_SIZE: usize = 0x18;
const DATA_BLOCK_SIZE: usize = 0x10;
const TAG_STRUCT_SIZE: usize = 0x20;
const DATA_REFERENCE_SIZE: usize = 0x14;
const TAG_REFERENCE_SIZE: usize = 0x10;
const STRING_ID_SIZE: usize = 0x8;
const ZONESET_HEADER_SIZE: usize = 0x10;
const ZONESET_ENTRY_SIZE: usize = 0x28;
const ZONESET_TAG_SIZE: usize = 0x8;
const ZONESET_RESOURCE_SIZE: usize = 0x4;

/// Sizes of the tag data fields the header's lists point at (PRD §5.2). The
/// definition reader (M3) uses these too.
pub mod field_size {
    pub const TAG_BLOCK: u32 = 0x1C;
    pub const DATA_REFERENCE: u32 = 0x1C;
    pub const TAG_REFERENCE: u32 = 0x20;
    pub const PAGEABLE_RESOURCE: u32 = 0x10;
}

/// The fixed 0x50-byte header. `unknown*` fields are kept as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderFields {
    pub version: u32,
    pub unknown8: u64,
    pub asset_checksum: u64,
    pub unknown18: u32,
    pub dependency_count: u32,
    pub data_block_count: u32,
    pub tag_struct_count: u32,
    pub data_reference_count: u32,
    pub tag_reference_count: u32,
    pub string_id_count: u32,
    pub string_table_size: u32,
    pub zoneset_size: u32,
    pub header_size: u32,
    pub data_size: u32,
    pub resource_data_size: u32,
    pub header_alignment: u8,
    pub tag_data_alignment: u8,
    pub resource_data_alignment: u8,
    pub unknown4b: u8,
    pub unknown4c: u32,
}

/// A tag this tag references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    pub class_code: FourCc,
    /// Offset of its file name in the string table.
    pub name_offset: u32,
    pub asset_id: i64,
    pub global_id: GlobalId,
    pub unknown14: i32,
}

/// A region of the tag's data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataBlock {
    pub size: u32,
    pub unknown4: u16,
    /// 0 header, 1 tag data, 2 resource data (see [`DataBlock::section`]).
    pub section: u16,
    /// Relative to the start of its section.
    pub offset: u64,
}

impl DataBlock {
    pub fn section(&self) -> Option<Section> {
        match self.section {
            0 => Some(Section::Header),
            1 => Some(Section::TagData),
            2 => Some(Section::ResourceData),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructKind {
    /// The tag's top-level fields.
    Main,
    /// The entries of a tag block field.
    TagBlock,
    /// A pageable resource field.
    Resource,
    /// An editor-only struct.
    Custom,
    Unknown(u16),
}

impl StructKind {
    fn from_raw(raw: u16) -> StructKind {
        match raw {
            0 => StructKind::Main,
            1 => StructKind::TagBlock,
            2 => StructKind::Resource,
            3 => StructKind::Custom,
            other => StructKind::Unknown(other),
        }
    }
}

/// One struct instance: what it is, which data block holds it, and which
/// field points at it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagStruct {
    /// The struct type's GUID (compared with definitions in M3, Q3).
    pub guid: [u8; 16],
    pub kind: StructKind,
    pub unknown12: u16,
    /// The data block holding the struct (main and tag block structs); for a
    /// resource struct probably the resource. `None` = null.
    pub target: Option<u32>,
    /// The data block holding the field that points here (`None` for the main struct).
    pub field_block: Option<u32>,
    pub field_offset: u32,
}

/// A data reference field and the data block it points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataReference {
    /// The tag struct containing the field.
    pub parent_struct: Option<u32>,
    pub unknown4: i32,
    /// `None` = null reference.
    pub target: Option<u32>,
    pub field_block: Option<u32>,
    pub field_offset: u32,
}

/// A tag reference field (null ones included) and the dependency it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagReference {
    pub field_block: Option<u32>,
    pub field_offset: u32,
    /// Offset of the referenced tag's file name in the string table.
    pub name_offset: u32,
    /// `None` = null reference.
    pub dependency: Option<u32>,
}

/// A string_id used by the tag; its value is the Murmur3 x86 32 hash of the string.
/// What a tag reference field holds (read from the tag data at the
/// reference's location). Null when the global ID is -1, whatever the
/// dependency index says (format-notes "Tags").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferenceField {
    /// Byte length of the listed name (+0x08; 0 when there is none).
    pub name_length: u32,
    pub global_id: GlobalId,
    pub class_code: FourCc,
}

impl ReferenceField {
    pub fn is_null(&self) -> bool {
        self.global_id == GlobalId::NONE
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringIdEntry {
    pub unknown0: u32,
    pub string_offset: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Zoneset {
    /// A string_id (hash) of a string in the tag's own string_id list.
    pub name: u32,
    /// Always 0 (padding; format-notes "Tags").
    pub unknown4: i32,
    /// The tag's resources this zoneset loads, as indices into the file
    /// entry's resource list: a count at +0x08 and an offset at +0x10 of
    /// 4-byte indices (not in AusarDocs; format-notes "Tags").
    pub resources: Vec<u32>,
    pub tags: Vec<ZonesetTag>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZonesetTag {
    pub global_id: GlobalId,
    /// A string_id (hash).
    pub zoneset_name: u32,
}

/// A tag's header section, every documented list parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagHeader {
    pub fields: HeaderFields,
    pub dependencies: Vec<Dependency>,
    pub data_blocks: Vec<DataBlock>,
    pub tag_structs: Vec<TagStruct>,
    pub data_references: Vec<DataReference>,
    pub tag_references: Vec<TagReference>,
    pub string_ids: Vec<StringIdEntry>,
    pub string_table: Vec<u8>,
    pub zonesets: Vec<Zoneset>,
    /// Bytes after the zoneset info, up to the end of the header section; not
    /// in AusarDocs, meaning unknown (PRD §12 Q11).
    pub trailing: Vec<u8>,
}

/// string_id hash → string, for one tag ([`TagHeader::string_id_names`]).
pub struct StringIdNames<'t> {
    tag: &'t TagHeader,
    /// Hash → offset in the string table.
    by_hash: HashMap<u32, u32>,
}

impl<'t> StringIdNames<'t> {
    /// The string whose hash is `hash`, if the tag lists it.
    pub fn get(&self, hash: u32) -> Option<Cow<'t, str>> {
        self.tag.string(*self.by_hash.get(&hash)?)
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum TagError {
    #[error("not a tag header (magic 0x{0:08X}, expected 'hscu')")]
    BadMagic(u32),
    #[error("unsupported tag header version {0} (supported: 27)")]
    UnsupportedVersion(u32),
    #[error("tag header too short for the {what}: needs {needed} bytes, has {len}")]
    Truncated { what: &'static str, needed: u64, len: u64 },
    #[error("zoneset info: {0}")]
    BadZoneset(String),
}

/// `-1` (or any negative index) is "none".
fn index(raw: i32) -> Option<u32> {
    u32::try_from(raw).ok()
}

fn truncated(what: &'static str, needed: u64, len: usize) -> TagError {
    TagError::Truncated { what, needed, len: len as u64 }
}

/// The next `len` bytes at `*at`, advancing `*at`.
fn take<'a>(bytes: &'a [u8], at: &mut usize, len: u64, what: &'static str) -> Result<&'a [u8], TagError> {
    let end = *at as u64 + len;
    if end > bytes.len() as u64 {
        return Err(truncated(what, end, bytes.len()));
    }
    let taken = &bytes[*at..end as usize];
    *at = end as usize;
    Ok(taken)
}

/// Reads `count` records of `size` bytes at `*at`, advancing `*at`.
fn list<T>(
    bytes: &[u8],
    at: &mut usize,
    count: u32,
    size: usize,
    what: &'static str,
    parse: impl Fn(&[u8]) -> T,
) -> Result<Vec<T>, TagError> {
    Ok(take(bytes, at, count as u64 * size as u64, what)?.chunks_exact(size).map(parse).collect())
}

/// The zoneset info: a 0x10 header (count, list offset), 0x28-byte entries,
/// and each entry's 8-byte tag list, all offsets relative to the section.
fn parse_zonesets(zs: &[u8]) -> Result<Vec<Zoneset>, TagError> {
    if zs.is_empty() {
        return Ok(Vec::new());
    }
    let bad = |what: String| TagError::BadZoneset(what);
    let fits = |start: u64, count: u64, size: usize| {
        count.checked_mul(size as u64).and_then(|n| start.checked_add(n)).is_some_and(|end| end <= zs.len() as u64)
    };
    if zs.len() < ZONESET_HEADER_SIZE {
        return Err(bad(format!("{} bytes, too short for its header", zs.len())));
    }
    let (count, list_at) = (u64_at(zs, 0), u64_at(zs, 8));
    if !fits(list_at, count, ZONESET_ENTRY_SIZE) {
        return Err(bad(format!("{count} zonesets at +{list_at} don't fit its {} bytes", zs.len())));
    }
    (0..count as usize)
        .map(|z| {
            let e = &zs[list_at as usize + z * ZONESET_ENTRY_SIZE..][..ZONESET_ENTRY_SIZE];
            let (resource_count, resources_at) = (u64_at(e, 0x08), u64_at(e, 0x10));
            if !fits(resources_at, resource_count, ZONESET_RESOURCE_SIZE) {
                return Err(bad(format!("zoneset {z}: {resource_count} resources at +{resources_at} don't fit")));
            }
            let (tag_count, tags_at) = (u64_at(e, 0x18), u64_at(e, 0x20));
            if !fits(tags_at, tag_count, ZONESET_TAG_SIZE) {
                return Err(bad(format!("zoneset {z}: {tag_count} tags at +{tags_at} don't fit")));
            }
            let resources = zs[resources_at as usize..][..resource_count as usize * ZONESET_RESOURCE_SIZE]
                .as_chunks::<ZONESET_RESOURCE_SIZE>()
                .0
                .iter()
                .map(|r| u32::from_le_bytes(*r))
                .collect();
            let tags = zs[tags_at as usize..][..tag_count as usize * ZONESET_TAG_SIZE]
                .as_chunks::<ZONESET_TAG_SIZE>()
                .0
                .iter()
                .map(|t| ZonesetTag { global_id: GlobalId(i32_at(t, 0)), zoneset_name: u32_at(t, 4) })
                .collect();
            Ok(Zoneset { name: u32_at(e, 0), unknown4: i32_at(e, 4), resources, tags })
        })
        .collect()
}

impl TagHeader {
    /// Parses a tag entry's header section ([`Section::Header`]).
    pub fn parse(bytes: &[u8]) -> Result<TagHeader, TagError> {
        if bytes.len() < HEADER_SIZE {
            return Err(truncated("header", HEADER_SIZE as u64, bytes.len()));
        }
        let magic = u32_at(bytes, 0);
        if magic != TAG_MAGIC {
            return Err(TagError::BadMagic(magic));
        }
        let h = bytes;
        let fields = HeaderFields {
            version: u32_at(h, 0x04),
            unknown8: u64_at(h, 0x08),
            asset_checksum: u64_at(h, 0x10),
            unknown18: u32_at(h, 0x18),
            dependency_count: u32_at(h, 0x1C),
            data_block_count: u32_at(h, 0x20),
            tag_struct_count: u32_at(h, 0x24),
            data_reference_count: u32_at(h, 0x28),
            tag_reference_count: u32_at(h, 0x2C),
            string_id_count: u32_at(h, 0x30),
            string_table_size: u32_at(h, 0x34),
            zoneset_size: u32_at(h, 0x38),
            header_size: u32_at(h, 0x3C),
            data_size: u32_at(h, 0x40),
            resource_data_size: u32_at(h, 0x44),
            header_alignment: h[0x48],
            tag_data_alignment: h[0x49],
            resource_data_alignment: h[0x4A],
            unknown4b: h[0x4B],
            unknown4c: u32_at(h, 0x4C),
        };
        if fields.version != TAG_VERSION {
            return Err(TagError::UnsupportedVersion(fields.version));
        }

        let mut at = HEADER_SIZE;
        let dependencies =
            list(bytes, &mut at, fields.dependency_count, DEPENDENCY_SIZE, "dependency list", |r| Dependency {
                class_code: FourCc(u32_at(r, 0x00)),
                name_offset: u32_at(r, 0x04),
                asset_id: i64_at(r, 0x08),
                global_id: GlobalId(i32_at(r, 0x10)),
                unknown14: i32_at(r, 0x14),
            })?;
        let data_blocks =
            list(bytes, &mut at, fields.data_block_count, DATA_BLOCK_SIZE, "data block list", |r| DataBlock {
                size: u32_at(r, 0x00),
                unknown4: u16_at(r, 0x04),
                section: u16_at(r, 0x06),
                offset: u64_at(r, 0x08),
            })?;
        let tag_structs =
            list(bytes, &mut at, fields.tag_struct_count, TAG_STRUCT_SIZE, "tag struct list", |r| TagStruct {
                guid: r[..16].try_into().expect("16 bytes"),
                kind: StructKind::from_raw(u16_at(r, 0x10)),
                unknown12: u16_at(r, 0x12),
                target: index(i32_at(r, 0x14)),
                field_block: index(i32_at(r, 0x18)),
                field_offset: u32_at(r, 0x1C),
            })?;
        let data_references =
            list(bytes, &mut at, fields.data_reference_count, DATA_REFERENCE_SIZE, "data reference list", |r| {
                DataReference {
                    parent_struct: index(i32_at(r, 0x00)),
                    unknown4: i32_at(r, 0x04),
                    target: index(i32_at(r, 0x08)),
                    field_block: index(i32_at(r, 0x0C)),
                    field_offset: u32_at(r, 0x10),
                }
            })?;
        let tag_references =
            list(bytes, &mut at, fields.tag_reference_count, TAG_REFERENCE_SIZE, "tag reference list", |r| {
                TagReference {
                    field_block: index(i32_at(r, 0x00)),
                    field_offset: u32_at(r, 0x04),
                    name_offset: u32_at(r, 0x08),
                    dependency: index(i32_at(r, 0x0C)),
                }
            })?;
        let string_ids = list(bytes, &mut at, fields.string_id_count, STRING_ID_SIZE, "string_id list", |r| {
            StringIdEntry { unknown0: u32_at(r, 0x00), string_offset: u32_at(r, 0x04) }
        })?;
        let string_table = take(bytes, &mut at, fields.string_table_size as u64, "string table")?.to_vec();
        let zonesets = parse_zonesets(take(bytes, &mut at, fields.zoneset_size as u64, "zoneset info")?)?;
        let trailing = bytes[at..].to_vec();

        Ok(TagHeader {
            fields,
            dependencies,
            data_blocks,
            tag_structs,
            data_references,
            tag_references,
            string_ids,
            string_table,
            zonesets,
            trailing,
        })
    }

    /// The bytes of the null-terminated string at `offset` in the string table
    /// (what a string_id's hash is computed over).
    pub fn string_bytes(&self, offset: u32) -> Option<&[u8]> {
        let rest = self.string_table.get(offset as usize..)?;
        Some(&rest[..rest.iter().position(|&b| b == 0)?])
    }

    /// The string at `offset`, decoded (Windows-1252).
    pub fn string(&self, offset: u32) -> Option<Cow<'_, str>> {
        self.string_bytes(offset).map(decode_cp1252)
    }

    /// Resolves string_id hashes (fields, zoneset names) by hashing the tag's
    /// own string_id strings (PRD §5.2). Build it once per tag.
    pub fn string_id_names(&self) -> StringIdNames<'_> {
        let by_hash = self
            .string_ids
            .iter()
            .filter_map(|s| Some((murmur3_x86_32(self.string_bytes(s.string_offset)?, 0), s.string_offset)))
            .collect();
        StringIdNames { tag: self, by_hash }
    }

    /// The main struct (exactly one in a sound tag).
    pub fn main_struct(&self) -> Option<&TagStruct> {
        self.tag_structs.iter().find(|s| s.kind == StructKind::Main)
    }

    /// Cross-checks of the header's lists against each other and against the
    /// file entry's section sizes. Reads no tag data.
    pub fn check(&self, entry: &FileEntry) -> Vec<CheckFailure> {
        let mut out = Failures::default();
        self.check_sizes(entry, &mut out);
        self.check_data_blocks(&mut out);
        self.check_structs(&mut out);
        self.check_data_references(&mut out);
        self.check_tag_references(&mut out);
        self.check_zonesets(entry, &mut out);
        for (i, d) in self.dependencies.iter().enumerate() {
            if self.string_bytes(d.name_offset).is_none() {
                out.add(
                    checks::DEPENDENCY,
                    item("dependency", i),
                    format!("name offset {} isn't a string", d.name_offset),
                );
            }
        }
        for (i, s) in self.string_ids.iter().enumerate() {
            if self.string_bytes(s.string_offset).is_none() {
                out.add(checks::STRING_ID, item("string_id", i), format!("offset {} isn't a string", s.string_offset));
            }
        }
        out.0
    }

    /// Cross-checks of the header's lists against the tag data they describe:
    /// each tag reference field holds its dependency's global ID and class,
    /// and each data reference field's size equals its target block's size.
    pub fn check_tag_data(&self, tag_data: &[u8]) -> Vec<CheckFailure> {
        let mut out = Failures::default();
        for (i, r) in self.tag_references.iter().enumerate() {
            let Some(f) = self.reference_field(tag_data, r) else { continue };
            let mut fail = |problem: String| out.add(checks::TAG_REFERENCE_FIELD, item("tag reference", i), problem);
            // Every reference: the field states the listed name's length.
            let listed = self.string_bytes(r.name_offset).map_or(0, <[u8]>::len) as u32;
            if f.name_length != listed {
                fail(format!("field says the name is {} bytes, the listed name has {listed}", f.name_length));
            }
            // With a dependency, the field holds its ID and class. Without one, the
            // field may still name a tag: the list isn't the whole truth (format-notes).
            if let Some(d) = r.dependency.and_then(|d| self.dependencies.get(d as usize)) {
                if f.global_id != d.global_id {
                    fail(format!("field holds global ID {}, its dependency is {}", f.global_id, d.global_id));
                }
                if f.class_code != d.class_code {
                    fail(format!("field holds class {}, its dependency is {}", f.class_code, d.class_code));
                }
            }
        }
        for (i, d) in self.data_references.iter().enumerate() {
            let Some(f) = self.field_bytes(tag_data, d.field_block, d.field_offset, field_size::DATA_REFERENCE) else {
                continue;
            };
            let size = u32_at(f, 0x18);
            let want = d.target.and_then(|t| self.data_blocks.get(t as usize)).map_or(0, |b| b.size);
            if size != want {
                out.add(
                    checks::DATA_REFERENCE_FIELD,
                    item("data reference", i),
                    format!("field says {size} bytes, target block has {want}"),
                );
            }
        }
        out.0
    }

    /// What tag reference `r`'s field holds; `None` if it lies outside its block.
    pub fn reference_field(&self, tag_data: &[u8], r: &TagReference) -> Option<ReferenceField> {
        let f = self.field_bytes(tag_data, r.field_block, r.field_offset, field_size::TAG_REFERENCE)?;
        Some(ReferenceField {
            name_length: u32_at(f, 0x08),
            global_id: GlobalId(i32_at(f, 0x0C)),
            class_code: FourCc(u32_at(f, 0x18)),
        })
    }

    /// The `size` bytes of the field at (`block`, `offset`) in the tag data.
    fn field_bytes<'d>(&self, tag_data: &'d [u8], block: Option<u32>, offset: u32, size: u32) -> Option<&'d [u8]> {
        self.block_data(tag_data, block?)?.get(offset as usize..offset as usize + size as usize)
    }

    /// The bytes of tag data block `b` (blocks lie in the tag data section).
    pub fn block_data<'d>(&self, tag_data: &'d [u8], b: u32) -> Option<&'d [u8]> {
        let block = self.data_blocks.get(b as usize).filter(|bl| bl.section() == Some(Section::TagData))?;
        tag_data.get(block.offset as usize..block.offset as usize + block.size as usize)
    }

    /// Size of a section, as the header states it.
    fn section_size(&self, s: Section) -> u64 {
        match s {
            Section::Header => self.fields.header_size as u64,
            Section::TagData => self.fields.data_size as u64,
            Section::ResourceData => self.fields.resource_data_size as u64,
        }
    }

    /// The header's three sizes equal the file entry's.
    fn check_sizes(&self, entry: &FileEntry, out: &mut Failures) {
        let f = &self.fields;
        let header = (f.header_size, f.data_size, f.resource_data_size);
        let file = (entry.header_size, entry.tag_data_size, entry.resource_data_size);
        if header != file {
            out.add(
                checks::TAG_SIZES,
                None,
                format!("header says header/tag data/resource = {header:?}, file entry says {file:?}"),
            );
        }
    }

    /// Each data block lies in a known section, inside it.
    fn check_data_blocks(&self, out: &mut Failures) {
        for (i, b) in self.data_blocks.iter().enumerate() {
            let Some(section) = b.section() else {
                out.add(checks::DATA_BLOCK, item("data block", i), format!("section {} is not 0, 1 or 2", b.section));
                continue;
            };
            if b.offset.saturating_add(b.size as u64) > self.section_size(section) {
                out.add(
                    checks::DATA_BLOCK,
                    item("data block", i),
                    format!(
                        "bytes {}+{} end past the {section:?} section ({} bytes)",
                        b.offset,
                        b.size,
                        self.section_size(section)
                    ),
                );
            }
        }
    }

    /// Exactly one main struct; struct targets are data blocks; the field that
    /// points at a struct lies inside its block.
    fn check_structs(&self, out: &mut Failures) {
        let blocks = self.data_blocks.len() as u32;
        let mains = self.tag_structs.iter().filter(|s| s.kind == StructKind::Main).count();
        if mains != 1 {
            out.add(checks::MAIN_STRUCT, None, format!("{mains} main structs, expected 1"));
        } else if let Some(main) = self.main_struct()
            && main.target.is_none_or(|t| t >= blocks)
        {
            out.add(
                checks::MAIN_STRUCT,
                None,
                format!("main struct target {:?} isn't one of {blocks} data blocks", main.target),
            );
        }
        for (i, s) in self.tag_structs.iter().enumerate() {
            let points_at_block = matches!(s.kind, StructKind::Main | StructKind::TagBlock);
            if points_at_block && let Some(t) = s.target.filter(|&t| t >= blocks) {
                out.add(checks::TAG_STRUCT, item("tag struct", i), format!("target block {t} of {blocks}"));
            }
            let size = match s.kind {
                StructKind::TagBlock => field_size::TAG_BLOCK,
                StructKind::Resource => field_size::PAGEABLE_RESOURCE,
                _ => 0,
            };
            if let Some(problem) = self.field_problem(s.field_block, s.field_offset, size, s.kind != StructKind::Main) {
                out.add(checks::TAG_STRUCT, item("tag struct", i), problem);
            }
        }
    }

    fn check_data_references(&self, out: &mut Failures) {
        let (blocks, structs) = (self.data_blocks.len() as u32, self.tag_structs.len() as u32);
        for (i, d) in self.data_references.iter().enumerate() {
            let mut fail = |problem: String| out.add(checks::DATA_REFERENCE, item("data reference", i), problem);
            if let Some(p) = d.parent_struct.filter(|&p| p >= structs) {
                fail(format!("parent struct {p} of {structs}"));
            }
            if let Some(t) = d.target.filter(|&t| t >= blocks) {
                fail(format!("target block {t} of {blocks}"));
            }
            if let Some(problem) = self.field_problem(d.field_block, d.field_offset, field_size::DATA_REFERENCE, true) {
                fail(problem);
            }
        }
    }

    fn check_tag_references(&self, out: &mut Failures) {
        let deps = self.dependencies.len() as u32;
        for (i, r) in self.tag_references.iter().enumerate() {
            let mut fail = |problem: String| out.add(checks::TAG_REFERENCE, item("tag reference", i), problem);
            if let Some(d) = r.dependency.filter(|&d| d >= deps) {
                fail(format!("dependency {d} of {deps}"));
            }
            if self.string_bytes(r.name_offset).is_none() {
                fail(format!("name offset {} isn't a string", r.name_offset));
            }
            if let Some(problem) = self.field_problem(r.field_block, r.field_offset, field_size::TAG_REFERENCE, true) {
                fail(problem);
            }
        }
    }

    /// The zoneset info holds exactly its header, entries, resource lists and
    /// tag lists, and lists only the entry's resources (format-notes "Tags").
    /// A resource no zoneset lists is allowed: some real tags have one.
    fn check_zonesets(&self, entry: &FileEntry, out: &mut Failures) {
        let resources: usize = self.zonesets.iter().map(|z| z.resources.len()).sum();
        let tags: usize = self.zonesets.iter().map(|z| z.tags.len()).sum();
        let contents = match self.zonesets.len() {
            0 => 0,
            n => {
                ZONESET_HEADER_SIZE
                    + n * ZONESET_ENTRY_SIZE
                    + resources * ZONESET_RESOURCE_SIZE
                    + tags * ZONESET_TAG_SIZE
            }
        };
        if contents != self.fields.zoneset_size as usize {
            out.add(
                checks::ZONESET,
                None,
                format!("zoneset info is {} bytes, its contents {contents}", self.fields.zoneset_size),
            );
        }
        let count = entry.resource_count;
        for (z, zs) in self.zonesets.iter().enumerate() {
            for &r in zs.resources.iter().filter(|&&r| r >= count) {
                out.add(checks::ZONESET, item("zoneset", z), format!("lists resource {r}, the entry has {count}"));
            }
        }
    }

    /// Why a field of `size` bytes at (`block`, `offset`) isn't inside its
    /// block, if it isn't. `required`: the field must have a block.
    fn field_problem(&self, block: Option<u32>, offset: u32, size: u32, required: bool) -> Option<String> {
        let Some(b) = block else { return required.then(|| "no field block".to_owned()) };
        let Some(bl) = self.data_blocks.get(b as usize) else {
            return Some(format!("field block {b} of {}", self.data_blocks.len()));
        };
        (offset as u64 + size as u64 > bl.size as u64)
            .then(|| format!("field at {offset} (+{size} bytes) ends past block {b} ({} bytes)", bl.size))
    }
}

fn item(kind: &'static str, index: usize) -> Option<Item> {
    Some(Item { kind, index: index as u32 })
}

impl fmt::Display for StructKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StructKind::Main => f.write_str("main"),
            StructKind::TagBlock => f.write_str("tag block"),
            StructKind::Resource => f.write_str("resource"),
            StructKind::Custom => f.write_str("custom"),
            StructKind::Unknown(n) => write!(f, "unknown ({n})"),
        }
    }
}

#[cfg(test)]
#[path = "tag_tests.rs"]
mod tests;
