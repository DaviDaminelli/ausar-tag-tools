//! References between tags, both ways, from the header scan (PRD §6.2, §6.6):
//! the scan is loaded or built when a tool first needs it, kept with its build,
//! and cached next to the index. Domain only.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use h5_module::index::Index;
use h5_module::scan::{HeaderScan, OutReference, TagScan};
use h5_module::text::decode_cp1252;
use h5_module::{FourCc, GlobalId};

use crate::catalog::{Build, Copy, Tag};
use crate::filters::ascii_matches;
use crate::query::choose_copy;
use crate::stored::{DefinitionLoadError, StoredTag, load_definition};
use crate::tag_view::join;

/// A build's header scan: where it is cached, and the scan once loaded or built.
#[derive(Default)]
pub struct ScanSlot {
    pub(crate) cache: Option<PathBuf>,
    index: OnceLock<ScanIndex>,
}

/// The header scan with its lookups.
pub struct ScanIndex {
    pub scan: HeaderScan,
    /// Tag copy → (module scan, tag scan).
    copies: HashMap<Copy, (u32, u32)>,
    /// Target → (module scan, tag scan, reference) of each field pointing to it.
    incoming: HashMap<GlobalId, Vec<(u32, u32, u32)>>,
    /// Module scan → the module's position in the index.
    modules: Vec<u32>,
}

/// What building the scan took, when a call had to (PRD §6.2: the result says so).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanBuilt {
    /// Tag copies read in all.
    pub tags: usize,
    pub elapsed: Duration,
}

impl ScanIndex {
    fn new(scan: HeaderScan, index: &Index) -> ScanIndex {
        let position: HashMap<&str, u32> =
            index.modules.iter().enumerate().map(|(i, m)| (m.rel_path.as_str(), i as u32)).collect();
        let mut copies = HashMap::new();
        let mut incoming: HashMap<GlobalId, Vec<(u32, u32, u32)>> = HashMap::new();
        let mut modules = Vec::with_capacity(scan.modules.len());
        for (mi, m) in scan.modules.iter().enumerate() {
            let module = position.get(m.rel_path.as_str()).copied().unwrap_or(u32::MAX);
            modules.push(module);
            for (ti, t) in m.tags.iter().enumerate() {
                copies.insert(Copy { module, entry: t.entry }, (mi as u32, ti as u32));
                for (ri, r) in t.references.iter().enumerate() {
                    incoming.entry(r.target).or_default().push((mi as u32, ti as u32, ri as u32));
                }
            }
        }
        ScanIndex { scan, copies, incoming, modules }
    }

    /// What tag copy `copy` references; `None` if it couldn't be read ([`ScanIndex::problems`]).
    pub fn outgoing(&self, copy: Copy) -> Option<&TagScan> {
        let &(m, t) = self.copies.get(&copy)?;
        Some(&self.scan.modules[m as usize].tags[t as usize])
    }

    /// Every reference field pointing to `target`, with the tag copy holding it.
    pub fn incoming(&self, target: GlobalId) -> impl Iterator<Item = (Copy, &OutReference)> {
        self.incoming.get(&target).into_iter().flatten().map(|&(m, t, r)| {
            let tag = &self.scan.modules[m as usize].tags[t as usize];
            (Copy { module: self.modules[m as usize], entry: tag.entry }, &tag.references[r as usize])
        })
    }

    /// Tag copies that couldn't be read: (module path, global ID, reason).
    pub fn problems(&self) -> impl Iterator<Item = (&str, GlobalId, &str)> {
        self.scan
            .modules
            .iter()
            .flat_map(|m| m.problems.iter().map(|(id, why)| (m.rel_path.as_str(), *id, why.as_str())))
    }
}

/// A tag that references a target: in how many of its copies, and with how
/// many fields (in the copy with the most).
pub struct Referrer<'b> {
    pub tag: &'b Tag,
    pub copies: usize,
    pub fields: usize,
}

/// The tags whose copies reference `target`, by class, then path.
pub fn referrers<'b>(b: &'b Build, scan: &ScanIndex, target: GlobalId) -> Vec<Referrer<'b>> {
    let mut fields: HashMap<Copy, usize> = HashMap::new();
    for (copy, _) in scan.incoming(target) {
        *fields.entry(copy).or_default() += 1;
    }
    let mut by_tag: HashMap<GlobalId, Referrer<'b>> = HashMap::new();
    for (copy, n) in fields {
        let Some(tag) = b.tag_by_id(b.entry(copy).global_id) else { continue };
        let r = by_tag.entry(tag.global_id).or_insert(Referrer { tag, copies: 0, fields: 0 });
        r.copies += 1;
        r.fields = r.fields.max(n);
    }
    let mut out: Vec<Referrer<'b>> = by_tag.into_values().collect();
    out.sort_by(|a, b| (a.tag.class, &a.tag.path_lower).cmp(&(b.tag.class, &b.tag.path_lower)));
    out
}

/// One step of an outgoing walk: a reference `depth` levels from the start,
/// held by `via`. `tag` is `None` when the target isn't indexed; `repeat`
/// when it was reached before (not expanded again).
pub struct Step<'b> {
    pub depth: u32,
    pub target: GlobalId,
    pub class: FourCc,
    pub tag: Option<&'b Tag>,
    /// The name `via` stores for it (for targets that aren't indexed).
    pub listed_name: String,
    pub via: &'b Tag,
    pub repeat: bool,
}

/// Every reference reachable from `start` within `max_depth` levels, in tree
/// order (each tag's references in its reference list's order, depth first),
/// following each tag's default copy (PRD §6.4.6). A tag already reached is
/// listed again as a repeat and not expanded.
pub fn walk_out<'b>(b: &'b Build, scan: &ScanIndex, start: &'b Tag, max_depth: u32) -> Vec<Step<'b>> {
    fn visit<'b>(
        b: &'b Build,
        scan: &ScanIndex,
        from: &'b Tag,
        depth: u32,
        max_depth: u32,
        seen: &mut std::collections::HashSet<GlobalId>,
        out: &mut Vec<Step<'b>>,
    ) {
        let Some(t) = choose_copy(b, from, None).and_then(|c| scan.outgoing(c)) else { return };
        for r in &t.references {
            let tag = b.tag_by_id(r.target);
            let repeat = !seen.insert(r.target);
            let listed_name =
                if tag.is_none() { t.listed_name(r).unwrap_or_default().into_owned() } else { String::new() };
            out.push(Step { depth, target: r.target, class: r.class, tag, listed_name, via: from, repeat });
            if let (Some(next), false, true) = (tag, repeat, depth < max_depth) {
                visit(b, scan, next, depth + 1, max_depth, seen, out);
            }
        }
    }
    let mut seen = std::collections::HashSet::from([start.global_id]);
    let mut out = Vec::new();
    visit(b, scan, start, 1, max_depth, &mut seen, &mut out);
    out
}

/// A tag whose string table holds a query, with the strings that matched
/// (distinct, sorted; from every copy).
pub struct StringHit<'b> {
    pub tag: &'b Tag,
    pub strings: Vec<String>,
}

/// `search_values` (PRD §6.6): tags whose string table (string_id names and
/// referenced tag names) contains `q`, ignoring ASCII case; with `class`, only
/// tags of that class. By class, then path. Each table is walked in place;
/// only matching strings are decoded (D26).
pub fn search_strings<'b>(b: &'b Build, scan: &ScanIndex, q: &str, class: Option<FourCc>) -> Vec<StringHit<'b>> {
    let mut by_tag: HashMap<GlobalId, std::collections::BTreeSet<String>> = HashMap::new();
    for t in scan.scan.modules.iter().flat_map(|m| &m.tags) {
        if class.is_some_and(|c| b.tag_by_id(t.global_id).is_none_or(|tag| tag.class != c)) {
            continue;
        }
        for s in t.strings.split(|&c| c == 0) {
            if ascii_matches(s, q.as_bytes()).next().is_some() {
                by_tag.entry(t.global_id).or_default().insert(decode_cp1252(s).into_owned());
            }
        }
    }
    let mut hits: Vec<StringHit<'b>> = by_tag
        .into_iter()
        .filter_map(|(id, strings)| Some(StringHit { tag: b.tag_by_id(id)?, strings: strings.into_iter().collect() }))
        .collect();
    hits.sort_by(|a, b| (a.tag.class, &a.tag.path_lower).cmp(&(b.tag.class, &b.tag.path_lower)));
    hits
}

/// (data block, offset) of a tag reference field → its field path.
pub type Places = HashMap<(u32, u32), String>;

/// Where each tag reference of `t` is, by field path (decisions D29), from
/// reading the tag with its class's definition. `None` without a definition.
pub fn reference_paths(b: &Build, tag: &Tag, t: &StoredTag) -> Result<Option<Places>, DefinitionLoadError> {
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
    let Some(def) = load_definition(b, tag)? else { return Ok(None) };
    let read = h5_meta::read_tag(&def, &t.header, &t.tag_data);
    let mut out = HashMap::new();
    collect(&read.root, "", &mut out);
    Ok(Some(out))
}

/// The header scan of `b`: loaded (and brought up to date) or built on the
/// first call, then kept; `ScanBuilt` when this call had to scan modules.
pub fn header_scan(b: &Build) -> (&ScanIndex, Option<ScanBuilt>) {
    let mut built = None;
    let index = b.scan.index.get_or_init(|| {
        let previous = b.scan.cache.as_deref().and_then(HeaderScan::load);
        let never = |_: &str| false;
        let (scan, report) = HeaderScan::update(&b.config.deploy, &b.index, previous, &never, &|_| {});
        if report.read > 0 || report.removed > 0 {
            let tags = scan.modules.iter().map(|m| m.tags.len() + m.problems.len()).sum();
            built = Some(ScanBuilt { tags, elapsed: report.elapsed });
            if let Some(path) = &b.scan.cache
                && let Err(e) = scan.save(path)
            {
                tracing::warn!(cache = %path.display(), error = %e, "couldn't save the header scan");
            }
        }
        tracing::info!(build = %b.name, ?report, "header scan ready");
        ScanIndex::new(scan, &b.index)
    });
    (index, built)
}

/// `reindex(what=header_scan)` (PRD §6.2): drops the scan, and the cached
/// scans of the modules whose path matches `force`, so the next call that
/// needs it scans them again. Returns how many modules were dropped.
pub fn forget_header_scan(b: &mut Build, force: &dyn Fn(&str) -> bool) -> usize {
    b.scan.index = OnceLock::new();
    let Some(path) = b.scan.cache.clone() else { return 0 };
    let Some(mut cached) = HeaderScan::load(&path) else { return 0 };
    let before = cached.modules.len();
    cached.modules.retain(|m| !force(&m.rel_path));
    let dropped = before - cached.modules.len();
    if let Err(e) = cached.save(&path) {
        tracing::warn!(cache = %path.display(), error = %e, "couldn't save the header scan");
    }
    dropped
}

#[cfg(test)]
#[path = "references_tests.rs"]
mod tests;
