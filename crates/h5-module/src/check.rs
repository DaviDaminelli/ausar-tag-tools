//! Structural cross-checks of a module (PRD §9, M1). Each compares two parts of
//! the file that are stored separately but must agree, so a misread layout fails.

use std::collections::HashMap;
use std::fmt;

use crate::global_id::GlobalId;
use crate::module::{EntryKind, FileEntry, Module};
use crate::names::LOAD_MANIFEST;

/// Check names, stable for tests, reports and the index (`ModuleIssue::check`).
pub mod checks {
    pub const FIRST_RESOURCE_INDEX: &str = "first_resource_index";
    pub const RESOURCE_LIST: &str = "resource_list";
    pub const NAME: &str = "name";
    pub const DATA_BOUNDS: &str = "data_bounds";
    pub const RESOURCE_RANGE: &str = "resource_range";
    pub const RESOURCE_PARENT: &str = "resource_parent";
    pub const BLOCK_RANGE: &str = "block_range";
    pub const BLOCKS_CONTIGUOUS: &str = "blocks_contiguous";
    pub const BLOCKS_COMPRESSED_RANGE: &str = "blocks_compressed_range";
    pub const BLOCKS_TOTAL: &str = "blocks_total";
    pub const SECTION_SIZES: &str = "section_sizes";
    pub const SECTION_BLOCK_COUNTS: &str = "section_block_counts";
    pub const DUPLICATE_GLOBAL_ID: &str = "duplicate_global_id";
    pub const LOADMANIFEST_SIZE: &str = "loadmanifest_size";
    pub const DATA_END: &str = "data_end";

    // Tag header checks (`tag.rs`).
    pub const TAG_SIZES: &str = "tag_sizes";
    pub const DATA_BLOCK: &str = "data_block";
    pub const MAIN_STRUCT: &str = "main_struct";
    pub const TAG_STRUCT: &str = "tag_struct";
    pub const DATA_REFERENCE: &str = "data_reference";
    pub const TAG_REFERENCE: &str = "tag_reference";
    pub const DEPENDENCY: &str = "dependency";
    pub const STRING_ID: &str = "string_id";
    pub const TAG_REFERENCE_FIELD: &str = "tag_reference_field";
    pub const DATA_REFERENCE_FIELD: &str = "data_reference_field";
}

/// What a failure is about: a kind of item and its index (`entry 3`, `dependency 7`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub kind: &'static str,
    pub index: u32,
}

impl Item {
    pub fn entry(index: u32) -> Item {
        Item { kind: "entry", index }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckFailure {
    /// One of [`checks`].
    pub check: &'static str,
    /// The item concerned, if any.
    pub item: Option<Item>,
    pub detail: String,
}

impl fmt::Display for CheckFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.item {
            Some(item) => write!(f, "{} ({} {}): {}", self.check, item.kind, item.index, self.detail),
            None => write!(f, "{}: {}", self.check, self.detail),
        }
    }
}

/// Collects failures (module checks here, tag checks in `tag`).
#[derive(Default)]
pub(crate) struct Failures(pub(crate) Vec<CheckFailure>);

impl Failures {
    pub(crate) fn add(&mut self, check: &'static str, item: Option<Item>, detail: String) {
        self.0.push(CheckFailure { check, item, detail });
    }

    fn entry(&mut self, check: &'static str, index: u32, detail: String) {
        self.add(check, Some(Item::entry(index)), detail);
    }
}

impl Module<'_> {
    /// Runs every structural cross-check. Reads no entry data.
    pub fn check(&self) -> Vec<CheckFailure> {
        let mut out = Failures::default();
        self.check_header(&mut out);
        self.check_resource_list(&mut out);
        let mut seen_ids: HashMap<GlobalId, u32> = HashMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            let i = i as u32;
            self.check_name(i, e, &mut out);
            self.check_data_bounds(i, e, &mut out);
            self.check_resources(i, &mut out);
            self.check_blocks(i, e, &mut out);
            check_sections(i, e, &mut out);
            if e.kind() == EntryKind::Tag
                && let Some(first) = seen_ids.insert(e.global_id, i)
            {
                out.add(
                    checks::DUPLICATE_GLOBAL_ID,
                    Some(Item::entry(i)),
                    format!("global ID {} also in entry {first}", e.global_id),
                );
            }
        }
        self.check_data_end(&mut out);
        out.0
    }

    /// Two header fields that must agree.
    fn check_header(&self, out: &mut Failures) {
        let h = &self.header;
        if h.file_count.checked_sub(h.resource_count) != Some(h.first_resource_index) {
            out.add(
                checks::FIRST_RESOURCE_INDEX,
                None,
                format!(
                    "firstResourceIndex is {}, but numFiles − numResources = {} − {}",
                    h.first_resource_index, h.file_count, h.resource_count
                ),
            );
        }
    }

    fn check_resource_list(&self, out: &mut Failures) {
        let files = self.header.file_count;
        for (i, &file) in self.resources.iter().enumerate() {
            if file >= files {
                out.add(
                    checks::RESOURCE_LIST,
                    None,
                    format!("resource list item {i} points to file {file} of {files}"),
                );
            }
        }
    }

    /// The name is readable; a load manifest's size matches the header.
    fn check_name(&self, i: u32, e: &FileEntry, out: &mut Failures) {
        match self.name(i) {
            Err(err) => out.entry(checks::NAME, i, err.to_string()),
            Ok(LOAD_MANIFEST) if e.total_uncompressed_size as u64 != self.header.loaded_tag_count as u64 * 4 => out
                .add(
                    checks::LOADMANIFEST_SIZE,
                    Some(Item::entry(i)),
                    format!(
                        "loadmanifest is {} bytes, header says {} tags × 4",
                        e.total_uncompressed_size, self.header.loaded_tag_count
                    ),
                ),
            Ok(_) => {}
        }
    }

    /// The entry's data lies inside the file.
    fn check_data_bounds(&self, i: u32, e: &FileEntry, out: &mut Failures) {
        if self.entry_data_end(e) > self.file_len() {
            out.add(
                checks::DATA_BOUNDS,
                Some(Item::entry(i)),
                format!(
                    "data at {}+{} (+{} bytes) ends past the file ({} bytes)",
                    self.data_start,
                    e.data_offset,
                    e.total_compressed_size,
                    self.file_len()
                ),
            );
        }
    }

    /// Resources point back to their owner.
    fn check_resources(&self, i: u32, out: &mut Failures) {
        let files = match self.entry_resources(i) {
            Ok(files) => files,
            Err(err) => return out.entry(checks::RESOURCE_RANGE, i, err.to_string()),
        };
        for &file in files {
            if let Some(r) = self.entries.get(file as usize)
                && r.parent() != Some(i)
            {
                out.add(
                    checks::RESOURCE_PARENT,
                    Some(Item::entry(i)),
                    format!("owns file {file}, whose parentFileIndex is {}", r.parent_file_index),
                );
            }
        }
    }

    /// Blocks tile the uncompressed data and stay inside the compressed data.
    /// For an entry not stored here, they must still tile its sections.
    fn check_blocks(&self, i: u32, e: &FileEntry, out: &mut Failures) {
        if !e.flags.has_blocks() {
            return;
        }
        let blocks = match self.entry_blocks(i) {
            Ok(blocks) => blocks,
            Err(err) => return out.entry(checks::BLOCK_RANGE, i, err.to_string()),
        };
        let mut expected = 0u64;
        for (b, block) in blocks.iter().enumerate() {
            if block.uncompressed_offset as u64 != expected {
                let detail = format!("block {b} starts at {}, expected {expected}", block.uncompressed_offset);
                return out.entry(checks::BLOCKS_CONTIGUOUS, i, detail);
            }
            expected += block.uncompressed_size as u64;
            let compressed_end = block.compressed_offset as u64 + block.compressed_size as u64;
            if !e.is_stored_elsewhere() && compressed_end > e.total_compressed_size as u64 {
                out.add(
                    checks::BLOCKS_COMPRESSED_RANGE,
                    Some(Item::entry(i)),
                    format!(
                        "block {b} ({}+{}) ends past the entry's {} compressed bytes",
                        block.compressed_offset, block.compressed_size, e.total_compressed_size
                    ),
                );
            }
        }
        if expected != e.data_size() {
            out.add(
                checks::BLOCKS_TOTAL,
                Some(Item::entry(i)),
                format!("blocks add up to {expected} bytes, entry says {}", e.data_size()),
            );
        }
    }

    /// The last entry's data ends exactly at the end of the file.
    fn check_data_end(&self, out: &mut Failures) {
        let end = self.data_end();
        if end != self.file_len() {
            out.add(checks::DATA_END, None, format!("entry data ends at {end}, file is {} bytes", self.file_len()));
        }
    }
}

/// Section sizes and section block counts agree with the totals.
fn check_sections(i: u32, e: &FileEntry, out: &mut Failures) {
    if e.flags.raw_file() || e.data_size() == 0 {
        return;
    }
    let sum = e.header_size as u64 + e.tag_data_size as u64 + e.resource_data_size as u64;
    if sum != e.data_size() {
        out.add(
            checks::SECTION_SIZES,
            Some(Item::entry(i)),
            format!(
                "header {} + tag data {} + resource data {} = {sum}, entry says {}",
                e.header_size,
                e.tag_data_size,
                e.resource_data_size,
                e.data_size()
            ),
        );
    }
    let blocks = e.header_block_count as u32 + e.tag_data_block_count as u32 + e.resource_block_count as u32;
    if e.flags.has_blocks() && blocks != e.block_count {
        out.add(
            checks::SECTION_BLOCK_COUNTS,
            Some(Item::entry(i)),
            format!(
                "section block counts {} + {} + {} = {blocks}, entry has {} blocks",
                e.header_block_count, e.tag_data_block_count, e.resource_block_count, e.block_count
            ),
        );
    }
}
