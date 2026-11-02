//! Presentation: domain results and errors as the text the agent reads.
//! Every marker comes from `output::marker`; every one is explained in
//! `docs/AGENT_GUIDE.md` (PRD §9a).

use h5_module::ModuleError;

use crate::catalog::{Build, ClassLookup, Tag};
use crate::filters::Mode;
use crate::output::{ToolError, format_size, marker};
use crate::query::ModuleStatus;
use crate::session::UnknownBuild;
use crate::stored::{DefinitionLoadError, ReadError};
use crate::view::{TagView, ViewNode};
use h5_meta::{Divergence, PathError, Problem, Section};

pub fn unknown_build(e: UnknownBuild) -> ToolError {
    ToolError::new(format!("unknown build '{}'", e.name)).hint(format!("builds: {}", e.available.join(", ")))
}

pub fn ambiguous_tag(build: &Build, text: &str, tags: &[&Tag]) -> ToolError {
    let list: Vec<String> = tags.iter().map(|t| format!("{} {}", t.global_id, build.display_name(t))).collect();
    ToolError::new(format!("'{text}' matches {} tags: {}", tags.len(), list.join("; ")))
        .hint("pass the global ID or the display name (path.class) to choose one")
}

/// Why no tag has class `query`, without claiming the game has none: only the
/// indexed modules were searched (decisions D18).
pub fn absent_class_hint(build: &Build, query: &str, lookup: &ClassLookup) -> String {
    match lookup {
        ClassLookup::DefinitionOnly(d) => format!(
            "no tags of class '{}' ({}) in the indexed modules of build '{}'. The class exists (it has a definition); its tags are in modules that aren't indexed. Don't say the game has none",
            d.file_code, d.class_name, build.name
        ),
        _ => format!(
            "no class '{query}' among the tags of build '{}' and no definition for it; check the spelling with list_classes",
            build.name
        ),
    }
}

/// An empty `list_entries(kind=...)` whose kind isn't known at all.
pub fn unknown_kind_hint(build: &Build, kind: &str) -> String {
    match build.lookup_class(kind) {
        lookup @ ClassLookup::DefinitionOnly(_) => absent_class_hint(build, kind, &lookup),
        _ => format!(
            "no entry kind '{kind}' in build '{}'; list_classes and list_resource_kinds list the kinds present",
            build.name
        ),
    }
}

pub fn module_status(status: &ModuleStatus) -> String {
    match status {
        ModuleStatus::Ok => "ok".into(),
        ModuleStatus::Incomplete { missing_bytes, entries_past_end } => format!(
            "{}: {} missing; {entries_past_end} entries are past the end and can't be read",
            marker::INCOMPLETE,
            format_size(*missing_bytes)
        ),
        ModuleStatus::Damaged { first, other_checks: 0 } => format!("{}: {first}", marker::DAMAGED),
        ModuleStatus::Damaged { first, other_checks } => {
            format!("{}: {first} (+{other_checks} other checks failed)", marker::DAMAGED)
        }
    }
}

pub fn not_indexed(error: &str) -> String {
    format!("{}: {error}", marker::NOT_INDEXED)
}

pub fn damaged_modules_hint(count: usize) -> String {
    format!(
        "{count} module(s) marked !! are damaged or incomplete: tags stored in them may fail to read. Tell the user; a fresh copy of the file usually fixes it"
    )
}

pub fn not_stored_hint() -> String {
    format!(
        "{}: the module describes this data (size shown) but doesn't contain it; where it is stored is unknown",
        marker::NOT_STORED
    )
}

/// The query matches tags, but the other filters (class, module) removed them all.
pub fn filtered_out_hint(name_only: usize) -> String {
    format!("no tag matches every filter; without class= and module=, the query matches {name_only} tags")
}

/// No tag matches the query at all; suggests close names when there are any.
pub fn no_match_hint(query: &str, mode: Mode, similar: &[String]) -> String {
    let what = match mode {
        Mode::Default => format!("no tag path contains '{query}'"),
        Mode::Regex => format!("no tag path matches the regex '{query}'"),
        Mode::Exact => format!("no tag path or display name is exactly '{query}'"),
    };
    if similar.is_empty() {
        format!("{what}; try a shorter query, or browse a folder")
    } else {
        format!("{what}; similar names in tag paths: {}", similar.join(", "))
    }
}

/// Modules named in the copies hint; more are counted.
const MAX_COPY_MODULES: usize = 5;

/// `modules`: every module holding a copy (one each), in copy order; `shown`:
/// the one used, named first.
pub fn other_copies_hint(modules: &[&str], shown: &str) -> String {
    let others = modules.iter().filter(|&&m| m != shown);
    let named: Vec<String> = std::iter::once(format!("{shown} (this)"))
        .chain(others.map(|m| (*m).to_owned()))
        .take(MAX_COPY_MODULES)
        .collect();
    let more = modules.len().saturating_sub(MAX_COPY_MODULES);
    let more = if more > 0 { format!(" (+{more} more)") } else { String::new() };
    format!("the tag has {} copies: {}{more}; use module= to choose another", modules.len(), named.join(", "))
}

/// A tag copy that can't be read from its module file.
pub fn read_error(e: ReadError) -> ToolError {
    let hint = match &e {
        ReadError::Open { .. } => "check the build's deploy folder; run reindex if modules were moved or removed",
        ReadError::Changed { .. } => "the file was replaced or updated: run reindex, then try again",
        ReadError::Module { error: ModuleError::NotStored { .. }, .. } => {
            "the module describes this tag but doesn't contain its data; where it is stored is unknown"
        }
        ReadError::Module { .. } => "the module may be damaged or incomplete; list_modules shows its status",
        ReadError::Header { .. } => "this tag's header doesn't have the documented layout",
    };
    ToolError::new(e.to_string()).hint(hint)
}

/// `which_maps` found the tag in no manifest; only indexed maps were looked at.
pub fn no_map_hint(checked: usize) -> String {
    match checked {
        0 => "no module in this build's indexed modules has a load manifest; maps that aren't indexed can't be checked"
            .into(),
        1 => "the load manifest in effect of 1 map doesn't list this tag; maps that aren't indexed weren't checked"
            .into(),
        n => format!(
            "the load manifests in effect of {n} maps don't list this tag; maps that aren't indexed weren't checked"
        ),
    }
}

/// Manifests `which_maps` couldn't read: the tag may be listed in them.
pub fn unread_manifests_hint(b: &Build, failed: &[(u32, ReadError)]) -> String {
    let named: Vec<String> =
        failed.iter().take(3).map(|(m, e)| format!("{} ({e})", b.index.modules[*m as usize].rel_path)).collect();
    format!("{} load manifest(s) couldn't be read; the tag may be in them too: {}", failed.len(), named.join("; "))
}

/// `map_tags`' module= found no load manifest, or one in several maps (D59).
pub fn pick_manifest_error(module: &str, e: crate::maps::PickError<'_>) -> ToolError {
    match e {
        crate::maps::PickError::NoManifest => {
            ToolError::new(format!("no module matching '{module}' has a load manifest"))
                .hint("list_modules shows which modules have one (column load manifest)")
        }
        crate::maps::PickError::SeveralMaps(stacks) => {
            ToolError::new(format!("'{module}' matches the manifests of several maps: {}", stacks.join(", ")))
                .hint("module= with one of these names reads that map's latest layer")
        }
    }
}

pub fn definition_error(e: DefinitionLoadError) -> ToolError {
    ToolError::new(e.to_string()).hint("the build's definitions folder (defs) may be damaged; check it in the config")
}

/// A tag whose class has no definition (PRD §6.4.5).
pub fn no_definition(class: &str) -> String {
    format!("{} '{class}'): its fields can't be read, only listed and followed by references", marker::NO_DEFINITION)
}

/// PRD §9 check 1 failed: the definition and the header's lists disagree.
pub fn divergences(found: &[Divergence]) -> Option<String> {
    let first = found.first()?;
    let place = match first {
        Divergence::NotInList { what, path } => format!("{what} at {path} is not in the tag header"),
        Divergence::Unreached { what, block, offset, .. } => {
            format!("the header lists a {what} at data block {block} +0x{offset:X} that no field reaches")
        }
    };
    Some(format!(
        "!! the definition doesn't match the tag header at {} place(s) (first: {place}): values may be misplaced",
        found.len()
    ))
}

pub fn main_struct_problem(p: &Problem) -> String {
    crate::tag_view::problem_text(p, "(main struct)")
}

pub fn path_error(e: PathError) -> ToolError {
    let level = |at: &str| if at.is_empty() { "the main struct".to_owned() } else { format!("'{at}'") };
    match e {
        PathError::NoSuchField { at, name, similar } if similar.is_empty() => {
            ToolError::new(format!("no field '{name}' in {}", level(&at))).hint("outline lists the fields")
        }
        PathError::NoSuchField { at, name, similar } => ToolError::new(format!("no field '{name}' in {}", level(&at)))
            .hint(format!("closest names there: {}", similar.join(", "))),
        PathError::NotABlock { at } => {
            ToolError::new(format!("'{at}' is not a tag block")).hint("[i] and [*] only follow tag block fields")
        }
        PathError::NoFields { at } => {
            ToolError::new(format!("'{at}' holds a value, not fields")).hint("end the path at that field")
        }
        PathError::OutOfRange { at, index, count } => {
            ToolError::new(format!("'{at}' has {count} entries; [{index}] doesn't exist")).hint(match count {
                0 => "the block is empty".to_owned(),
                n => format!("use [0] to [{}], or [*] for every entry", n - 1),
            })
        }
        PathError::Empty { at } => {
            ToolError::new(format!("'{at}' has no entries (empty)")).hint("the block is empty in this tag")
        }
        PathError::NotRead { at } => ToolError::new(format!("!! '{at}' wasn't read"))
            .hint("a problem earlier in the tag stopped the read; read_tag without path= shows it"),
    }
}

/// `section=` found none, or several (PRD §6.4.3: list them instead of reading).
pub fn section_error(heading: &str, found: &[Section], fields: &[h5_defs::definition::Field]) -> ToolError {
    if found.is_empty() {
        return ToolError::new(format!("no section '{heading}'")).hint("outline lists the headings");
    }
    let places: Vec<String> = found
        .iter()
        .map(|s| match s.parent.display(fields) {
            p if p.is_empty() => "the main struct".to_owned(),
            p => p,
        })
        .collect();
    ToolError::new(format!("'{heading}' heads {} sections, in: {}", found.len(), places.join("; ")))
        .hint("read one with path= (the struct it is in)")
}

/// The outline levels a guarded read returns (decisions D41): about 1,000
/// tokens for a weapon, where depth 2 is close to the default limit itself.
pub const GUARD_OUTLINE_DEPTH: u32 = 1;

/// The size guard on whole reads (PRD §6.4.1, decisions D41): `view` as it is
/// when its text is within `limit` tokens (characters ÷ 4), or with a warning
/// first when `confirm`ed; otherwise `outline()` in its place, with a hint
/// giving the whole read's size and how to go on.
pub fn size_guard(mut view: TagView, limit: usize, confirm: bool, outline: impl FnOnce() -> Vec<ViewNode>) -> TagView {
    let text = view.render_lines(false);
    let tokens = text.len() / 4;
    if tokens <= limit {
        return view;
    }
    let size = format!("≈ {tokens} tokens ({} lines)", text.lines().count());
    if confirm {
        view.title_lines.insert(0, format!("warning: large result, {size}"));
        return view;
    }
    view.nodes = outline();
    view.legend.clear();
    view.brief = false;
    view.hints.push(format!("whole read {size}; read a path= or section=, or repeat with confirm_large=true"));
    view
}

/// Classes listed in a `by class` summary; the rest are counted as `+N more`.
const CLASS_BREAKDOWN_MAX: usize = 8;

/// `by class: weap 3, proj 2, …` over a list's rows (most first), when they
/// span more than one class: narrow with `class=` in one step.
pub fn by_class(counts: Vec<(h5_module::FourCc, usize)>) -> Option<crate::output::Summary> {
    if counts.len() < 2 {
        return None;
    }
    let counts = counts.into_iter().map(|(code, n)| (code.to_string(), n)).collect();
    Some(crate::output::Summary {
        label: "by class",
        key: "by_class",
        value: crate::output::Cell::counts(counts, CLASS_BREAKDOWN_MAX),
    })
}

/// An entry's compressed size, or that its data isn't in this module.
pub fn compressed_size(e: &h5_module::index::EntryRecord) -> crate::output::Cell {
    if e.stored { crate::output::Cell::Size(e.compressed_size as u64) } else { marker::NOT_STORED.into() }
}

/// A module's layer as its name gives it: `base`, or `<version>-<revision>`.
pub fn layer_text(layer: h5_module::names::Layer) -> String {
    if layer.version == 0 { "base".into() } else { format!("{}-{}", layer.version, layer.revision) }
}

/// `<what> built: N tags, T s`, on the call that had to build it.
fn built(label: &'static str, key: &'static str, tags: usize, elapsed: std::time::Duration) -> crate::output::Summary {
    let value = format!("{tags} tags, {:.2} s", elapsed.as_secs_f64());
    crate::output::Summary { label, key, value: value.into() }
}

/// `header scan built: N tags, T s`, on the call that had to scan (PRD §6.2).
pub fn scan_built(b: Option<crate::references::ScanBuilt>) -> Option<crate::output::Summary> {
    b.map(|b| built("header scan built", "header_scan_built", b.tags, b.elapsed))
}

/// `definition check built: N tags, T s`, on the call that had to check (PRD §6.9).
pub fn check_built(b: Option<crate::def_check::CheckBuilt>) -> Option<crate::output::Summary> {
    b.map(|b| built("definition check built", "definition_check_built", b.tags, b.elapsed))
}

/// A gap of `definition_status(class=)`, as its `gap` column says it.
pub fn gap_text(kind: &crate::def_check::GapKind) -> String {
    use crate::def_check::GapKind;
    match kind {
        GapKind::Unreadable(why) => format!("unreadable: {why}"),
        GapKind::Size => "size".into(),
        GapKind::Guid { known: true, .. } => "GUID".into(),
        GapKind::Guid { known: false, .. } => "GUID (in no definition)".into(),
        GapKind::MainStruct(_) => "main struct".into(),
    }
}

/// Modules the definition check couldn't read: their tags aren't counted.
pub fn unchecked_modules_hint(failed: &[(String, String)]) -> String {
    let named: Vec<String> = failed.iter().take(3).map(|(m, e)| format!("{m} ({e})")).collect();
    format!("{} module(s) couldn't be checked; their tags aren't counted: {}", failed.len(), named.join("; "))
}

/// `query_field` errors, with the next step where there is one.
pub fn query_error(e: crate::field_query::QueryError) -> ToolError {
    use crate::field_query::QueryError;
    let hint = match &e {
        QueryError::NotAValue { field, .. } => {
            Some(format!("query one of its fields: {field}[*].<field> (outline or get_definition lists them)"))
        }
        QueryError::Unsupported { .. } => Some(
            "numbers: == != > >= < <=; enums: == != by label; flags: contains <option>; text: == != contains".into(),
        ),
        QueryError::NotANumber { .. } => None,
    };
    let err = ToolError::new(e.to_string());
    match hint {
        Some(h) => err.hint(h),
        None => err,
    }
}

/// `N tags skipped: <name> (<why>); …` for tags a query couldn't read.
pub fn skipped_hint(build: &Build, skipped: &[crate::field_query::Skipped<'_>]) -> Option<String> {
    use crate::field_query::SkipReason;
    const SHOWN: usize = 3;
    if skipped.is_empty() {
        return None;
    }
    let shown: Vec<String> = skipped
        .iter()
        .take(SHOWN)
        .map(|s| {
            let why = match &s.reason {
                SkipReason::Read(e) => e.to_string(),
                SkipReason::Path(e) => path_error(e.clone()).message,
            };
            format!("{} ({why})", build.display_name(s.tag))
        })
        .collect();
    let n = skipped.len();
    let more = if n > SHOWN { format!("; +{} more", n - SHOWN) } else { String::new() };
    let tags = if n == 1 { "1 tag skipped".to_owned() } else { format!("{n} tags skipped") };
    Some(format!("{tags}: {}{more}", shown.join("; ")))
}
