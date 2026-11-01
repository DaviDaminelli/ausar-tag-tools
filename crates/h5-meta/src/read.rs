//! Reading a tag's data with its definition (PRD §6.4). Structs and arrays are
//! inline; a tag block's entries live in the data block the header's tag
//! struct list gives for the block field's (data block, offset). Sizes are
//! always checked; a struct that doesn't fit isn't read, the rest still is.

use std::collections::{HashMap, HashSet};
use std::fmt;

use h5_defs::definition::{Definition, Field, FieldKind};
use h5_defs::field_type::FieldType;
use h5_module::FourCc;
use h5_module::tag::{StringIdNames, StructKind, TagHeader, TagReference};
use h5_module::text::decode_cp1252;

use crate::tree::{
    Block, Body, Content, DataRef, Divergence, Node, Problem, ReadTag, ResourceRef, StringIdValue, TagRef, Value,
};

/// Reads the whole tag: the main struct and everything below it.
pub fn read_tag<'d>(def: &'d Definition, header: &TagHeader, tag_data: &[u8]) -> ReadTag<'d> {
    let mut w = Walker::new(header, tag_data);
    let root = w.main(def);
    ReadTag { root, divergences: w.divergences() }
}

/// (data block, offset) → index in one of the header's lists.
type ByPlace = HashMap<(u32, u32), usize>;

fn by_place<'a>(places: impl Iterator<Item = (Option<u32>, u32)> + 'a) -> ByPlace {
    places.enumerate().filter_map(|(i, (block, offset))| Some(((block?, offset), i))).collect()
}

struct Walker<'h> {
    header: &'h TagHeader,
    tag_data: &'h [u8],
    names: StringIdNames<'h>,
    /// For each tag struct of kind resource: its position among them (Q9).
    resource_positions: Vec<Option<u32>>,
    tag_refs: ByPlace,
    data_refs: ByPlace,
    /// Tag structs other than the main one.
    structs: ByPlace,
    seen_tag_refs: Vec<bool>,
    seen_data_refs: Vec<bool>,
    seen_structs: Vec<bool>,
    /// Data blocks whose contents were read: list entries in them must be reached.
    read_blocks: HashSet<u32>,
    not_in_list: Vec<Divergence>,
}

/// Where a struct starts: its data block, that block's bytes, and the offset in them.
#[derive(Clone, Copy)]
struct Place<'b> {
    block: u32,
    bytes: &'b [u8],
    at: u32,
}

impl<'b> Place<'b> {
    fn plus(self, offset: u32) -> Place<'b> {
        Place { at: self.at + offset, ..self }
    }
}

/// The child that labels a tag block entry (PRD §6.4.4): the field marked
/// `^`, else the first string, string_id or tag reference.
fn label_of(entry: &Body<'_>) -> Option<usize> {
    let c = &entry.children;
    c.iter().position(|n| n.field.name.labels_entries()).or_else(|| {
        c.iter()
            .position(|n| matches!(n.content, Content::Value(Value::String(_) | Value::StringId(_) | Value::TagRef(_))))
    })
}

/// A field path, formatted only when a problem or divergence needs it (PRD
/// §6.4.2 syntax: `barrels[0].firing noise`).
#[derive(Clone, Copy)]
struct Path<'p> {
    parent: Option<&'p Path<'p>>,
    step: &'p str,
    index: Option<u32>,
}

const ROOT: Path<'static> = Path { parent: None, step: "", index: None };

impl<'p> Path<'p> {
    fn child(&'p self, step: &'p str) -> Path<'p> {
        Path { parent: Some(self), step, index: None }
    }

    /// Entry `i` of the tag block at this path.
    fn entry(&self, i: u32) -> Path<'p> {
        Path { index: Some(i), ..*self }
    }

    fn is_root(&self) -> bool {
        self.parent.is_none() && self.step.is_empty()
    }
}

impl fmt::Display for Path<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(p) = self.parent.filter(|p| !p.is_root()) {
            write!(f, "{p}.")?;
        }
        f.write_str(self.step)?;
        match self.index {
            Some(i) => write!(f, "[{i}]"),
            None => Ok(()),
        }
    }
}

fn le<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    b[at..at + N].try_into().expect("the field's size was checked")
}

impl<'h> Walker<'h> {
    fn new(header: &'h TagHeader, tag_data: &'h [u8]) -> Walker<'h> {
        let h = header;
        let mut next = 0u32;
        let resource_positions = h
            .tag_structs
            .iter()
            .map(|s| {
                (s.kind == StructKind::Resource).then(|| {
                    next += 1;
                    next - 1
                })
            })
            .collect();
        Walker {
            header,
            tag_data,
            names: header.string_id_names(),
            resource_positions,
            tag_refs: by_place(h.tag_references.iter().map(|r| (r.field_block, r.field_offset))),
            data_refs: by_place(h.data_references.iter().map(|r| (r.field_block, r.field_offset))),
            structs: by_place(
                h.tag_structs.iter().map(|s| (s.field_block.filter(|_| s.kind != StructKind::Main), s.field_offset)),
            ),
            seen_tag_refs: vec![false; h.tag_references.len()],
            seen_data_refs: vec![false; h.data_references.len()],
            seen_structs: vec![false; h.tag_structs.len()],
            read_blocks: HashSet::new(),
            not_in_list: Vec::new(),
        }
    }

    fn main<'d>(&mut self, def: &'d Definition) -> Body<'d> {
        let target = self.header.main_struct().and_then(|s| s.target);
        let Some((t, bytes)) = target.and_then(|t| Some((t, self.header.block_data(self.tag_data, t)?))) else {
            return Body { problem: Some(Problem::NoDataBlock), ..Body::default() };
        };
        if let Some(size) = def.size()
            && size as usize != bytes.len()
        {
            let problem = Problem::SizeMismatch { definition: size, tag: bytes.len() as u32 };
            return Body { problem: Some(problem), ..Body::default() };
        }
        self.read_blocks.insert(t);
        self.body(&def.fields, Place { block: t, bytes, at: 0 }, &ROOT)
    }

    /// The fields `fields` of one struct, starting at `start`.
    fn body<'d>(&mut self, fields: &'d [Field], start: Place<'_>, path: &Path<'_>) -> Body<'d> {
        let mut body = Body::default();
        let mut offset = 0u32;
        for f in fields {
            let name = f.step.as_deref();
            let field_path = path.child(name.unwrap_or(&f.name.display));
            let FieldKind::Known(t) = &f.kind else {
                let FieldKind::Unknown(type_name) = &f.kind else { unreachable!() };
                body.problem = Some(Problem::UnknownType { at: field_path.to_string(), type_name: type_name.clone() });
                break;
            };
            let here = start.plus(offset);
            let Some(size) = f.size() else {
                // A struct or array holding an unknown type: show what can be
                // read, then stop here too (later offsets are unknown).
                if matches!(t, FieldType::Struct | FieldType::Array) {
                    let inner = self.body(&f.children, here, &field_path);
                    body.problem = inner.problem.clone();
                    body.children.push(Node { field: f, name, offset, content: Content::Struct(inner) });
                } else {
                    body.problem = Some(Problem::UnknownSize { at: field_path.to_string() });
                }
                break;
            };
            let Some(field) = here.bytes.get(here.at as usize..(here.at + size) as usize) else {
                body.problem = Some(Problem::OutOfBounds { at: field_path.to_string() });
                break;
            };
            let content = self.content(f, *t, here, field, &field_path);
            body.children.push(Node { field: f, name, offset, content });
            offset += size;
        }
        body
    }

    /// The node for field `f` of type `t` at `here`; `b` is the field's bytes.
    fn content<'d>(&mut self, f: &'d Field, t: FieldType, here: Place<'_>, b: &[u8], path: &Path<'_>) -> Content<'d> {
        let (block, at) = (here.block, here.at);
        use FieldType as T;
        let int = |v: i64| Content::Value(Value::Int(v));
        let reals = |n: usize| Content::Value(Value::Reals((0..n).map(|i| f32::from_le_bytes(le(b, i * 4))).collect()));
        let i16s =
            |n: usize| Content::Value(Value::Ints((0..n).map(|i| i16::from_le_bytes(le(b, i * 2)) as i64).collect()));
        match t {
            T::Explanation | T::Custom => Content::Heading,
            T::Pad | T::Skip => Content::Value(Value::Padding),
            T::Struct | T::Array => {
                let mut inner = self.body(&f.children, here, path);
                inner.guid_mismatch = self.mark_struct(block, at, f);
                if t == T::Struct { Content::Struct(inner) } else { Content::Array(inner) }
            }
            T::Block64 => Content::Block(self.block(f, block, at, b, path)),
            T::TagReference64 => {
                let place = (block, at);
                let listed = self.tag_refs.get(&place).copied();
                if let Some(i) = listed {
                    self.seen_tag_refs[i] = true;
                } else {
                    self.not_in_list.push(Divergence::NotInList { what: "tag reference", path: path.to_string() });
                }
                let entry = listed.map(|i| &self.header.tag_references[i]);
                let probe =
                    TagReference { field_block: Some(block), field_offset: at, name_offset: 0, dependency: None };
                let field = self.header.reference_field(self.tag_data, entry.unwrap_or(&probe)).expect("size checked");
                let listed_name = entry.and_then(|r| self.header.string(r.name_offset)).filter(|s| !s.is_empty());
                Content::Value(Value::TagRef(TagRef {
                    field,
                    listed_name: listed_name.map(|s| s.into_owned()),
                    dependency: entry.and_then(|r| r.dependency),
                }))
            }
            T::Data64 => {
                match self.data_refs.get(&(block, at)) {
                    Some(&i) => self.seen_data_refs[i] = true,
                    None => {
                        self.not_in_list.push(Divergence::NotInList { what: "data reference", path: path.to_string() })
                    }
                }
                Content::Value(Value::Data(DataRef { size: u32::from_le_bytes(le(b, 0x18)) }))
            }
            T::PageableResource64 => {
                self.mark_struct(block, at, f);
                let index = self.structs.get(&(block, at)).and_then(|&i| self.resource_positions[i]);
                Content::Value(Value::Resource(ResourceRef { index }))
            }
            T::CharInteger => int(i8::from_le_bytes(le(b, 0)) as i64),
            T::ByteInteger => int(b[0] as i64),
            T::ShortInteger => int(i16::from_le_bytes(le(b, 0)) as i64),
            T::WordInteger => int(u16::from_le_bytes(le(b, 0)) as i64),
            T::LongInteger => int(i32::from_le_bytes(le(b, 0)) as i64),
            T::DwordInteger => int(u32::from_le_bytes(le(b, 0)) as i64),
            T::Int64Integer => int(i64::from_le_bytes(le(b, 0))),
            T::QwordInteger => Content::Value(Value::Uint(u64::from_le_bytes(le(b, 0)))),
            T::ShortIntegerBounds | T::Point2d => i16s(2),
            T::Rectangle2d => i16s(4),
            T::CharEnum => Content::Value(Value::Enum(i8::from_le_bytes(le(b, 0)) as i64)),
            T::ShortEnum => Content::Value(Value::Enum(i16::from_le_bytes(le(b, 0)) as i64)),
            T::LongEnum => Content::Value(Value::Enum(i32::from_le_bytes(le(b, 0)) as i64)),
            T::ByteFlags | T::ByteBlockFlags => Content::Value(Value::Flags(b[0] as u64)),
            T::WordFlags | T::WordBlockFlags => Content::Value(Value::Flags(u16::from_le_bytes(le(b, 0)) as u64)),
            T::LongFlags | T::LongBlockFlags => Content::Value(Value::Flags(u32::from_le_bytes(le(b, 0)) as u64)),
            T::CharBlockIndex => Content::Value(Value::BlockIndex(i8::from_le_bytes(le(b, 0)) as i64)),
            T::ShortBlockIndex | T::CustomShortBlockIndex => {
                Content::Value(Value::BlockIndex(i16::from_le_bytes(le(b, 0)) as i64))
            }
            T::LongBlockIndex | T::CustomLongBlockIndex => {
                Content::Value(Value::BlockIndex(i32::from_le_bytes(le(b, 0)) as i64))
            }
            T::Real | T::RealFraction | T::Angle => Content::Value(Value::Real(f32::from_le_bytes(le(b, 0)))),
            T::RealBounds | T::AngleBounds | T::FractionBounds | T::RealPoint2d | T::RealVector2d => reals(2),
            T::RealEulerAngles2d => reals(2),
            T::RealPoint3d | T::RealVector3d | T::RealEulerAngles3d | T::RealPlane2d => reals(3),
            T::RealRgbColor | T::RealHsvColor => reals(3),
            T::RealQuaternion | T::RealPlane3d | T::RealArgbColor | T::RealAhsvColor => reals(4),
            T::RgbColor | T::ArgbColor => Content::Value(Value::Color(u32::from_le_bytes(le(b, 0)))),
            T::String | T::LongString => {
                let text = b.split(|&c| c == 0).next().unwrap_or_default();
                Content::Value(Value::String(decode_cp1252(text).into_owned()))
            }
            T::StringId | T::OldStringId => {
                let hash = u32::from_le_bytes(le(b, 0));
                let text = (hash != 0).then(|| self.names.get(hash)).flatten().map(|s| s.into_owned());
                Content::Value(Value::StringId(StringIdValue { hash, text }))
            }
            T::Tag => Content::Value(Value::Tag(FourCc(u32::from_le_bytes(le(b, 0))))),
        }
    }

    /// A tag block field at (`block`, `at`): its entries, if they fit.
    fn block<'d>(&mut self, f: &'d Field, block: u32, at: u32, b: &[u8], path: &Path<'_>) -> Block<'d> {
        let count = i32::from_le_bytes(le(b, 0x10)).max(0) as u32;
        let guid_mismatch = self.mark_struct(block, at, f);
        let mut out = Block { count, entries: Vec::new(), problem: None, guid_mismatch };
        if count == 0 {
            return out;
        }
        let target = self.structs.get(&(block, at)).and_then(|&i| self.header.tag_structs[i].target);
        let Some((t, bytes)) = target.and_then(|t| Some((t, self.header.block_data(self.tag_data, t)?))) else {
            self.not_in_list.push(Divergence::NotInList { what: "tag block", path: path.to_string() });
            out.problem = Some(Problem::NoDataBlock);
            return out;
        };
        let Some(entry_size) = f.entry_size() else {
            // An unknown type inside: read the first entry up to it.
            let first = self.body(&f.children, Place { block: t, bytes, at: 0 }, &path.entry(0));
            out.problem = first.problem.clone();
            out.entries.push(first);
            return out;
        };
        let want = count as u64 * entry_size as u64;
        if want != bytes.len() as u64 {
            out.problem = Some(Problem::SizeMismatch { definition: want as u32, tag: bytes.len() as u32 });
            return out;
        }
        self.read_blocks.insert(t);
        out.entries = (0..count)
            .map(|i| {
                let mut entry = self.body(&f.children, Place { block: t, bytes, at: i * entry_size }, &path.entry(i));
                entry.label = label_of(&entry);
                entry
            })
            .collect();
        out
    }

    /// Marks the tag struct entry at (`block`, `at`) as reached; the tag's GUID
    /// if it differs from the definition's `srcguid` (PRD §6.4.5 GUID check).
    fn mark_struct(&mut self, block: u32, at: u32, f: &Field) -> Option<[u8; 16]> {
        let &i = self.structs.get(&(block, at))?;
        self.seen_structs[i] = true;
        let tag = self.header.tag_structs[i].guid;
        f.guid().filter(|g| *g != tag).map(|_| tag)
    }

    /// Check 1: everything the definition placed but the header doesn't list,
    /// then every list entry in a read data block that nothing reached.
    fn divergences(self) -> Vec<Divergence> {
        let h = self.header;
        let mut out = self.not_in_list;
        let mut unreached = |what: &'static str, index: usize, block: Option<u32>, offset: u32| {
            if let Some(b) = block.filter(|b| self.read_blocks.contains(b)) {
                out.push(Divergence::Unreached { what, index: index as u32, block: b, offset });
            }
        };
        for (i, s) in
            h.tag_structs.iter().enumerate().filter(|(i, s)| s.kind != StructKind::Main && !self.seen_structs[*i])
        {
            let what = match s.kind {
                StructKind::TagBlock => "tag block",
                StructKind::Resource => "resource",
                _ => "struct",
            };
            unreached(what, i, s.field_block, s.field_offset);
        }
        for (i, r) in h.data_references.iter().enumerate().filter(|(i, _)| !self.seen_data_refs[*i]) {
            unreached("data reference", i, r.field_block, r.field_offset);
        }
        for (i, r) in h.tag_references.iter().enumerate().filter(|(i, _)| !self.seen_tag_refs[*i]) {
            unreached("tag reference", i, r.field_block, r.field_offset);
        }
        out
    }
}
