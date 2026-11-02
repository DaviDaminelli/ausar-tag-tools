//! `diff_tags` (PRD §6.6): where two read tags of one class differ. Domain
//! only: typed differences with typed paths; the tool renders the values.

use h5_meta::{Body, Content, FieldPath, Node, Select, Step, Value};

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
    pub path: FieldPath,
    pub kind: Kind,
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
    // One path for the whole walk; copied only into the differences found (D26).
    let mut path = Vec::with_capacity(at.steps.len());
    follow(a, b, &at.steps, &mut path, &mut out);
    out
}

fn record(path: &[Step], step: Option<Step>, kind: Kind, out: &mut Vec<Difference>) {
    let mut steps = path.to_vec();
    steps.extend(step);
    out.push(Difference { path: FieldPath { steps }, kind });
}

/// Walks the rest of the requested path in both bodies, then compares there.
fn follow(a: &Body<'_>, b: &Body<'_>, rest: &[Step], path: &mut Vec<Step>, out: &mut Vec<Difference>) {
    let Some((step, rest)) = rest.split_first() else { return diff_into(a, b, path, out) };
    let (Some(x), Some(y)) = (a.children.get(step.field), b.children.get(step.field)) else {
        return record(path, Some(*step), Kind::NotRead, out);
    };
    match (&x.content, &y.content) {
        (Content::Block(p), Content::Block(q)) if !rest.is_empty() || step.select.is_some() => {
            let picked = match step.select {
                None => 0..1,
                Some(Select::Entry(e)) => e..e + 1,
                Some(Select::Every) => 0..p.count.max(q.count),
            };
            for e in picked {
                let here = Step { field: step.field, select: Some(Select::Entry(e)) };
                match (
                    (e < p.count).then(|| p.entries.get(e as usize)),
                    (e < q.count).then(|| q.entries.get(e as usize)),
                ) {
                    (Some(Some(pe)), Some(Some(qe))) => {
                        path.push(here);
                        follow(pe, qe, rest, path, out);
                        path.pop();
                    }
                    (Some(_), None) => record(path, Some(here), Kind::OnlyIn(Side::A), out),
                    (None, Some(_)) => record(path, Some(here), Kind::OnlyIn(Side::B), out),
                    (None, None) => {}
                    _ => record(path, Some(here), Kind::NotRead, out),
                }
            }
        }
        (Content::Struct(p), Content::Struct(q)) | (Content::Array(p), Content::Array(q)) if !rest.is_empty() => {
            path.push(*step);
            follow(p, q, rest, path, out);
            path.pop();
        }
        _ => diff_node(x, y, step.field, path, out),
    }
}

fn diff_into(a: &Body<'_>, b: &Body<'_>, path: &mut Vec<Step>, out: &mut Vec<Difference>) {
    if a.problem.is_some() || b.problem.is_some() {
        record(path, None, Kind::NotRead, out);
    }
    for (i, (x, y)) in a.children.iter().zip(&b.children).enumerate() {
        diff_node(x, y, i, path, out);
    }
}

/// Compares field `i` of the body at `path` in both tags.
fn diff_node(x: &Node<'_>, y: &Node<'_>, i: usize, path: &mut Vec<Step>, out: &mut Vec<Difference>) {
    let here = Step { field: i, select: None };
    match (&x.content, &y.content) {
        (Content::Value(p), Content::Value(q)) => {
            if !same_value(p, q) {
                record(path, Some(here), Kind::Changed, out);
            }
        }
        // A curve is one value (D46): one row, which shows its fingerprint.
        (Content::Struct(p), Content::Struct(q)) if x.field.is_mapping_function() => {
            let mut inside = Vec::new();
            diff_into(p, q, path, &mut inside);
            if !inside.is_empty() {
                let not_read = inside.iter().any(|d| d.kind == Kind::NotRead);
                record(path, Some(here), if not_read { Kind::NotRead } else { Kind::Changed }, out);
            }
        }
        (Content::Struct(p), Content::Struct(q)) | (Content::Array(p), Content::Array(q)) => {
            path.push(here);
            diff_into(p, q, path, out);
            path.pop();
        }
        (Content::Block(p), Content::Block(q)) => {
            if p.problem.is_some() || q.problem.is_some() {
                record(path, Some(here), Kind::NotRead, out);
            }
            if p.count != q.count {
                record(path, Some(here), Kind::Count { a: p.count, b: q.count }, out);
            }
            let entry = |e: usize| Step { field: i, select: Some(Select::Entry(e as u32)) };
            for (e, (pe, qe)) in p.entries.iter().zip(&q.entries).enumerate() {
                path.push(entry(e));
                diff_into(pe, qe, path, out);
                path.pop();
            }
            let (longer, side) =
                if p.entries.len() > q.entries.len() { (&p.entries, Side::A) } else { (&q.entries, Side::B) };
            for e in p.entries.len().min(q.entries.len())..longer.len() {
                record(path, Some(entry(e)), Kind::OnlyIn(side), out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
