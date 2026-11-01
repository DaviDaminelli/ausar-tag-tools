//! Presentation: domain results and errors as the text the agent reads.
//! Every marker comes from `output::marker`; every one is explained in
//! `docs/AGENT_GUIDE.md` (PRD §9a).

use crate::catalog::{Build, ClassLookup, Tag};
use crate::output::{ToolError, format_size, marker};
use crate::query::ModuleStatus;
use crate::session::UnknownBuild;

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

pub fn other_copies_hint(copies: usize, module: &str) -> String {
    format!("the tag has {copies} copies; listing the copy in {module}; use module= to choose another")
}
