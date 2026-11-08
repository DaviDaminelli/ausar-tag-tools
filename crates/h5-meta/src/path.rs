//! Field paths and sections (PRD §6.4.2, §6.4.3). A path is resolved against
//! the definition's own names: real names contain `.` and `[…]` (125 and 157
//! of them, docs/format-notes.md "Names"), so each step is the longest name at
//! its level that the text starts with, followed by the end, `.` or an index.

use h5_defs::definition::{Custom, Field, FieldKind, custom_role};
use h5_defs::field_type::FieldType;

use crate::tree::{Body, Content, Node};

/// Which entries of a tag block a step selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Select {
    Entry(u32),
    Every,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// Position of the field among its siblings in the definition.
    pub field: usize,
    pub select: Option<Select>,
}

/// A resolved path; empty = the whole tag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldPath {
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathError {
    /// No field at this level has that name; `similar` lists up to 5 that do.
    NoSuchField { at: String, name: String, similar: Vec<String> },
    /// `[i]` or `[*]` after a field that isn't a tag block.
    NotABlock { at: String },
    /// A step after a field that holds a value, not fields.
    NoFields { at: String },
    /// The tag block has fewer entries.
    OutOfRange { at: String, index: u32, count: u32 },
    /// The tag block has no entries to step into.
    Empty { at: String },
    /// The field wasn't read (a problem before it, PRD §8).
    NotRead { at: String },
}

fn is_block(f: &Field) -> bool {
    f.kind == FieldKind::Known(FieldType::Block64)
}

fn has_fields(f: &Field) -> bool {
    matches!(f.kind, FieldKind::Known(FieldType::Struct | FieldType::Array | FieldType::Block64))
}

/// `[n]` or `[*]` at the start of `text`, followed by the end or `.`: the
/// selection and the bytes it takes.
fn index_at(text: &str) -> Option<(Select, usize)> {
    let inner = text.strip_prefix('[')?;
    let close = inner.find(']')?;
    let after = &inner[close + 1..];
    if !(after.is_empty() || after.starts_with('.')) {
        return None;
    }
    let select = match &inner[..close] {
        "*" => Select::Every,
        n => Select::Entry(n.parse().ok()?),
    };
    Some((select, close + 2))
}

fn join(at: &str, step: &str) -> String {
    if at.is_empty() { step.to_owned() } else { format!("{at}.{step}") }
}

/// How a step's selection is written: `[2]`, `[*]`, or nothing.
pub fn suffix(select: Option<Select>) -> String {
    match select {
        Some(Select::Entry(i)) => format!("[{i}]"),
        Some(Select::Every) => "[*]".into(),
        None => String::new(),
    }
}

/// Up to 5 field names at a level, closest to `name` first (edit distance).
fn similar(level: &[Field], name: &str) -> Vec<String> {
    let want = name.to_lowercase();
    let mut scored: Vec<(usize, &str)> = level
        .iter()
        .filter_map(|f| f.step.as_deref())
        .map(|s| (strsim::levenshtein(&want, &s.to_lowercase()), s))
        .collect();
    scored.sort_by_key(|(d, _)| *d);
    scored.into_iter().take(5).map(|(_, s)| s.to_owned()).collect()
}

/// Resolves `text` against a definition's top-level `fields`.
pub fn parse_path(fields: &[Field], text: &str) -> Result<FieldPath, PathError> {
    let mut steps = Vec::new();
    let mut level = fields;
    let mut rest = text.trim();
    let mut at = String::new();
    while !rest.is_empty() {
        let fits = |step: &str| {
            rest.get(..step.len()).is_some_and(|head| head.eq_ignore_ascii_case(step)) && {
                let after = &rest[step.len()..];
                after.is_empty() || after.starts_with('.') || index_at(after).is_some()
            }
        };
        let best = level
            .iter()
            .enumerate()
            .filter_map(|(i, f)| Some((i, f.step.as_deref()?)))
            .filter(|(_, step)| fits(step))
            .max_by_key(|(_, step)| step.len());
        let Some((i, step)) = best else {
            let name = rest.split(['.', '[']).next().unwrap_or(rest).trim().to_owned();
            return Err(PathError::NoSuchField { similar: similar(level, &name), at, name });
        };
        let f = &level[i];
        rest = &rest[step.len()..];
        at = join(&at, step);
        let mut select = None;
        if let Some((s, used)) = index_at(rest) {
            if !is_block(f) {
                return Err(PathError::NotABlock { at });
            }
            select = Some(s);
            rest = &rest[used..];
            at.push_str(&suffix(select));
        }
        steps.push(Step { field: i, select });
        if let Some(r) = rest.strip_prefix('.') {
            if !has_fields(f) {
                return Err(PathError::NoFields { at });
            }
            rest = r;
            level = &f.children;
        }
    }
    Ok(FieldPath { steps })
}

impl FieldPath {
    /// The path as text, with the definition's names (`barrels[1].rounds per second`).
    pub fn display(&self, fields: &[Field]) -> String {
        self.display_with(fields, |_, select| suffix(select))
    }

    /// The same, with each step's index written by `index(step number, selection)`.
    pub fn display_with(&self, fields: &[Field], mut index: impl FnMut(usize, Option<Select>) -> String) -> String {
        let mut level = fields;
        let mut out = String::new();
        for (k, s) in self.steps.iter().enumerate() {
            let f = &level[s.field];
            out = join(&out, f.step.as_deref().unwrap_or_default());
            out.push_str(&index(k, s.select));
            level = &f.children;
        }
        out
    }
}

/// A node a path selects; for a path ending at a tag block entry, `node` is
/// the block and `entry` the entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Selected<'a, 'd> {
    /// The concrete path (`barrels[0].rounds per second`).
    pub path: String,
    pub node: &'a Node<'d>,
    pub entry: Option<u32>,
    /// The structs around `node` and their paths, from the main struct to the
    /// one holding it (block indices inside may point into any of them).
    pub scopes: Vec<(&'a Body<'d>, String)>,
}

/// The entries `select` picks of block `node` (at `at`): (path, entry index, body).
fn entries<'a, 'd>(
    node: &'a Node<'d>,
    select: Option<Select>,
    at: &str,
) -> Result<Vec<(String, u32, &'a Body<'d>)>, PathError> {
    let Content::Block(b) = &node.content else { return Err(PathError::NoFields { at: at.to_owned() }) };
    let picked: Vec<u32> = match select {
        Some(Select::Every) => (0..b.count).collect(),
        Some(Select::Entry(i)) if i >= b.count => {
            return Err(PathError::OutOfRange { at: at.to_owned(), index: i, count: b.count });
        }
        Some(Select::Entry(i)) => vec![i],
        None if b.count == 0 => return Err(PathError::Empty { at: at.to_owned() }),
        // Passing through a block without an index: entry 0 (PRD §6.4.2).
        None => vec![0],
    };
    picked
        .into_iter()
        .map(|i| {
            let body = b.entries.get(i as usize).ok_or_else(|| PathError::NotRead { at: at.to_owned() })?;
            Ok((format!("{at}[{i}]"), i, body))
        })
        .collect()
}

/// The nodes `path` selects in a read tag (several with `[*]`).
pub fn select<'a, 'd>(root: &'a Body<'d>, path: &FieldPath) -> Result<Vec<Selected<'a, 'd>>, PathError> {
    // (path, body, the bodies around it)
    type Scopes<'a, 'd> = Vec<(&'a Body<'d>, String)>;
    let mut current: Vec<(String, &'a Body<'d>, Scopes<'a, 'd>)> = vec![(String::new(), root, Vec::new())];
    let mut out = Vec::new();
    for (k, step) in path.steps.iter().enumerate() {
        let last = k + 1 == path.steps.len();
        let mut next = Vec::new();
        for (at, body, outer) in &current {
            let mut scopes = outer.clone();
            scopes.push((*body, at.clone()));
            // Children follow the definition's fields, in order, until a problem.
            let Some(node) = body.children.get(step.field) else {
                return Err(PathError::NotRead { at: at.clone() });
            };
            let here = join(at, node.name.unwrap_or_default());
            match (&node.content, last) {
                (Content::Block(_), true) if step.select.is_some() => {
                    for (p, i, _) in entries(node, step.select, &here)? {
                        out.push(Selected { path: p, node, entry: Some(i), scopes: scopes.clone() });
                    }
                }
                (_, true) => out.push(Selected { path: here, node, entry: None, scopes: scopes.clone() }),
                (Content::Struct(b) | Content::Array(b), false) => next.push((here, b, scopes.clone())),
                (Content::Block(_), false) => {
                    next.extend(entries(node, step.select, &here)?.into_iter().map(|(p, _, b)| (p, b, scopes.clone())));
                }
                (_, false) => return Err(PathError::NoFields { at: here }),
            }
        }
        current = next;
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    /// A top-level section label (`## WEAPON`).
    Class,
    Explanation,
    /// A `_field_custom` group heading, or a section label below the top level.
    Group,
}

impl SectionKind {
    fn rank(self) -> u8 {
        if self == SectionKind::Class { 2 } else { 1 }
    }
}

/// A section (PRD §6.4.3): fields `start + 1 .. end` of the struct at `parent`,
/// headed by field `start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub parent: FieldPath,
    pub start: usize,
    pub end: usize,
    pub heading: String,
    pub kind: SectionKind,
}

/// The heading field `fields[i]` is, if any (`top`: at the definition's top level).
fn heading(fields: &[Field], i: usize, top: bool) -> Option<(String, SectionKind)> {
    let f = &fields[i];
    match &f.kind {
        FieldKind::Known(FieldType::Explanation) => Some((f.name.display.clone(), SectionKind::Explanation)),
        FieldKind::Known(FieldType::Custom) => match custom_role(fields, i)? {
            Custom::ClassSection(label) if top => Some((label, SectionKind::Class)),
            Custom::ClassSection(label) => Some((label, SectionKind::Group)),
            Custom::Group(name) => Some((name, SectionKind::Group)),
            Custom::Widget => None,
        },
        _ => None,
    }
}

/// Every section whose heading is `text` (case-insensitive), in definition order.
pub fn find_sections(fields: &[Field], text: &str) -> Vec<Section> {
    fn visit(fields: &[Field], parent: &FieldPath, text: &str, out: &mut Vec<Section>) {
        let top = parent.steps.is_empty();
        let headings: Vec<Option<(String, SectionKind)>> = (0..fields.len()).map(|i| heading(fields, i, top)).collect();
        for (i, h) in headings.iter().enumerate() {
            if let Some((name, kind)) = h
                && name.eq_ignore_ascii_case(text.trim())
            {
                let end = (i + 1..fields.len())
                    .find(|&j| headings[j].as_ref().is_some_and(|(_, k)| k.rank() >= kind.rank()))
                    .unwrap_or(fields.len());
                out.push(Section { parent: parent.clone(), start: i, end, heading: name.clone(), kind: *kind });
            }
        }
        for (i, f) in fields.iter().enumerate().filter(|(_, f)| has_fields(f)) {
            let mut p = parent.clone();
            p.steps.push(Step { field: i, select: None });
            visit(&f.children, &p, text, out);
        }
    }
    let mut out = Vec::new();
    visit(fields, &FieldPath::default(), text, &mut out);
    out
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
