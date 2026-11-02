//! Every M1 tool against a real deploy folder. Ignored by default:
//!
//! ```text
//! H5_DEPLOY=<folder with .module files> cargo test -p h5tags-core -- --ignored --nocapture
//! ```
//!
//! Definitions default to `reference/H5Tags/tags`; set `H5_DEFS` to override.

mod common;

use std::time::Instant;

use h5_module::testutil::TempDir;
use h5tags_core::filters::{Mode, Size};
use h5tags_core::query::EntrySort;
use h5tags_core::tools::args::{BuildArg, PageArgs};
use h5tags_core::tools::{
    Browse, ListClasses, ListEntries, ListModules, ListResourceKinds, Reindex, SearchTags, Tool, What,
};
use h5tags_core::{Format, Session};

/// A session on `H5_DEPLOY`, with its index cache in a folder deleted afterwards.
fn session() -> (Session, TempDir) {
    let start = Instant::now();
    let opened = common::real_session("core-real");
    println!("session opened in {:.2?}", start.elapsed());
    opened
}

fn check(s: &mut Session, name: &str, tool: &impl Tool) {
    let start = Instant::now();
    let out = tool.run(s).unwrap_or_else(|e| panic!("{name}: {e} ({:?})", e.hint));
    let text = out.render(Format::Text);
    // Both renderings must work; JSON must parse.
    serde_json::from_str::<serde_json::Value>(&out.render(Format::Json)).unwrap();
    println!("--- {name} ({:.2?})\n{}", start.elapsed(), text.lines().take(8).collect::<Vec<_>>().join("\n"));
}

fn page(limit: usize) -> PageArgs {
    PageArgs { offset: 0, limit: Some(limit) }
}

#[test]
#[ignore]
fn every_m1_tool_runs() {
    let (mut s, _cache) = session();
    let b = &s.builds()[0];
    let classes_without_definition: Vec<String> =
        b.classes.values().filter(|c| c.definition.is_none()).map(|c| format!("{} ({})", c.code, c.name)).collect();
    println!(
        "{} tags, {} classes; classes without a definition: {classes_without_definition:?}",
        b.tags.len(),
        b.classes.len()
    );

    let build = BuildArg::default;
    let search = |query: &str| SearchTags {
        query: query.into(),
        class: None,
        module: None,
        mode: Mode::Default,
        build: build(),
        page: page(20),
    };
    check(&mut s, "search_tags", &search("rifle"));
    check(&mut s, "search_tags regex", &SearchTags { mode: Mode::Regex, ..search(r"magnum|pistol") });
    check(&mut s, "browse top", &Browse { path: String::new(), build: build(), page: page(50) });
    check(&mut s, "list_classes", &ListClasses { build: build() });
    check(&mut s, "list_modules", &ListModules { build: build(), page: page(50) });
    check(&mut s, "list_resource_kinds", &ListResourceKinds { build: build() });
    let entries = ListEntries {
        tag: None,
        query: None,
        mode: Mode::Default,
        kind: None,
        module: None,
        min_size: Some(Size(1_000_000)),
        max_size: None,
        sort: EntrySort::Size,
        build: build(),
        page: page(10),
    };
    check(&mut s, "list_entries biggest", &entries);
    // Sampled tags resolve by global ID and list their resources.
    let ids: Vec<String> = s.builds()[0].tags.iter().step_by(97).map(|t| t.global_id.to_string()).collect();
    for id in &ids {
        ListEntries { tag: Some(id.clone()), min_size: None, ..entries.clone() }.run(&mut s).unwrap();
    }
    println!("--- list_entries(tag) on {} sampled tags: ok", ids.len());
    check(&mut s, "reindex index", &Reindex { what: What::Index, module: None, class: None, build: build() });
    std::fs::remove_dir_all(s.cache_dir()).unwrap();
}
