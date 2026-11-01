//! The node tree of one tag read with its definition (PRD §7.7: one tree,
//! rendered as text or JSON elsewhere). Domain only: typed values and typed
//! problems, no text formatting.

use h5_defs::definition::Field;
use h5_module::FourCc;
use h5_module::tag::ReferenceField;

/// One field of a struct, array or tag block entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Node<'d> {
    /// The definition field this node reads.
    pub field: &'d Field,
    /// The path step: the display name, made unique among siblings
    /// (`name#2`). `None` for explanations and customs.
    pub name: Option<&'d str>,
    /// Bytes from the start of its struct (PRD §7.6, `detail=full`).
    pub offset: u32,
    pub content: Content<'d>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Content<'d> {
    /// An explanation or custom: no data (its role is in the definition).
    Heading,
    Value(Value),
    Struct(Body<'d>),
    Array(Body<'d>),
    Block(Block<'d>),
}

/// The fields of a struct, an array or one tag block entry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Body<'d> {
    pub children: Vec<Node<'d>>,
    /// Why the fields stop early, or weren't read at all (`children` empty).
    pub problem: Option<Problem>,
    /// The tag's struct GUID, when it differs from the definition's srcguid
    /// (`guid_check = warn`; also for the definition gap list, decisions D33).
    pub guid_mismatch: Option<[u8; 16]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block<'d> {
    /// Entry count stored in the tag block field.
    pub count: u32,
    /// Empty when the block isn't read (`problem` says why) or has no entries.
    pub entries: Vec<Body<'d>>,
    pub problem: Option<Problem>,
    pub guid_mismatch: Option<[u8; 16]>,
}

/// Why something wasn't read (PRD §8, category 3: the tool's problem).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// PRD §6.4.5: the definition's size doesn't match the tag's data.
    SizeMismatch { definition: u32, tag: u32 },
    /// PRD §6.4.4: reading stopped at a field type the table doesn't know.
    UnknownType { at: String, type_name: String },
    /// A field whose size can't be computed (padding without a length).
    UnknownSize { at: String },
    /// The tag block has entries, but the header lists no data block for it.
    NoDataBlock,
    /// The data the definition describes lies outside its data block.
    OutOfBounds { at: String },
}

/// A field's value as stored. Labels (enum options, flag names, block entry
/// labels) come from the definition when the value is presented.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    /// Unsigned 64-bit integers (`_field_qword_integer`).
    Uint(u64),
    Real(f32),
    /// Multi-component integers (points, rectangles, short bounds).
    Ints(Vec<i64>),
    /// Multi-component reals (bounds, vectors, angles, quaternions, planes, colours).
    Reals(Vec<f32>),
    /// The option's position (PRD §5.3).
    Enum(i64),
    /// Set bits (bit numbers are option positions).
    Flags(u64),
    /// An index into a tag block; -1 = none.
    BlockIndex(i64),
    /// An 8-bit-per-channel colour as stored (`_field_rgb_color`, `_field_argb_color`).
    Color(u32),
    String(String),
    StringId(u32),
    Tag(FourCc),
    TagRef(TagRef),
    Data(DataRef),
    Resource,
    /// `_field_pad`, `_field_skip`: bytes with no meaning.
    Padding,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TagRef {
    /// What the field holds (null when its global ID is -1).
    pub field: ReferenceField,
    /// The name listed for it in the tag's string table (no class suffix).
    pub listed_name: Option<String>,
    /// Its dependency index, if the header gives one (format-notes "Tags").
    pub dependency: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataRef {
    /// Bytes of data (0 = empty).
    pub size: u32,
}

/// PRD §9 check 1: where the definition and the tag header's lists disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Divergence {
    /// The definition places a tag reference, tag block or data reference
    /// here; the header lists none at this (data block, offset).
    NotInList { what: &'static str, path: String },
    /// A header list entry in a data block that was read, reached by no
    /// field of the definition.
    Unreached { what: &'static str, index: u32, block: u32, offset: u32 },
}

/// A tag read with its definition.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadTag<'d> {
    /// The main struct's fields.
    pub root: Body<'d>,
    pub divergences: Vec<Divergence>,
}
