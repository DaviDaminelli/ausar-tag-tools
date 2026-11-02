//! `query_field` (PRD §6.6): one field across every tag of a class, with an
//! optional filter and sort. Domain only: typed filters in, typed rows out.

use std::cmp::Ordering;

use h5_defs::definition::{Definition, Field, FieldKind};
use h5_defs::field_type::FieldType;
use h5_meta::{Content, EnumOption, FieldPath, Node, PathError, Value, select};
use h5_module::FourCc;

use crate::catalog::{Build, Tag};
use crate::filters::ascii_matches;
use crate::query::choose_copy;
use crate::stored::{ReadError, read_stored_tag};

/// A comparison: `== != > >= < <= contains`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
}

impl Op {
    /// Longest first, so `>=` isn't read as `>`.
    pub const ALL: [(&'static str, Op); 7] = [
        ("contains", Op::Contains),
        ("==", Op::Eq),
        ("!=", Op::Ne),
        (">=", Op::Ge),
        ("<=", Op::Le),
        (">", Op::Gt),
        ("<", Op::Lt),
    ];

    fn holds(self, o: Ordering) -> bool {
        match self {
            Op::Eq => o.is_eq(),
            Op::Ne => o.is_ne(),
            Op::Gt => o.is_gt(),
            Op::Ge => o.is_ge(),
            Op::Lt => o.is_lt(),
            Op::Le => o.is_le(),
            Op::Contains => false,
        }
    }
}

/// `<op> <value>`, as the user wrote the value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldFilter {
    pub op: Op,
    pub value: String,
}

/// What a value is, for filtering and sorting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// Integers, reals, block indices: compared as numbers.
    Number,
    /// Compared by option label (case-insensitive), or by number.
    Enum,
    /// `contains <option label>`: whether that flag is set.
    Flags,
    /// Strings, string_ids, tag references (by name): `== != contains`.
    Text,
    /// Bounds, points, vectors, colors: several numbers in one value.
    Multi,
    /// Data, resources, padding.
    Other,
}

impl ValueKind {
    pub fn of(v: &Value) -> ValueKind {
        match v {
            Value::Int(_) | Value::Uint(_) | Value::Real(_) | Value::BlockIndex(_) => ValueKind::Number,
            Value::Enum(_) => ValueKind::Enum,
            Value::Flags(_) => ValueKind::Flags,
            Value::String(_) | Value::StringId(_) | Value::TagRef(_) | Value::Tag(_) => ValueKind::Text,
            Value::Ints(_) | Value::Reals(_) | Value::Color(_) => ValueKind::Multi,
            Value::Data(_) | Value::Resource(_) | Value::Padding => ValueKind::Other,
        }
    }

    fn name(self) -> &'static str {
        match self {
            ValueKind::Number => "a number",
            ValueKind::Enum => "an enum",
            ValueKind::Flags => "flags",
            ValueKind::Text => "text",
            ValueKind::Multi => "several numbers (bounds, a point, a vector or a color)",
            ValueKind::Other => "data, a resource or padding",
        }
    }
}

/// Row order (PRD §6.6): as the tags are listed, or by value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    /// By tag path.
    #[default]
    None,
    /// Smallest first (text: alphabetically).
    Asc,
    /// Largest first.
    Desc,
}

/// One selected value: its tag, and which of the tag's selections it is (for `[*]`).
pub struct Row<'b> {
    pub tag: &'b Tag,
    pub selection: usize,
    key: Option<SortKey>,
}

/// A tag the query couldn't read, and why (PRD §6.6: listed, never fatal).
pub struct Skipped<'b> {
    pub tag: &'b Tag,
    pub reason: SkipReason,
}

pub enum SkipReason {
    Read(ReadError),
    /// The path doesn't reach a value in this tag (an empty block, a part not read).
    Path(PathError),
}

pub struct Rows<'b> {
    pub rows: Vec<Row<'b>>,
    pub skipped: Vec<Skipped<'b>>,
}

/// The definition field `path` ends at, if it holds one value (not a struct,
/// array, tag block or curve).
pub fn value_field<'d>(fields: &'d [Field], path: &FieldPath, shown: &str) -> Result<&'d Field, QueryError> {
    let mut level = fields;
    let mut field = None;
    for s in &path.steps {
        let f = &level[s.field];
        field = Some(f);
        level = &f.children;
    }
    let Some(f) = field else { return Err(QueryError::NotAValue { field: shown.to_owned(), what: "the whole tag" }) };
    let what = match f.kind {
        FieldKind::Known(FieldType::Block64) => "a tag block",
        FieldKind::Known(FieldType::Struct) if f.is_mapping_function() => "a curve",
        FieldKind::Known(FieldType::Struct) => "a struct",
        FieldKind::Known(FieldType::Array) => "an array",
        _ => return Ok(f),
    };
    Err(QueryError::NotAValue { field: shown.to_owned(), what })
}

/// `query_field` (PRD §6.6): the value at `path` in each tag of `class` (its
/// default copy, §6.4.6), filtered and sorted. A filter or sort that doesn't
/// fit the values is an error at the first value read; tags that can't be
/// read are `skipped`.
pub fn query_field<'b>(
    b: &'b Build,
    class: FourCc,
    def: &Definition,
    path: &FieldPath,
    shown: &str,
    filter: Option<&FieldFilter>,
    sort: Sort,
) -> Result<Rows<'b>, QueryError> {
    let mut out = Rows { rows: Vec::new(), skipped: Vec::new() };
    let mut checked = false;
    for tag in b.tags.iter().filter(|t| t.class == class) {
        let Some(copy) = choose_copy(b, tag, None) else { continue };
        let stored = match read_stored_tag(b, copy) {
            Ok(s) => s,
            Err(e) => {
                out.skipped.push(Skipped { tag, reason: SkipReason::Read(e) });
                continue;
            }
        };
        let read = h5_meta::read_tag(def, &stored.header, &stored.tag_data);
        let selections = match select(&read.root, path) {
            Ok(s) => s,
            Err(e) => {
                out.skipped.push(Skipped { tag, reason: SkipReason::Path(e) });
                continue;
            }
        };
        for (i, s) in selections.iter().enumerate() {
            let Content::Value(v) = &s.node.content else { continue };
            if !checked {
                let kind = ValueKind::of(v);
                if let Some(f) = filter {
                    check_filter(shown, kind, f)?;
                }
                if sort != Sort::None {
                    check_sort(shown, kind)?;
                }
                checked = true;
            }
            if filter.is_none_or(|f| passes(b, s.node, f)) {
                let key = (sort != Sort::None).then(|| SortKey::of(b, s.node)).flatten();
                out.rows.push(Row { tag, selection: i, key });
            }
        }
    }
    let order = |a: &Row<'_>, z: &Row<'_>| match (&a.key, &z.key) {
        (Some(x), Some(y)) => x.compare(y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    match sort {
        Sort::None => {}
        Sort::Asc => out.rows.sort_by(order),
        Sort::Desc => out.rows.sort_by(|a, z| order(z, a)),
    }
    Ok(out)
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum QueryError {
    #[error("{field} is {what}, not a value")]
    NotAValue { field: String, what: &'static str },
    #[error("{field} is {}: {what} doesn't apply to it", kind.name())]
    Unsupported { field: String, kind: ValueKind, what: &'static str },
    #[error("'{value}' isn't a number")]
    NotANumber { value: String },
}

/// Whether `f` can apply to values of `kind` (`field` names it in the error).
pub fn check_filter(field: &str, kind: ValueKind, f: &FieldFilter) -> Result<(), QueryError> {
    let unsupported = |what| Err(QueryError::Unsupported { field: field.to_owned(), kind, what });
    let number = || f.value.parse::<f64>().map(|_| ()).map_err(|_| QueryError::NotANumber { value: f.value.clone() });
    match (kind, f.op) {
        (ValueKind::Number, Op::Contains) => unsupported("contains"),
        (ValueKind::Number, _) => number(),
        (ValueKind::Enum, Op::Eq | Op::Ne) => Ok(()),
        (ValueKind::Enum, Op::Contains) => unsupported("contains"),
        (ValueKind::Enum, _) => number(),
        (ValueKind::Flags, Op::Contains) => Ok(()),
        (ValueKind::Flags, _) => unsupported("a comparison (use contains <option>)"),
        (ValueKind::Text, Op::Eq | Op::Ne | Op::Contains) => Ok(()),
        (ValueKind::Text, _) => unsupported("an ordering comparison"),
        (ValueKind::Multi | ValueKind::Other, _) => unsupported("a filter"),
    }
}

/// Whether values of `kind` can be sorted.
pub fn check_sort(field: &str, kind: ValueKind) -> Result<(), QueryError> {
    match kind {
        ValueKind::Multi | ValueKind::Other => {
            Err(QueryError::Unsupported { field: field.to_owned(), kind, what: "sort" })
        }
        _ => Ok(()),
    }
}

/// A read value's text as filters and sorts see it: a tag reference by its
/// display name (or the name the tag stores, if it isn't indexed).
fn text<'a>(b: &Build, v: &'a Value) -> Option<std::borrow::Cow<'a, str>> {
    match v {
        Value::String(s) => Some(s.as_str().into()),
        Value::StringId(s) => s.text.as_deref().map(Into::into),
        Value::TagRef(r) if r.field.is_null() => Some("".into()),
        Value::TagRef(r) => Some(match b.tag_by_id(r.field.global_id) {
            Some(t) => b.display_name(t).into(),
            None => r.listed_name.clone().unwrap_or_default().into(),
        }),
        Value::Tag(c) => Some(c.to_string().into()),
        _ => None,
    }
}

fn number(v: &Value) -> Option<f64> {
    match *v {
        Value::Int(i) | Value::BlockIndex(i) | Value::Enum(i) => Some(i as f64),
        Value::Uint(u) | Value::Flags(u) => Some(u as f64),
        Value::Real(r) => Some(f64::from(r)),
        _ => None,
    }
}

fn label<'n>(n: &Node<'n>) -> Option<&'n str> {
    match n.enum_option()? {
        EnumOption::Named(name) => Some(name.display.as_str()),
        _ => None,
    }
}

/// Whether the value read at `n` passes `f` (already checked to apply).
pub fn passes(b: &Build, n: &Node<'_>, f: &FieldFilter) -> bool {
    let h5_meta::Content::Value(v) = &n.content else { return false };
    let q = f.value.as_bytes();
    match ValueKind::of(v) {
        ValueKind::Number => {
            let (Some(x), Ok(y)) = (number(v), f.value.parse::<f64>()) else { return false };
            f.op.holds(x.total_cmp(&y))
        }
        ValueKind::Enum => match (f.op, f.value.parse::<f64>()) {
            (_, Ok(y)) => number(v).is_some_and(|x| f.op.holds(x.total_cmp(&y))),
            (Op::Eq | Op::Ne, Err(_)) => {
                let same = label(n).is_some_and(|l| l.eq_ignore_ascii_case(&f.value));
                (f.op == Op::Eq) == same
            }
            _ => false,
        },
        ValueKind::Flags => n
            .flag_bits()
            .is_some_and(|bits| bits.set.iter().any(|(_, name)| name.display.eq_ignore_ascii_case(&f.value))),
        ValueKind::Text => {
            let t = text(b, v).unwrap_or_default();
            match f.op {
                Op::Contains => ascii_matches(t.as_bytes(), q).next().is_some(),
                Op::Ne => !t.eq_ignore_ascii_case(&f.value),
                _ => t.eq_ignore_ascii_case(&f.value),
            }
        }
        ValueKind::Multi | ValueKind::Other => false,
    }
}

/// How a row sorts: numbers by value, text and enums by text (lowercase).
#[derive(Clone, Debug, PartialEq)]
pub enum SortKey {
    Number(f64),
    Text(String),
}

impl SortKey {
    pub fn of(b: &Build, n: &Node<'_>) -> Option<SortKey> {
        let h5_meta::Content::Value(v) = &n.content else { return None };
        match ValueKind::of(v) {
            ValueKind::Number | ValueKind::Flags => number(v).map(SortKey::Number),
            ValueKind::Enum => Some(SortKey::Text(label(n).unwrap_or_default().to_ascii_lowercase())),
            ValueKind::Text => Some(SortKey::Text(text(b, v).unwrap_or_default().to_ascii_lowercase())),
            ValueKind::Multi | ValueKind::Other => None,
        }
    }

    /// Numbers before text; numbers by value (total order), text by bytes.
    pub fn compare(&self, other: &SortKey) -> Ordering {
        match (self, other) {
            (SortKey::Number(a), SortKey::Number(b)) => a.total_cmp(b),
            (SortKey::Text(a), SortKey::Text(b)) => a.cmp(b),
            (SortKey::Number(_), SortKey::Text(_)) => Ordering::Less,
            (SortKey::Text(_), SortKey::Number(_)) => Ordering::Greater,
        }
    }
}

#[cfg(test)]
#[path = "field_query_tests.rs"]
mod tests;
