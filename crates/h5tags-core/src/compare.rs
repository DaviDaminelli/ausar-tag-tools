//! `compare_builds` (PRD §6.6, M6): which tags two builds have, and which
//! hold different data. Tags are matched by global ID (format-notes: the ID is
//! a tag's identity across builds; a shared tag under another path is the
//! same asset named for each platform), and compared by the tag data
//! fingerprint of each build's copy read by default (D58, D81). Domain only.

use h5_module::FourCc;

use crate::catalog::{Build, Copy, Tag};
use crate::query::choose_copy;
use crate::references::ScanIndex;
use crate::stored::{ReadError, read_stored_tag};

/// Where a tag stands between build A and build B; also the rows' order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    /// In both, with different tag data.
    Different,
    OnlyInA,
    OnlyInB,
    /// In both, with the same tag data (counted, not listed).
    Same,
}

/// One tag that isn't the same in both builds.
pub struct Row<'b> {
    pub status: Status,
    /// Build A's tag; build B's for a tag only in B.
    pub tag: &'b Tag,
    /// Build B's tag, when it is in both (its path may name another platform).
    pub other: Option<&'b Tag>,
}

pub struct Comparison<'b> {
    /// Every tag not the same in both: different, then only in A, then only
    /// in B; within each, by class, then path.
    pub rows: Vec<Row<'b>>,
    /// Tags in both builds with the same data.
    pub same: usize,
    /// Tags in both builds whose copy one scan couldn't read: not compared (A's tag).
    pub not_compared: Vec<&'b Tag>,
}

impl Comparison<'_> {
    pub fn count(&self, status: Status) -> usize {
        match status {
            Status::Same => self.same,
            _ => self.rows.iter().filter(|r| r.status == status).count(),
        }
    }
}

/// The fingerprint of `tag`'s copy read by default in build `b`, if its scan read it.
fn print(b: &Build, scan: &ScanIndex, tag: &Tag) -> Option<(u64, u64)> {
    choose_copy(b, tag, None).and_then(|c| scan.outgoing(c)).map(|t| t.data_print)
}

/// Build A against build B (`sa`, `sb`: their header scans); with `class`,
/// only that class's tags.
pub fn compare_builds<'b>(
    a: &'b Build,
    sa: &ScanIndex,
    b: &'b Build,
    sb: &ScanIndex,
    class: Option<FourCc>,
) -> Comparison<'b> {
    let wanted = |t: &&Tag| class.is_none_or(|c| t.class == c);
    let mut out = Comparison { rows: Vec::new(), same: 0, not_compared: Vec::new() };
    // Each build's tags are in path order; the sort below keeps it within a status and class.
    for t in a.tags.iter().filter(wanted) {
        let Some(u) = b.tag_by_id(t.global_id) else {
            out.rows.push(Row { status: Status::OnlyInA, tag: t, other: None });
            continue;
        };
        match (print(a, sa, t), print(b, sb, u)) {
            (Some(p), Some(q)) if p == q => out.same += 1,
            (Some(_), Some(_)) => out.rows.push(Row { status: Status::Different, tag: t, other: Some(u) }),
            _ => out.not_compared.push(t),
        }
    }
    for u in b.tags.iter().filter(wanted).filter(|u| a.tag_by_id(u.global_id).is_none()) {
        out.rows.push(Row { status: Status::OnlyInB, tag: u, other: None });
    }
    out.rows.sort_by_key(|r| (r.status, r.tag.class));
    out
}

/// One tag (its copy `copy` in build `a`) against build `b` (D84): `Same` or
/// `Different` by tag data, as [`compare_builds`] compares, with `b`'s copy
/// read by default; `OnlyInA` if `b` has no tag with its global ID. Both
/// copies are read (from the tag cache when there), not the header scans:
/// the tag line doesn't wait for a scan.
pub fn tag_in_other_build(a: &Build, tag: &Tag, copy: Copy, b: &Build) -> Result<Status, ReadError> {
    let Some(other) = b.tag_by_id(tag.global_id).and_then(|u| choose_copy(b, u, None)) else {
        return Ok(Status::OnlyInA);
    };
    let (x, y) = (read_stored_tag(a, copy)?, read_stored_tag(b, other)?);
    Ok(if x.tag_data == y.tag_data { Status::Same } else { Status::Different })
}

#[cfg(test)]
#[path = "compare_tests.rs"]
mod tests;
