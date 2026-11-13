//! Presentation: domain results and errors as the text the agent reads.
//! Every marker comes from `output::marker`; every one is explained in
//! `docs/AGENT_GUIDE.md` (PRD §9a).

use h5_module::{FourCc, ModuleError};

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
/// `read_script`: a script that couldn't be read (D85).
pub fn script_error(e: crate::script::ScriptError) -> ToolError {
    use crate::script::ScriptError;
    match e {
        ScriptError::NotAScript => ToolError::new(e.to_string())
            .hint("search_tags(class='hsc*') or search_tags(class='luas') lists the script tags"),
        ScriptError::Read(r) => read_error(r),
        ScriptError::Definition(d) => definition_error(d),
        ScriptError::NoDefinition(class) => ToolError::new(no_definition(&class)),
        ScriptError::Field { .. } => {
            ToolError::new(e.to_string()).hint("definition_status(class=…) lists the class's gaps")
        }
    }
}

/// `diff_tags` on two script tags with changed lines (D89).
pub const SCRIPT_DIFF_HINT: &str = "changed lines only; read_script(tag, offset=<line - 1>) shows the lines around them; \
     path= compares the tags' fields instead";

/// `diff_tags` on two script tags whose scripts are equal.
pub const SCRIPTS_SAME_HINT: &str = "the scripts are the same; path= compares the tags' other fields";

/// `diff_tags` on two scripts whose changed middle is too long to match.
pub fn script_diff_too_large(e: crate::script::TooLarge) -> ToolError {
    ToolError::new(format!("the scripts differ in too much to compare: {e}"))
        .hint("read_script each tag, or search_scripts for the part you need")
}

/// `search_scripts`: a class given that holds no scripts.
pub fn not_a_script_class(class: FourCc) -> ToolError {
    ToolError::new(format!(
        "{} isn't a script class: scripts are in hsc* (lua) and luas (lua_script) tags",
        class.to_string().trim_end()
    ))
}

/// `search_scripts`: no line or string holds the term.
pub fn no_script_holds(term: &str) -> String {
    format!(
        "no script holds '{term}'; compiled (luas) scripts are searched by the strings they hold, not their code; \
         names built at runtime aren't seen"
    )
}

/// `search_scripts`, `lookup_text`: tags (`what`: script, unic) that couldn't
/// be read, so weren't searched.
pub fn skipped_tags(what: &str, skipped: &[(&Tag, impl std::fmt::Display)]) -> Option<String> {
    let (first, why) = skipped.first()?;
    Some(format!("{} {what} tags couldn't be read and weren't searched, e.g. {} ({why})", skipped.len(), first.path))
}

pub fn text_error(e: crate::text::TextError) -> ToolError {
    use crate::text::TextError;
    match e {
        TextError::NotText => {
            ToolError::new(e.to_string()).hint("search_tags(class='unic') lists them; omit tag= to search them all")
        }
        TextError::Read(r) => read_error(r),
        TextError::Definition(d) => definition_error(d),
        TextError::NoDefinition => ToolError::new(no_definition("unic")),
        TextError::Field { .. } => {
            ToolError::new(e.to_string()).hint("definition_status(class='unic') lists the class's gaps")
        }
    }
}

/// `lookup_text`: a `lang=` the definition doesn't have.
pub fn unknown_language(lang: &str, languages: &[String]) -> ToolError {
    ToolError::new(format!("no language '{lang}'")).hint(format!("languages: {}, or all", languages.join(", ")))
}

/// `lookup_text`: string_ids the tables give different texts (D92): each
/// (name, language, texts), the first three named.
pub fn differing_texts(differ: &[(String, &str, usize)]) -> Option<String> {
    if differ.is_empty() {
        return None;
    }
    let named: Vec<String> = differ.iter().take(3).map(|(id, l, n)| format!("{id} ({n} {l} texts)")).collect();
    let more = if differ.len() > 3 { format!(" +{} more", differ.len() - 3) } else { String::new() };
    Some(format!(
        "string_ids with different texts in different unic tags: {}{more}; \
         the files don't say which table a given tag's text comes from",
        named.join(", ")
    ))
}

/// `lookup_text`: hits from tags with copies in other layers.
pub fn older_text_copies(tags: usize) -> String {
    format!(
        "unic tags listed with copies in other layers (they differ): {tags}; tag_copies lists them; \
         lookup_text(tag=, module=) reads one"
    )
}

/// `lookup_text`: nothing found.
pub fn no_text(string_id: Option<&str>, term: Option<&str>, lang: &str) -> String {
    match (string_id, term) {
        (Some(id), _) => format!(
            "no unic tag has string_id '{id}' (by name, ignoring case); search_values finds the tags using a name"
        ),
        (_, Some(t)) if lang == "all" => format!("no text in any language holds '{t}'"),
        (_, Some(t)) => format!("no {lang} text holds '{t}'; lang=all searches every language"),
        _ => "this unic tag holds no texts".into(),
    }
}

/// `lookup_text`: texts that couldn't be read (offset outside the data, no
/// end, not UTF-8).
pub fn unreadable_texts(n: usize) -> String {
    format!(
        "{n} texts couldn't be read (stored offset or bytes not as expected) and show (not read) or weren't matched"
    )
}

/// `read_script`: under the tag line, the tags the script names by path.
pub fn tag_paths_line(paths: usize, strings: usize, indexed: usize) -> String {
    format!(
        "tag paths: {paths} ({strings} strings; {indexed} indexed, {} not in indexed modules); names=true lists them",
        paths - indexed
    )
}

/// `read_script`: under the tag line of source that isn't UTF-8.
pub const NOT_UTF8_LINE: &str = "encoding: not UTF-8 (other bytes shown as \u{fffd})";

/// `read_script` on compiled code: what its rows are, and what they aren't.
pub const COMPILED_HINT: &str = "compiled Havok Script: these are the strings it holds, in order (names, text, \
     paths of other scripts and tags); its code isn't decompiled, so what it does with them isn't shown";

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

/// `diff_tags` with differences: method step 2 (AGENT_GUIDE §2.1), where the
/// agent is looking (D64).
pub const CANDIDATES_HINT: &str =
    "these are candidates, not causes: check one across the class with query_field before naming it the cause";

/// `diff_tags` across builds whose definitions of the class differ (D83).
pub fn definitions_differ_hint(class: &str) -> String {
    format!(
        "the two builds' definitions of {class} differ: fields are matched by name; \
         {} = that build's definition has no field of that name",
        marker::NO_FIELD
    )
}

/// `read_tag` and `diff_tags`: path= and section= both given.
pub fn path_and_section() -> ToolError {
    ToolError::new("path= and section= can't be used together").hint("pass one of them")
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

pub fn main_struct_problem(p: &Problem, class: FourCc) -> String {
    let text = crate::tag_view::problem_text(p, "(main struct)");
    match p {
        // D68 step 2: where to look next.
        Problem::SizeMismatch { .. } => format!("{text}; definition_status(class={class}) lists the class's gaps"),
        _ => text,
    }
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
        // `at`: the struct holding the field, where the read stopped.
        PathError::NotRead { at } => {
            ToolError::new(format!("!! the field wasn't read: the read of {} stopped before it", level(&at)))
                .hint("a problem earlier in the tag stopped the read; read_tag without path= shows it")
        }
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
/// About how many tokens a view's text is (4 bytes each), and its text.
pub fn estimated_tokens(view: &TagView) -> (usize, String) {
    let text = view.render_lines(false);
    (text.len() / 4, text)
}

pub fn size_guard(mut view: TagView, limit: usize, confirm: bool, outline: impl FnOnce() -> Vec<ViewNode>) -> TagView {
    let (tokens, text) = estimated_tokens(&view);
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

/// Module formats with what they're known as: `23 (xbox), 27 (forge)`.
pub fn format_names(versions: impl IntoIterator<Item = u32>) -> String {
    let mut seen: Vec<u32> = versions.into_iter().collect();
    seen.sort_unstable();
    seen.dedup();
    let names: Vec<String> = seen
        .iter()
        .map(|&v| crate::identify::format_label(v).map_or_else(|| v.to_string(), |l| format!("{v} ({l})")))
        .collect();
    names.join(", ")
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

/// `compare_builds`: each build's header scan, on the call that had to build it (D82).
pub fn scans_built(
    a: Option<crate::references::ScanBuilt>,
    b: Option<crate::references::ScanBuilt>,
) -> Vec<crate::output::Summary> {
    let mut out = Vec::new();
    out.extend(a.map(|x| built("a's header scan built", "header_scan_built_a", x.tags, x.elapsed)));
    out.extend(b.map(|x| built("b's header scan built", "header_scan_built_b", x.tags, x.elapsed)));
    out
}

/// `compare_builds`: shared tags whose copy a scan couldn't read (D82).
pub fn not_compared_hint(n: usize, example: &str) -> String {
    let tags = if n == 1 { "1 tag".to_owned() } else { format!("{n} tags") };
    format!(
        "{tags} in both builds couldn't be compared: a header scan couldn't read its copy (e.g. {example}); find_references says why"
    )
}

/// `definition check built: N tags, T s`, on the call that had to check (PRD §6.9).
pub fn check_built(b: Option<crate::def_check::CheckBuilt>) -> Option<crate::output::Summary> {
    b.map(|b| built("definition check built", "definition_check_built", b.tags, b.elapsed))
}

/// A gap of `definition_status(class=)`, as its `gap` column says it.
pub fn gap_text(gap: &crate::def_check::Gap) -> String {
    use crate::def_check::{GapKind, OtherKind};
    let text = match &gap.kind {
        GapKind::Unreadable(why) => format!("unreadable: {why}"),
        GapKind::Size => "size".into(),
        GapKind::Guid { known: true, .. } => "GUID".into(),
        GapKind::Guid { known: false, .. } => "GUID (in no definition)".into(),
        GapKind::Other(OtherKind::UnknownType(t)) => format!("other: unknown field type {t}"),
        GapKind::Other(OtherKind::UnknownSize) => "other: field size unknown".into(),
        GapKind::Other(OtherKind::NoDataBlock) => "other: tag block with entries but no data block".into(),
        GapKind::Other(OtherKind::OutOfBounds) => "other: data past the end of its data block".into(),
        GapKind::Other(OtherKind::NotInList(what)) => format!("other: {what} not in the tag header's lists"),
        GapKind::Other(OtherKind::Unreached(what)) => format!("other: {what} in the tag header reached by no field"),
    };
    // D80: no copy read by default has it: only older layers do.
    if gap.default_copies == 0 { format!("{text} (older layers only)") } else { text }
}

/// `definition_status(class=)`'s `layouts:` line: each main struct GUID and its copies (D80).
pub fn layouts_text(layouts: &[([u8; 16], usize)]) -> String {
    let each: Vec<String> = layouts
        .iter()
        .map(|(g, n)| {
            format!("{} ({n} {})", h5_defs::definition::guid_text(g), if *n == 1 { "copy" } else { "copies" })
        })
        .collect();
    each.join(", ")
}

/// A `query_field` `also` path that reaches no value in a row's tag (D75):
/// what stopped it, as a marker.
pub fn missing_value(e: &PathError) -> &'static str {
    match e {
        PathError::Empty { .. } => marker::EMPTY,
        PathError::OutOfRange { .. } => marker::NO_ENTRY,
        _ => marker::NOT_READ,
    }
}

/// D77: a `path=` read of a block of one-field entries shows every entry's field.
pub fn every_entry_note(field: &str, path: &str) -> String {
    format!("every entry's {field} (as path={path})")
}

/// D77: the same, over the size limit: entry 0 is shown, and this says where the rest is.
pub fn every_entry_hint(field: &str, path: &str, tokens: usize) -> String {
    format!("every entry's {field}: path={path} (≈ {tokens} tokens; over the size limit, so entry 0 is shown)")
}

/// `which_maps(module=)` kept no map (D76).
pub fn no_map_matches(module: &str) -> String {
    format!("no map's module path contains '{module}'; list_modules lists the modules with a load manifest")
}

/// `list_classes(query=)` found nothing.
pub fn no_class_matches(query: &str) -> String {
    format!("no class code or name contains '{query}'; list_classes without query lists every class")
}

/// Modules the definition check couldn't read: their tags aren't counted.
pub fn unchecked_modules_hint(failed: &[(String, String)]) -> String {
    let named: Vec<String> = failed.iter().take(3).map(|(m, e)| format!("{m} ({e})")).collect();
    format!("{} module(s) couldn't be checked; their tags aren't counted: {}", failed.len(), named.join("; "))
}

/// Options named in a `query_field` label error; more are counted.
const MAX_OPTIONS: usize = 12;

/// `query_field` errors, with the next step where there is one.
pub fn query_error(e: crate::field_query::QueryError) -> ToolError {
    use crate::field_query::QueryError;
    let hint = match &e {
        // A struct, array, curve or tag block entry (D75: a block without an index is a value).
        QueryError::NotAValue { field, .. } if field.is_empty() => {
            Some("query one field: its path (outline or get_definition lists them)".into())
        }
        QueryError::NotAValue { field, .. } => {
            Some(format!("query one of its fields: {field}.<field> (outline or get_definition lists them)"))
        }
        QueryError::AlsoNotAligned { example, .. } => {
            Some(format!("give it an index ({example}), or put the same [*] in path"))
        }
        QueryError::Unsupported { kind, .. } => Some(format!("for {}: {}", kind.name(), kind.operators())),
        QueryError::NotANumber { .. } => None,
        QueryError::NotAnOption { options, .. } if options.is_empty() => {
            Some("its definition names no options: compare it as a number (e.g. '== 2')".into())
        }
        QueryError::NotAnOption { options, .. } => {
            let more = options.len().saturating_sub(MAX_OPTIONS);
            let more = if more > 0 { format!(" (+{more} more)") } else { String::new() };
            Some(format!("options: {}{more}", options[..options.len().min(MAX_OPTIONS)].join(", ")))
        }
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
