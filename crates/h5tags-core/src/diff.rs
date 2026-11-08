//! `diff_tags` (PRD §6.6): where two read tags of one class differ. Domain
//! only: typed differences with typed paths; the tool renders the values.

use std::collections::HashMap;
use std::ops::Range;

use h5_meta::{Block, Body, Content, FieldPath, Node, Select, Step, Value};

/// Which tag of the two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The value at the path differs.
    Changed,
    /// The tag block at the path has another entry count.
    Count { a: u32, b: u32 },
    /// The entry at the path exists in one tag only.
    OnlyIn(Side),
    /// One side (or both) couldn't be read here: not compared.
    NotRead,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    /// Where, with tag a's entry indices; for an entry only tag b has, b's.
    pub path: FieldPath,
    /// The same place with tag b's entry indices, when they differ from a's
    /// (entries matched by label, decisions D72).
    pub in_b: Option<FieldPath>,
    pub kind: Kind,
}

impl Difference {
    /// The difference's place in tag `side`.
    pub fn path_in(&self, side: Side) -> &FieldPath {
        match side {
            Side::A => &self.path,
            Side::B => self.in_b.as_ref().unwrap_or(&self.path),
        }
    }
}

/// The walk's place in both tags: the same steps, with entry indices that
/// differ once entries are matched by label. One for the whole walk, copied
/// only into the differences found (D26).
#[derive(Default)]
struct Here {
    a: Vec<Step>,
    b: Vec<Step>,
}

impl Here {
    fn push(&mut self, a: Step, b: Step) {
        self.a.push(a);
        self.b.push(b);
    }

    fn pop(&mut self) {
        self.a.pop();
        self.b.pop();
    }
}

/// Which entries of two tag blocks are compared (D72), as (a's index, b's
/// index); `None` = the entry exists in the other tag only.
type Pairs = Vec<(Option<u32>, Option<u32>)>;

/// By label, when every entry of both blocks has one, no label repeats within
/// a block, and the blocks share at least one: a's entries in order (with the
/// entry of b that has their label), then the entries only b has. `None`:
/// compare by position.
fn label_pairs(p: &Block<'_>, q: &Block<'_>) -> Option<Pairs> {
    fn labels<'k>(k: &'k Block<'_>) -> Option<HashMap<&'k str, u32>> {
        // Most blocks have unlabelled entries: no map for them.
        let read = k.problem.is_none() && k.entries.len() == k.count as usize;
        if !read || k.entries.is_empty() || !k.entries.iter().all(|e| e.label_text().is_some()) {
            return None;
        }
        let mut by_label = HashMap::with_capacity(k.entries.len());
        for (i, e) in k.entries.iter().enumerate() {
            if by_label.insert(e.label_text()?, i as u32).is_some() {
                return None;
            }
        }
        Some(by_label)
    }
    let (in_p, in_q) = (labels(p)?, labels(q)?);
    if !in_p.keys().any(|l| in_q.contains_key(l)) {
        return None;
    }
    // Every entry has a label here (`labels` checked); borrowed, no copies.
    let mut out: Pairs = Vec::with_capacity(p.entries.len() + q.entries.len());
    for (i, e) in p.entries.iter().enumerate() {
        out.push((Some(i as u32), e.label_text().and_then(|l| in_q.get(l)).copied()));
    }
    for (j, e) in q.entries.iter().enumerate() {
        if e.label_text().is_some_and(|l| !in_p.contains_key(l)) {
            out.push((None, Some(j as u32)));
        }
    }
    Some(out)
}

/// Whether two read values say the same thing. A tag reference also records
/// where its field sits, which differs between any two tags: references
/// compare by target. Curves compare by fingerprint, other data by size (its
/// bytes aren't read). Resources (contents not read in v1) and padding aren't
/// compared. Reals compare by their stored bits (so NaN is the same as NaN).
pub fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::TagRef(x), Value::TagRef(y)) => {
            x.field.global_id == y.field.global_id && x.field.class_code == y.field.class_code
        }
        (Value::Data(x), Value::Data(y)) => x.size == y.size && x.fingerprint == y.fingerprint,
        (Value::Resource(_), Value::Resource(_)) | (Value::Padding, Value::Padding) => true,
        (Value::Real(x), Value::Real(y)) => x.to_bits() == y.to_bits(),
        (Value::Reals(x), Value::Reals(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits())
        }
        _ => a == b,
    }
}

/// The differences under `at` (empty = the whole tag) between two read tags
/// of one definition (so their fields line up one to one), in field order,
/// with entry indices in every path. A block passed through without an index
/// is entry 0 (PRD §6.4.2); `[*]` compares every entry.
pub fn diff_at(a: &Body<'_>, b: &Body<'_>, at: &FieldPath) -> Vec<Difference> {
    let mut out = Vec::new();
    let mut here = Here::default();
    follow(a, b, &at.steps, &mut here, &mut out, &mut diff_into, false);
    out
}

/// The differences in fields `fields` of the struct at `parent`: a section
/// (PRD §6.4.3), as `find_sections` gives it (fields `start + 1 .. end`).
/// Inside a tag block passed without an index, entry 0, like `read_tag`.
pub fn diff_section(a: &Body<'_>, b: &Body<'_>, parent: &FieldPath, fields: Range<usize>) -> Vec<Difference> {
    let mut out = Vec::new();
    let mut here = Here::default();
    let mut compare = |a: &Body<'_>, b: &Body<'_>, here: &mut Here, out: &mut Vec<Difference>| {
        if a.problem.is_some() || b.problem.is_some() {
            record(here, None, Kind::NotRead, out);
        }
        for i in fields.clone() {
            if let (Some(x), Some(y)) = (a.children.get(i), b.children.get(i)) {
                diff_node(x, y, i, here, out);
            }
        }
    };
    follow(a, b, &parent.steps, &mut here, &mut out, &mut compare, true);
    out
}

/// A difference at `here`, plus `last` (a's step, b's step) if given.
fn record(here: &Here, last: Option<(Step, Step)>, kind: Kind, out: &mut Vec<Difference>) {
    let (mut a, mut b) = (here.a.clone(), here.b.clone());
    if let Some((x, y)) = last {
        a.push(x);
        b.push(y);
    }
    let in_b = (a != b).then_some(FieldPath { steps: b });
    out.push(Difference { path: FieldPath { steps: a }, in_b, kind });
}

/// A step to entry `e` of the tag block at field `field`.
fn entry(field: usize, e: u32) -> Step {
    Step { field, select: Some(Select::Entry(e)) }
}

/// Entry `i` of a tag block: `None` past its count, `Some(None)` within it
/// but not read.
fn entry_in<'k, 'd>(k: &'k Block<'d>, i: Option<u32>) -> Option<Option<&'k Body<'d>>> {
    i.filter(|&i| i < k.count).map(|i| k.entries.get(i as usize))
}

/// Compares entry `ia` of a's tag block at field `field` with entry `ib` of b's.
fn diff_entry(
    pe: &Body<'_>,
    qe: &Body<'_>,
    field: usize,
    (ia, ib): (u32, u32),
    here: &mut Here,
    out: &mut Vec<Difference>,
) {
    here.push(entry(field, ia), entry(field, ib));
    diff_into(pe, qe, here, out);
    here.pop();
}

/// What is compared where a walk ends: the two bodies reached.
type End<'e> = dyn FnMut(&Body<'_>, &Body<'_>, &mut Here, &mut Vec<Difference>) + 'e;

/// Walks the rest of the requested path in both bodies, then compares there
/// with `end`. The last step's struct or tag block is compared as a node
/// unless `into_last` (then `end` gets its body, or each picked entry). A
/// picked entry is a's; b's is the one with its label (D72), else the same
/// index.
fn follow(
    a: &Body<'_>,
    b: &Body<'_>,
    rest: &[Step],
    here: &mut Here,
    out: &mut Vec<Difference>,
    end: &mut End<'_>,
    into_last: bool,
) {
    let Some((step, rest)) = rest.split_first() else { return end(a, b, here, out) };
    let (Some(x), Some(y)) = (a.children.get(step.field), b.children.get(step.field)) else {
        return record(here, Some((*step, *step)), Kind::NotRead, out);
    };
    let descend = !rest.is_empty() || into_last;
    match (&x.content, &y.content) {
        (Content::Block(p), Content::Block(q)) if descend || step.select.is_some() => {
            let by_label = label_pairs(p, q);
            let mut visit = |ia: Option<u32>, ib: Option<u32>| {
                let field = step.field;
                match (entry_in(p, ia), entry_in(q, ib), ia, ib) {
                    (Some(Some(pe)), Some(Some(qe)), Some(i), Some(j)) => {
                        here.push(entry(field, i), entry(field, j));
                        follow(pe, qe, rest, here, out, end, into_last);
                        here.pop();
                    }
                    (Some(_), None, Some(i), _) => {
                        record(here, Some((entry(field, i), entry(field, i))), Kind::OnlyIn(Side::A), out)
                    }
                    (None, Some(_), _, Some(j)) => {
                        record(here, Some((entry(field, j), entry(field, j))), Kind::OnlyIn(Side::B), out)
                    }
                    (None, None, ..) => {}
                    (.., i, j) => {
                        let e = i.or(j).unwrap_or_default();
                        record(here, Some((entry(field, e), entry(field, e))), Kind::NotRead, out)
                    }
                }
            };
            match (step.select, &by_label) {
                (Some(Select::Every), Some(pairs)) => pairs.iter().for_each(|&(i, j)| visit(i, j)),
                (Some(Select::Every), None) => (0..p.count.max(q.count)).for_each(|e| visit(Some(e), Some(e))),
                (select, by_label) => {
                    let e = match select {
                        Some(Select::Entry(e)) => e,
                        _ => 0,
                    };
                    let matched = by_label.as_ref().and_then(|pairs| pairs.iter().find(|(i, _)| *i == Some(e)));
                    match matched {
                        Some(&(i, j)) => visit(i, j),
                        None => visit(Some(e), Some(e)),
                    }
                }
            }
        }
        (Content::Struct(p), Content::Struct(q)) | (Content::Array(p), Content::Array(q)) if descend => {
            here.push(*step, *step);
            follow(p, q, rest, here, out, end, into_last);
            here.pop();
        }
        _ => diff_node(x, y, step.field, here, out),
    }
}

fn diff_into(a: &Body<'_>, b: &Body<'_>, here: &mut Here, out: &mut Vec<Difference>) {
    if a.problem.is_some() || b.problem.is_some() {
        record(here, None, Kind::NotRead, out);
    }
    for (i, (x, y)) in a.children.iter().zip(&b.children).enumerate() {
        diff_node(x, y, i, here, out);
    }
}

/// Compares field `i` of the body at `here` in both tags.
fn diff_node(x: &Node<'_>, y: &Node<'_>, i: usize, here: &mut Here, out: &mut Vec<Difference>) {
    let field = Step { field: i, select: None };
    let at = Some((field, field));
    match (&x.content, &y.content) {
        (Content::Value(p), Content::Value(q)) => {
            if !same_value(p, q) {
                record(here, at, Kind::Changed, out);
            }
        }
        // A curve is one value (D46): one row, which shows its fingerprint.
        (Content::Struct(p), Content::Struct(q)) if x.field.is_mapping_function() => {
            let mut inside = Vec::new();
            diff_into(p, q, here, &mut inside);
            if !inside.is_empty() {
                let not_read = inside.iter().any(|d| d.kind == Kind::NotRead);
                record(here, at, if not_read { Kind::NotRead } else { Kind::Changed }, out);
            }
        }
        (Content::Struct(p), Content::Struct(q)) | (Content::Array(p), Content::Array(q)) => {
            here.push(field, field);
            diff_into(p, q, here, out);
            here.pop();
        }
        (Content::Block(p), Content::Block(q)) => {
            if p.problem.is_some() || q.problem.is_some() {
                record(here, at, Kind::NotRead, out);
            }
            if p.count != q.count {
                record(here, at, Kind::Count { a: p.count, b: q.count }, out);
            }
            if let Some(pairs) = label_pairs(p, q) {
                for (ia, ib) in pairs {
                    match (ia, ib) {
                        (Some(e), Some(f)) => {
                            diff_entry(&p.entries[e as usize], &q.entries[f as usize], i, (e, f), here, out)
                        }
                        (Some(e), None) => record(here, Some((entry(i, e), entry(i, e))), Kind::OnlyIn(Side::A), out),
                        (None, Some(f)) => record(here, Some((entry(i, f), entry(i, f))), Kind::OnlyIn(Side::B), out),
                        (None, None) => {}
                    }
                }
                return;
            }
            // By position: no pairs list (D26).
            for (e, (pe, qe)) in p.entries.iter().zip(&q.entries).enumerate() {
                diff_entry(pe, qe, i, (e as u32, e as u32), here, out);
            }
            let (longer, side) =
                if p.entries.len() > q.entries.len() { (&p.entries, Side::A) } else { (&q.entries, Side::B) };
            for e in p.entries.len().min(q.entries.len())..longer.len() {
                record(here, Some((entry(i, e as u32), entry(i, e as u32))), Kind::OnlyIn(side), out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
