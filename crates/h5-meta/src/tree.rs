//! The node tree of one tag read with its definition (PRD §7.7: one tree,
//! rendered as text or JSON elsewhere). Domain only: typed values and typed
//! problems, no text formatting.

use h5_defs::definition::Field;
use h5_defs::name::Name;
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
    /// For a tag block entry: the child that labels it (PRD §6.4.4): the field
    /// marked `^`, else the first string, string_id or tag reference.
    pub label: Option<usize>,
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
    /// PRD §6.4.5: the definition's size doesn't match the tag's data. For
    /// the main struct, the fields up to the last anchor the tag header
    /// confirms are read (decisions D70): `read_up_to`.
    SizeMismatch { definition: u32, tag: u32, read_up_to: Option<ReadUpTo> },
    /// D70: a field past the last anchor the tag header confirms (a struct
    /// across that point stops here).
    Unconfirmed { at: String },
    /// PRD §6.4.4: reading stopped at a field type the table doesn't know.
    UnknownType { at: String, type_name: String },
    /// A field whose size can't be computed (padding without a length).
    UnknownSize { at: String },
    /// The tag block has entries, but the header lists no data block for it.
    NoDataBlock,
    /// The data the definition describes lies outside its data block.
    OutOfBounds { at: String },
}

/// How much of a main struct of another size was read (D70): its first
/// `bytes`, which end with the anchor at `last_anchor` (a field path).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadUpTo {
    pub bytes: u32,
    pub last_anchor: String,
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
    StringId(StringIdValue),
    Tag(FourCc),
    TagRef(TagRef),
    Data(DataRef),
    Resource(ResourceRef),
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
    /// The field's (data block, offset): where the header's tag reference list puts it.
    pub place: (u32, u32),
}

/// A string_id field: its hash, and the string it hashes, from the tag's own
/// string_id list (PRD §5.2). 0 means none; another hash without a string is
/// unresolved (PRD §8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringIdValue {
    pub hash: u32,
    pub text: Option<String>,
}

/// A pageable resource field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceRef {
    /// The position of its resource struct among the tag's resource structs:
    /// probably the resource `[index]` of the file entry (PRD §12 Q9).
    /// `None` when the header lists no resource struct here.
    pub index: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataRef {
    /// Bytes of data (0 = empty).
    pub size: u32,
    /// The data block holding the bytes (the header's data reference list);
    /// `None` for a null reference or one the list doesn't have.
    pub target: Option<u32>,
    /// For a mapping function's curve only: MurmurHash3 x86 32 (seed 0) of
    /// its bytes, so equal curves can be told from different ones without
    /// decoding them (decisions D46).
    pub fingerprint: Option<u32>,
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

/// What an enum field's value names (PRD §5.3: an option is its position).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnumOption<'d> {
    Named(&'d Name),
    /// No option at that position (PRD §8 `no_option`).
    Missing {
        value: i64,
        defined: usize,
    },
    /// The field has no `<item>`s (PRD §8 `no_options`).
    NoOptions,
}

/// A flags field's set bits (PRD §5.3: an option's bit is its position).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlagBits<'d> {
    pub set: Vec<(u32, &'d Name)>,
    /// Set bits with no option (PRD §8 `unnamed_bits`).
    pub unnamed: Vec<u32>,
}

impl<'d> Node<'d> {
    /// For an enum field: the option its value names.
    pub fn enum_option(&self) -> Option<EnumOption<'d>> {
        let Content::Value(Value::Enum(v)) = self.content else { return None };
        let options = &self.field.options;
        Some(match usize::try_from(v).ok().and_then(|i| options.get(i)) {
            Some(n) => EnumOption::Named(n),
            None if options.is_empty() => EnumOption::NoOptions,
            None => EnumOption::Missing { value: v, defined: options.len() },
        })
    }

    /// For a flags field: which bits are set, named or not.
    pub fn flag_bits(&self) -> Option<FlagBits<'d>> {
        let Content::Value(Value::Flags(raw)) = self.content else { return None };
        let mut out = FlagBits { set: Vec::new(), unnamed: Vec::new() };
        for bit in (0..64).filter(|b| raw & (1u64 << b) != 0) {
            match self.field.options.get(bit as usize) {
                Some(n) => out.set.push((bit, n)),
                None => out.unnamed.push(bit),
            }
        }
        Some(out)
    }
}

/// The tag block a block index field points into: `targetGuid` matched with a
/// block field's `srcguid`, else `targetName` with its `srcName` (PRD §5.3),
/// looked for in the enclosing structs, nearest first. `scopes` runs from the
/// main struct to the struct holding the index field.
pub fn target_block<'a, 'd>(index_field: &Field, scopes: &[&'a Body<'d>]) -> Option<&'a Node<'d>> {
    let wanted = |f: &Field| match (index_field.attr("targetGuid"), index_field.attr("targetName")) {
        (Some(g), _) => f.attr("srcguid").is_some_and(|s| s.eq_ignore_ascii_case(g)),
        (None, Some(n)) => f.attr("srcName").is_some_and(|s| s == n),
        (None, None) => false,
    };
    scopes
        .iter()
        .rev()
        .find_map(|body| body.children.iter().find(|n| matches!(n.content, Content::Block(_)) && wanted(n.field)))
}

/// A tag read with its definition.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadTag<'d> {
    /// The main struct's fields.
    pub root: Body<'d>,
    pub divergences: Vec<Divergence>,
}
