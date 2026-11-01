//! `header-dump <tag>` (PRD §6.7): every list of a tag's header, as stored.
//! A developer command, CLI only: not an MCP tool, so its columns are not in
//! `AGENT_GUIDE.md`.

use h5_module::murmur3::murmur3_x86_32;
use std::collections::HashMap;

use h5_module::tag::{DataBlock, Dependency, StringIdNames, StructKind, TagHeader};
use h5_module::{FourCc, GlobalId, Section};

use super::args::{self, BuildArg};
use super::present;
use crate::catalog::{Build, Copy, Tag};
use crate::output::{Cell, Column, Output, Part, Table, ToolError, col};
use crate::session::Session;
use crate::stored::{StoredTag, load_definition, read_stored_tag};
use crate::tag_view::join;

/// All sections of a tag's header, as stored (developer command; not a tool).
#[derive(Clone, Debug, clap::Args)]
pub struct HeaderDump {
    /// The tag: global ID, tag path or display name.
    pub tag: String,
    /// Read the copy in the module whose path contains this.
    #[arg(long)]
    pub module: Option<String>,
    #[command(flatten)]
    pub build: BuildArg,
}

impl HeaderDump {
    pub fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let t = args::tag_copy(s, &self.tag, &self.build, self.module.as_deref())?;
        let stored = read_stored_tag(t.build, t.copy).map_err(present::read_error)?;
        let h = &stored.header;
        let names = h.string_id_names();
        let paths = reference_paths(t.build, t.tag, &stored)?;
        let mut header = header_part(h, &t.build.module(t.copy).rel_path);
        header.table.hints.extend(t.other_copies_hint);
        Ok(Output::Parts {
            build: s.label(t.build),
            parts: vec![
                header,
                dependencies(t.build, h),
                data_blocks(h),
                tag_structs(h),
                data_references(h),
                tag_references(t.build, &stored, paths.as_ref()),
                string_ids(h),
                zonesets(h, &names),
                zoneset_resources(t.build, t.copy, h, &names),
                zoneset_tags(t.build, h, &names),
            ],
        })
    }
}

/// Columns of this command only (CLI, so not in `AGENT_GUIDE.md`).
mod column {
    use crate::output::Column;
    pub const INDEX: Column = Column { header: "#", key: "index" };
    pub const FIELD: Column = Column { header: "field", key: "field" };
    pub const VALUE: Column = Column { header: "value", key: "value" };
    pub const LISTED_NAME: Column = Column { header: "listed name", key: "listed_name" };
    pub const INDEXED: Column = Column { header: "indexed", key: "indexed" };
    pub const ASSET_ID: Column = Column { header: "asset ID", key: "asset_id" };
    pub const SECTION: Column = Column { header: "section", key: "section" };
    pub const OFFSET: Column = Column { header: "offset", key: "offset" };
    pub const BYTES: Column = Column { header: "bytes", key: "bytes" };
    pub const TARGET_BLOCK: Column = Column { header: "target block", key: "target_block" };
    pub const FIELD_BLOCK: Column = Column { header: "field block", key: "field_block" };
    pub const FIELD_OFFSET: Column = Column { header: "field offset", key: "field_offset" };
    pub const PARENT_STRUCT: Column = Column { header: "parent struct", key: "parent_struct" };
    pub const DEPENDENCY: Column = Column { header: "dependency", key: "dependency" };
    pub const GUID: Column = Column { header: "GUID", key: "guid" };
    pub const HASH: Column = Column { header: "hash", key: "hash" };
    pub const STRING: Column = Column { header: "string", key: "string" };
    pub const ZONESET: Column = Column { header: "zoneset", key: "zoneset" };
    pub const NAME_HASH: Column = Column { header: "name hash", key: "name_hash" };
    pub const RESOURCE: Column = Column { header: "resource", key: "resource" };
    pub const RESOURCES: Column = Column { header: "resources", key: "resources" };
    pub const LOADS_ZONESET: Column = Column { header: "loads zoneset", key: "loads_zoneset" };
    // Fields of unknown meaning, named by their offset in the list entry.
    pub const UNKNOWN0: Column = Column { header: "unknown0", key: "unknown0" };
    pub const UNKNOWN4: Column = Column { header: "unknown4", key: "unknown4" };
    pub const UNKNOWN12: Column = Column { header: "unknown12", key: "unknown12" };
    pub const UNKNOWN14: Column = Column { header: "unknown14", key: "unknown14" };
}

fn part(title: &'static str, key: &'static str, columns: Vec<Column>, rows: Vec<Vec<Cell>>) -> Part {
    Part { title, key, table: Table { total: rows.len(), columns, rows, ..Table::default() } }
}

/// `part`, with the name column `wide` shown last in the `table` layout.
fn named_part(title: &'static str, key: &'static str, columns: Vec<Column>, rows: Vec<Vec<Cell>>) -> Part {
    let mut p = part(title, key, columns, rows);
    p.table.wide_column = p.table.columns.iter().position(|c| c.key == col::DISPLAY_NAME.key);
    p
}

fn int(n: impl Into<i64>) -> Cell {
    Cell::Int(n.into())
}

fn hex(n: impl std::fmt::UpperHex) -> Cell {
    format!("0x{n:X}").into()
}

/// A 32-bit hash, all 8 digits.
fn hash(n: u32) -> Cell {
    format!("0x{n:08X}").into()
}

/// A list index; none (-1) is an empty cell.
fn index(i: Option<u32>) -> Cell {
    i.map_or(Cell::Empty, int)
}

/// The display name of a referenced tag: from the index when it is indexed,
/// else the name listed in this tag + `.` + its class name (or code).
fn target_name(b: &Build, id: GlobalId, class: FourCc, listed: Option<&str>) -> Cell {
    match (b.tag_by_id(id), listed) {
        (Some(tag), _) => b.display_name(tag).into(),
        (None, Some(name)) if !name.is_empty() => format!("{name}.{}", b.class_name(class)).into(),
        _ => Cell::Empty,
    }
}

fn header_part(h: &TagHeader, module: &str) -> Part {
    let f = &h.fields;
    let alignments = format!(
        "header {}, tag data {}, resource data {}",
        f.header_alignment, f.tag_data_alignment, f.resource_data_alignment
    );
    let rows = [
        ("module", module.into()),
        ("version", int(f.version)),
        ("asset checksum", format!("0x{:016X}", f.asset_checksum).into()),
        ("header size", int(f.header_size)),
        ("tag data size", int(f.data_size)),
        ("resource data size", int(f.resource_data_size)),
        ("string table size", int(f.string_table_size)),
        ("zoneset info size", int(f.zoneset_size)),
        ("trailing data size", int(h.trailing.len() as i64)),
        ("alignments", alignments.into()),
        ("unknown8", hex(f.unknown8)),
        ("unknown18", hex(f.unknown18)),
        ("unknown4b", hex(f.unknown4b)),
        ("unknown4c", hex(f.unknown4c)),
    ]
    .into_iter()
    .map(|(name, value)| vec![name.into(), value])
    .collect();
    part("header", "header", vec![column::FIELD, column::VALUE], rows)
}

fn dependencies(b: &Build, h: &TagHeader) -> Part {
    let rows = h
        .dependencies
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let listed = h.string(d.name_offset);
            vec![
                int(i as i64),
                d.class_code.to_string().into(),
                d.global_id.to_string().into(),
                target_name(b, d.global_id, d.class_code, listed.as_deref()),
                Cell::Bool(b.tag_by_id(d.global_id).is_some()),
                listed.map_or(Cell::Empty, Cell::from),
                hex(d.asset_id),
                int(d.unknown14),
            ]
        })
        .collect();
    let columns = vec![
        column::INDEX,
        col::CLASS,
        col::GLOBAL_ID,
        col::DISPLAY_NAME,
        column::INDEXED,
        column::LISTED_NAME,
        column::ASSET_ID,
        column::UNKNOWN14,
    ];
    named_part("dependencies", "dependencies", columns, rows)
}

fn section_label(block: &DataBlock) -> Cell {
    match block.section() {
        Some(Section::Header) => "header".into(),
        Some(Section::TagData) => "tag data".into(),
        Some(Section::ResourceData) => "resource data".into(),
        None => int(block.section),
    }
}

fn data_blocks(h: &TagHeader) -> Part {
    let rows = h
        .data_blocks
        .iter()
        .enumerate()
        .map(|(i, d)| vec![int(i as i64), section_label(d), hex(d.offset), int(d.size), int(d.unknown4)])
        .collect();
    let columns = vec![column::INDEX, column::SECTION, column::OFFSET, column::BYTES, column::UNKNOWN4];
    part("data blocks", "data_blocks", columns, rows)
}

fn struct_kind(kind: StructKind) -> Cell {
    match kind {
        StructKind::Main => "main".into(),
        StructKind::TagBlock => "tag block".into(),
        StructKind::Resource => "resource".into(),
        StructKind::Custom => "custom".into(),
        StructKind::Unknown(n) => format!("unknown ({n})").into(),
    }
}

fn tag_structs(h: &TagHeader) -> Part {
    let rows = h
        .tag_structs
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let guid: String = s.guid.iter().map(|b| format!("{b:02X}")).collect();
            vec![
                int(i as i64),
                struct_kind(s.kind),
                index(s.target),
                index(s.field_block),
                hex(s.field_offset),
                guid.into(),
                int(s.unknown12),
            ]
        })
        .collect();
    let columns = vec![
        column::INDEX,
        col::KIND,
        column::TARGET_BLOCK,
        column::FIELD_BLOCK,
        column::FIELD_OFFSET,
        column::GUID,
        column::UNKNOWN12,
    ];
    part("tag structs", "tag_structs", columns, rows)
}

fn data_references(h: &TagHeader) -> Part {
    let rows = h
        .data_references
        .iter()
        .enumerate()
        .map(|(i, d)| {
            vec![
                int(i as i64),
                index(d.parent_struct),
                index(d.target),
                index(d.field_block),
                hex(d.field_offset),
                int(d.unknown4),
            ]
        })
        .collect();
    let columns = vec![
        column::INDEX,
        column::PARENT_STRUCT,
        column::TARGET_BLOCK,
        column::FIELD_BLOCK,
        column::FIELD_OFFSET,
        column::UNKNOWN4,
    ];
    part("data references", "data_references", columns, rows)
}

/// Each reference with what its field holds: a reference without a
/// dependency may still name a tag (format-notes "Tags").
/// (data block, offset) → field path.
type Places = HashMap<(u32, u32), String>;

/// Where each tag reference is, by field path (decisions D29): (data block,
/// offset) → path, from reading the tag with its class's definition. `None`
/// without a definition.
fn reference_paths(b: &Build, tag: &Tag, t: &StoredTag) -> Result<Option<Places>, ToolError> {
    fn collect(body: &h5_meta::Body<'_>, path: &str, out: &mut Places) {
        for n in &body.children {
            let p = join(path, n.name.unwrap_or_default());
            match &n.content {
                h5_meta::Content::Value(h5_meta::Value::TagRef(r)) => {
                    out.insert(r.place, p);
                }
                h5_meta::Content::Struct(b) | h5_meta::Content::Array(b) => collect(b, &p, out),
                h5_meta::Content::Block(b) => {
                    for (i, e) in b.entries.iter().enumerate() {
                        collect(e, &format!("{p}[{i}]"), out);
                    }
                }
                _ => {}
            }
        }
    }
    let Some(def) = load_definition(b, tag).map_err(present::definition_error)? else { return Ok(None) };
    let read = h5_meta::read_tag(&def, &t.header, &t.tag_data);
    let mut out = HashMap::new();
    collect(&read.root, "", &mut out);
    Ok(Some(out))
}

fn tag_references(b: &Build, t: &StoredTag, paths: Option<&Places>) -> Part {
    let h = &t.header;
    let rows = h
        .tag_references
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let field = h.reference_field(&t.tag_data, r);
            let target = field.filter(|f| !f.is_null());
            let listed = h.string(r.name_offset);
            let path = paths.map(|p| {
                r.field_block.and_then(|b| p.get(&(b, r.field_offset))).map_or(Cell::Empty, |s| s.clone().into())
            });
            let mut row = vec![int(i as i64), index(r.field_block), hex(r.field_offset)];
            row.extend(path);
            row.extend([
                index(r.dependency),
                field.map_or(Cell::Empty, |f| f.global_id.to_string().into()),
                field.map_or(Cell::Empty, |f| f.class_code.to_string().into()),
                target.map_or(Cell::Empty, |f| target_name(b, f.global_id, f.class_code, listed.as_deref())),
            ]);
            row
        })
        .collect();
    let mut columns = vec![column::INDEX, column::FIELD_BLOCK, column::FIELD_OFFSET];
    if paths.is_some() {
        columns.push(col::FIELD_PATH);
    }
    columns.extend([column::DEPENDENCY, col::GLOBAL_ID, col::CLASS, col::DISPLAY_NAME]);
    named_part("tag references", "tag_references", columns, rows)
}

fn string_ids(h: &TagHeader) -> Part {
    let rows = h
        .string_ids
        .iter()
        .enumerate()
        .map(|(i, s)| {
            vec![
                int(i as i64),
                // The value a string_id field holds for this string (PRD §5.2).
                h.string_bytes(s.string_offset).map_or(Cell::Empty, |b| hash(murmur3_x86_32(b, 0))),
                h.string(s.string_offset).map_or(Cell::Empty, Cell::from),
                int(s.unknown0),
            ]
        })
        .collect();
    let columns = vec![column::INDEX, column::HASH, column::STRING, column::UNKNOWN0];
    part("string IDs", "string_ids", columns, rows)
}

/// A string_id: its string when the tag lists it, else the hash.
fn string_id(names: &StringIdNames<'_>, value: u32) -> Cell {
    names.get(value).map_or_else(|| hash(value), Cell::from)
}

fn zonesets(h: &TagHeader, names: &StringIdNames<'_>) -> Part {
    let rows = h
        .zonesets
        .iter()
        .enumerate()
        .map(|(i, z)| {
            vec![
                int(i as i64),
                string_id(names, z.name),
                hash(z.name),
                int(z.resources.len() as i64),
                int(z.tags.len() as i64),
                int(z.unknown4),
            ]
        })
        .collect();
    let columns = vec![column::INDEX, col::NAME, column::NAME_HASH, column::RESOURCES, col::TAGS, column::UNKNOWN4];
    part("zonesets", "zonesets", columns, rows)
}

/// The tag's resources each zoneset loads: index into the tag's resource
/// list, and that resource entry (format-notes "Tags").
fn zoneset_resources(b: &Build, copy: Copy, h: &TagHeader, names: &StringIdNames<'_>) -> Part {
    let resources = &b.entry(copy).resources;
    let rows = h
        .zonesets
        .iter()
        .flat_map(|z| z.resources.iter().map(move |&r| (z, r)))
        .map(|(z, r)| {
            let entry = resources.get(r as usize).map(|&file| &b.module(copy).entries[file as usize]);
            vec![
                string_id(names, z.name),
                int(r),
                entry.and_then(|e| e.resource_name()).map_or(Cell::Empty, |n| n.kind.into()),
                entry.map_or(Cell::Empty, |e| e.name.clone().into()),
            ]
        })
        .collect();
    let mut p = part(
        "zoneset resources",
        "zoneset_resources",
        vec![column::ZONESET, column::RESOURCE, col::KIND, col::NAME],
        rows,
    );
    p.table.wide_column = Some(3);
    p
}

/// Zoneset tags carry only a global ID; tags that aren't indexed are named
/// from the tag's own dependency list.
fn zoneset_tags(b: &Build, h: &TagHeader, names: &StringIdNames<'_>) -> Part {
    let deps: HashMap<GlobalId, &Dependency> = h.dependencies.iter().map(|d| (d.global_id, d)).collect();
    let rows = h
        .zonesets
        .iter()
        .flat_map(|z| z.tags.iter().map(move |t| (z, t)))
        .map(|(z, t)| {
            let dep = deps.get(&t.global_id);
            let listed = dep.and_then(|d| h.string(d.name_offset));
            let class = dep.map_or(FourCc::NONE, |d| d.class_code);
            vec![
                string_id(names, z.name),
                t.global_id.to_string().into(),
                target_name(b, t.global_id, class, listed.as_deref()),
                string_id(names, t.zoneset_name),
            ]
        })
        .collect();
    let columns = vec![column::ZONESET, col::GLOBAL_ID, col::DISPLAY_NAME, column::LOADS_ZONESET];
    named_part("zoneset tags", "zoneset_tags", columns, rows)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::output::Format;
    use crate::testdeploy::{BULLET, TestDeploy, WEAPON};

    fn dump(d: &mut TestDeploy, module: Option<&str>) -> Result<Output, ToolError> {
        let cmd = HeaderDump { tag: WEAPON.into(), module: module.map(Into::into), build: BuildArg::default() };
        cmd.run(&mut d.session)
    }

    fn json(out: &Output) -> Value {
        serde_json::from_str(&out.render(Format::Json)).unwrap()
    }

    #[test]
    fn dumps_every_section() {
        let mut d = TestDeploy::new("dump-sections");
        let v = json(&dump(&mut d, None).unwrap());
        let totals: Vec<(&str, u64)> = [
            "header",
            "dependencies",
            "data_blocks",
            "tag_structs",
            "data_references",
            "tag_references",
            "string_ids",
            "zonesets",
            "zoneset_resources",
            "zoneset_tags",
        ]
        .iter()
        .map(|k| (*k, v[k]["total"].as_u64().unwrap_or_else(|| panic!("no part {k}"))))
        .collect();
        let lists: Vec<u64> = totals[1..].iter().map(|t| t.1).collect();
        assert_eq!(lists, [2, 3, 2, 1, 3, 2, 1, 1, 2]);
    }

    /// Zoneset names are string_ids of the tag; each zoneset lists the
    /// tag's resources it loads, by index.
    #[test]
    fn zonesets_show_their_names_and_resources() {
        let mut d = TestDeploy::new("dump-zonesets");
        let v = json(&dump(&mut d, None).unwrap());
        assert_eq!(v["zonesets"]["rows"][0]["name"], "__default__");
        assert_eq!(v["zonesets"]["rows"][0]["name_hash"], "0x9B555AD2");
        assert_eq!(v["zonesets"]["rows"][0]["resources"], 1);
        let res = &v["zoneset_resources"]["rows"][0];
        assert_eq!(
            (&res["zoneset"], &res["resource"], &res["kind"]),
            (&"__default__".into(), &0.into(), &"mesh resource".into())
        );
        assert_eq!(res["name"], format!("{WEAPON}[0:mesh resource]"));
        assert_eq!(v["zoneset_tags"]["rows"][1]["zoneset"], "__default__");
        // Not indexed: named from the tag's own dependency list.
        assert_eq!(v["zoneset_tags"]["rows"][1]["name"], r"fx\impact\bullet_hit.effe");
    }

    /// M2 done-when: dependencies print as readable display names.
    #[test]
    fn dependencies_have_display_names_and_global_ids() {
        let mut d = TestDeploy::new("dump-deps");
        let v = json(&dump(&mut d, None).unwrap());
        let deps = &v["dependencies"]["rows"];
        assert_eq!(deps[0]["name"], BULLET);
        assert_eq!(deps[0]["id"], "0x00401000");
        assert_eq!(deps[0]["indexed"], true);
        // Not in the indexed modules: the class code stands in for the unknown class name.
        assert_eq!(deps[1]["name"], r"fx\impact\bullet_hit.effe");
        assert_eq!(deps[1]["indexed"], false);
    }

    #[test]
    fn tag_references_show_what_their_field_holds() {
        let mut d = TestDeploy::new("dump-refs");
        let v = json(&dump(&mut d, None).unwrap());
        let refs = &v["tag_references"]["rows"];
        assert_eq!(refs[0]["dependency"], 0);
        assert_eq!(refs[0]["id"], "0x00401000");
        // A null reference: no dependency, and its field holds global ID -1.
        assert!(refs[1].get("dependency").is_none());
        assert_eq!(refs[1]["id"], "0xFFFFFFFF");
    }

    /// With a definition, each reference also gets its field path (decisions D29).
    #[test]
    fn tag_references_have_field_paths() {
        let mut d = TestDeploy::new("dump-paths");
        let v = json(&dump(&mut d, None).unwrap());
        let paths: Vec<&str> =
            v["tag_references"]["rows"].as_array().unwrap().iter().map(|r| r["path"].as_str().unwrap_or("-")).collect();
        assert_eq!(paths, ["projectile", "entries[0].target", "entries[1].target"]);
    }

    #[test]
    fn text_has_one_titled_table_per_section() {
        let mut d = TestDeploy::new("dump-text");
        let text = dump(&mut d, None).unwrap().render(Format::Text);
        assert!(text.starts_with("## header\n"), "{text}");
        assert!(text.contains("\n## tag structs\n"), "{text}");
    }

    #[test]
    fn a_module_without_the_tag_or_a_changed_file_is_an_error() {
        let mut d = TestDeploy::new("dump-errors");
        let err = dump(&mut d, Some("nowhere")).unwrap_err();
        assert!(err.message.contains("no copy in a module matching 'nowhere'"), "{err:?}");
        std::fs::write(d.module_file(), b"changed").unwrap();
        let err = dump(&mut d, None).unwrap_err();
        assert!(err.message.contains("changed since it was indexed"), "{err:?}");
        assert!(err.hint.unwrap().contains("reindex"));
    }
}
