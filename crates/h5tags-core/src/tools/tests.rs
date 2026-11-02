use std::path::PathBuf;

use h5_defs::{DefinitionFile, Definitions};
use h5_module::index::{EntryRecord, FailedModule, FileStamp, Index, ModuleIssue, ModuleRecord};
use h5_module::{FourCc, GlobalId, checks};
use serde_json::Value;

use super::args::{self, BuildArg, PageArgs};
use super::*;
use crate::catalog::Build;
use crate::config::BuildConfig;
use crate::filters::{Mode, Size};
use crate::output::{Cell, Format, Output};
use crate::query::EntrySort;
use crate::session::Session;

fn tag(name: &str, id: i32, class: &str, resources: Vec<u32>) -> EntryRecord {
    EntryRecord {
        name: name.into(),
        global_id: GlobalId(id),
        class_code: FourCc::from_code(class).unwrap(),
        parent: None,
        resources,
        asset_checksum: id as u64,
        compressed_size: 100,
        uncompressed_size: 1000,
        stored: true,
    }
}

fn resource(name: &str, parent: Option<u32>, size: u64, stored: bool) -> EntryRecord {
    EntryRecord {
        name: name.into(),
        global_id: GlobalId::NONE,
        class_code: FourCc::NONE,
        parent,
        resources: vec![],
        asset_checksum: 0,
        compressed_size: if stored { (size / 2) as u32 } else { 0 },
        uncompressed_size: size,
        stored,
    }
}

fn module(path: &str, entries: Vec<EntryRecord>) -> ModuleRecord {
    let stamp = FileStamp { size: 5000, modified_secs: 0, modified_nanos: 0 };
    ModuleRecord {
        rel_path: path.into(),
        stamp,
        version: 27,
        loaded_tag_count: 0,
        entries,
        issues: vec![],
        data_end: 5000,
    }
}

const MAGNUM: &str = r"objects\weapons\pistol\magnum\magnum";

fn session() -> Session {
    let globals = module(
        r"any\levels\globals-rtx-1.module",
        vec![
            tag(&format!("{MAGNUM}.weapon"), 0x10, "weap", vec![]),
            tag(&format!("{MAGNUM}.bitmap"), 0x11, "bitm", vec![2, 3]),
            resource(&format!("{MAGNUM}.bitmap[0:bitmap data]"), Some(1), 4_000_000, true),
            resource(&format!("{MAGNUM}.bitmap[1:mesh resource!*]"), Some(1), 9_000_000, false),
            tag(r"objects\weapons\rifle\assault_rifle\assault_rifle.weapon", 0x20, "weap", vec![]),
            tag(r"Objects\Weapons\Rifle\battle_rifle\battle_rifle.weapon", 0x21, "weap", vec![]),
            tag(r"levels\shared\lua\scripts.lua", 0x30, "hsc*", vec![]),
        ],
    );
    let mut update =
        module(r"any\levels\globals-rtx-11587-1.module", vec![tag(&format!("{MAGNUM}.weapon"), 0x10, "weap", vec![])]);
    update.entries.push(resource("loadmanifest", None, 8, true));
    let mut cut = module(
        r"pc\levels\cut.module",
        vec![tag(r"objects\weapons\rifle\assault_rifle\assault_rifle.weapon", 0x20, "weap", vec![])],
    );
    cut.data_end = 65_000_000;
    cut.issues = vec![ModuleIssue { check: checks::DATA_BOUNDS.into(), count: 3, first: "...".into() }];
    let index = Index {
        modules: vec![globals, update, cut],
        failed: vec![FailedModule { rel_path: "broken.module".into(), error: "not a module file".into() }],
    };
    let def = |code: &str, name: &str| DefinitionFile {
        file_code: code.into(),
        class_name: name.into(),
        path: PathBuf::from(format!("({code}){name}.xml")),
    };
    let defs = Definitions::from_files(vec![def("weap", "weapon"), def("hsc_", "lua"), def("snd!", "sound")]);
    let cfg = BuildConfig { name: "forge".into(), deploy: "d".into(), defs: "t".into(), resource_defs: None };
    Session::from_builds("c".into(), vec![Build::new("forge".into(), cfg, index, defs)])
}

fn run(t: &impl Tool) -> Value {
    match t.run(&mut session()) {
        Ok(out) => serde_json::from_str(&out.render(Format::Json)).unwrap(),
        Err(e) => serde_json::from_str(&e.render(Format::Json)).unwrap(),
    }
}

fn text(t: &impl Tool) -> String {
    match t.run(&mut session()) {
        Ok(out) => out.render(Format::Text),
        Err(e) => e.render(Format::Text),
    }
}

fn names(v: &Value) -> Vec<String> {
    v["rows"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap().to_owned()).collect()
}

fn page(offset: usize, limit: Option<usize>) -> PageArgs {
    PageArgs { offset, limit }
}

fn search(query: &str) -> SearchTags {
    SearchTags {
        query: query.into(),
        class: None,
        module: None,
        mode: Mode::Default,
        build: BuildArg::default(),
        page: PageArgs::default(),
    }
}

fn browse(path: &str) -> Browse {
    Browse { path: path.into(), build: BuildArg::default(), page: PageArgs::default() }
}

fn entries() -> ListEntries {
    ListEntries {
        tag: None,
        query: None,
        mode: Mode::Default,
        kind: None,
        module: None,
        min_size: None,
        max_size: None,
        sort: EntrySort::Name,
        build: BuildArg::default(),
        page: PageArgs::default(),
    }
}

#[test]
fn search_default_rule() {
    // Path contains the query, case-insensitively.
    let v = run(&search("MAGNUM"));
    assert_eq!(v["total"], 2);
    assert_eq!(v["rows"][1]["copies"], 2); // sorted by path, then class: bitm, weap
    // Or the query is a class code or class name.
    assert_eq!(run(&search("weap"))["total"], 4);
    assert_eq!(run(&search("weapon"))["total"], 4);
    assert_eq!(run(&search("lua"))["total"], 1);
}

#[test]
fn search_filters_combine() {
    let v = run(&SearchTags { class: Some("bitm".into()), ..search("magnum") });
    assert_eq!(names(&v), [format!("{MAGNUM}.bitmap")]); // class name from the file name, without a definition
}

#[test]
fn search_modes_and_paging() {
    let v = run(&SearchTags { mode: Mode::Regex, ..search(r"magnum|battle") });
    assert_eq!(v["total"], 3);
    let v = run(&SearchTags { mode: Mode::Exact, ..search(&format!("{MAGNUM}.WEAPON")) });
    assert_eq!(v["total"], 1);
    let v = run(&SearchTags { module: Some("11587".into()), ..search("") });
    assert_eq!(v["total"], 1);
    let v = run(&SearchTags { page: page(1, Some(2)), ..search("weap") });
    assert_eq!(v["rows"].as_array().unwrap().len(), 2);
    assert_eq!(v["more"]["next_offset"], 3);
    assert!(run(&SearchTags { mode: Mode::Regex, ..search("(") })["error"].as_str().unwrap().contains("invalid regex"));
}

#[test]
fn absent_classes_are_explained_not_denied() {
    // A typo: no tags and no definition.
    let v = run(&SearchTags { class: Some("nope".into()), ..search("") });
    assert_eq!(v["total"], 0);
    assert!(v["hints"][0].as_str().unwrap().contains("check the spelling"));
    // A real class whose tags aren't in the indexed modules.
    let v = run(&SearchTags { class: Some("sound".into()), ..search("") });
    assert_eq!(v["total"], 0);
    let hint = v["hints"][0].as_str().unwrap();
    assert!(hint.contains("has a definition") && hint.contains("Don't say the game has none"), "{hint}");
    let v = run(&ListEntries { kind: Some("snd!".into()), ..entries() });
    assert!(v["hints"][0].as_str().unwrap().contains("has a definition"));
    let v = run(&ListEntries { kind: Some("nothing".into()), ..entries() });
    assert!(v["hints"][0].as_str().unwrap().contains("no entry kind"));
}

#[test]
fn search_text_output() {
    let t = text(&SearchTags { class: Some("weapon".into()), ..search("rifle") });
    assert_eq!(
        t,
        "total: 2\nin objects\\weapons\\rifle\\:\nglobal ID\tclass\tdisplay name\tcopies\n\
         0x00000020\tweap\tassault_rifle\\assault_rifle.weapon\t2\n\
         0x00000021\tweap\tbattle_rifle\\battle_rifle.weapon\t1\n"
    );
}

#[test]
fn class_names_come_from_file_names() {
    let s = session();
    let b = &s.builds()[0];
    let hsc = &b.classes[&FourCc::from_code("hsc*").unwrap()];
    assert_eq!(hsc.name, "lua");
    // Found by name, since the definition file says hsc_.
    assert_eq!(hsc.definition.as_deref(), Some(std::path::Path::new("(hsc_)lua.xml")));
    assert!(b.classes[&FourCc::from_code("bitm").unwrap()].definition.is_none());
}

#[test]
fn browse_folders_and_tags() {
    let v = run(&browse(""));
    assert_eq!(names(&v), [r"levels\", r"objects\"]);
    assert_eq!(v["rows"][1]["tags"], 4);

    // Case-insensitive; folders keep the casing first seen.
    let v = run(&browse(r"OBJECTS\weapons\"));
    assert_eq!(names(&v), [r"pistol\", r"rifle\"]);
    let v = run(&browse(r"objects\weapons\pistol\magnum"));
    assert_eq!(names(&v), ["magnum.bitmap", "magnum.weapon"]);
    assert_eq!(v["tags_here"], 2);
    assert!(run(&browse("nowhere"))["error"].is_string());
}

#[test]
fn list_classes_and_modules() {
    let v = run(&ListClasses { build: BuildArg::default() });
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(v["total"], 3);
    let weap = rows.iter().find(|r| r["class"] == "weap").unwrap();
    assert_eq!(
        (weap["class_name"].as_str(), weap["tags"].as_i64(), weap["definition"].as_bool()),
        (Some("weapon"), Some(3), Some(true))
    );

    let v = run(&ListModules { build: BuildArg::default(), page: PageArgs::default() });
    assert_eq!(v["total"], 4);
    // The build's formats and newest game version (D63).
    assert_eq!((v["formats"].as_str(), v["latest_game_version"].as_str()), (Some("27 (forge)"), Some("11587-1")));
    let status: Vec<&str> = v["rows"].as_array().unwrap().iter().map(|r| r["status"].as_str().unwrap()).collect();
    assert_eq!(status[0], "ok");
    assert!(status[2].starts_with("!! incomplete file: 65.0 MB missing; 3 entries"), "{}", status[2]);
    assert!(status[3].starts_with("!! not indexed"));
    assert_eq!(v["rows"][1]["load_manifest"], true);
    assert!(v["hints"][0].as_str().unwrap().contains("2 module(s)"));
}

#[test]
fn list_entries_of_a_tag() {
    let v = run(&ListEntries { tag: Some(format!("{MAGNUM}.bitmap")), ..entries() });
    // Tag path + class name (bitm has no definition, but its file name gives "bitmap").
    assert_eq!(v["total"], 3);
    assert_eq!(v["total_size"], 1000 + 4_000_000 + 9_000_000);
    let rows = v["rows"].as_array().unwrap();
    let mesh = rows.iter().find(|r| r["kind"] == "mesh resource").unwrap();
    assert_eq!(mesh["compressed"], "(not stored in this module)");
    assert_eq!(mesh["size"], 9_000_000);
    assert!(v["hints"][0].as_str().unwrap().contains("not stored"));

    // The hint names the modules holding a copy, so another can be chosen;
    // the latest layer's copy is the default (D58).
    let v = run(&ListEntries { tag: Some("0x00000010".into()), ..entries() });
    assert_eq!(
        v["hints"][0],
        r"the tag has 2 copies: any\levels\globals-rtx-11587-1.module (this), any\levels\globals-rtx-1.module; use module= to choose another"
    );
    let v =
        run(&ListEntries { tag: Some("0x00000010".into()), module: Some("globals-rtx-1.module".into()), ..entries() });
    assert_eq!(v["rows"][0]["module"], r"any\levels\globals-rtx-1.module");
    // The copy shown comes first.
    assert_eq!(
        v["hints"][0],
        r"the tag has 2 copies: any\levels\globals-rtx-1.module (this), any\levels\globals-rtx-11587-1.module; use module= to choose another"
    );
    let v = run(&ListEntries { tag: Some("0x00000010".into()), module: Some("nowhere".into()), ..entries() });
    assert!(v["error"].as_str().unwrap().contains("has no copy"));
}

#[test]
fn list_entries_filters() {
    // query matches a resource by its owner's tag path.
    let v = run(&ListEntries { query: Some("magnum".into()), kind: Some("bitmap data".into()), ..entries() });
    assert_eq!(v["total"], 1);
    let v = run(&ListEntries { kind: Some("weapon".into()), ..entries() });
    assert_eq!(v["total"], 5); // entries, so copies count
    let v = run(&ListEntries { kind: Some("file".into()), ..entries() });
    assert_eq!(names(&v), ["loadmanifest"]);
    let v = run(&ListEntries { min_size: Some(Size(1_000_000)), max_size: Some(Size(5_000_000)), ..entries() });
    assert_eq!(v["total"], 1);
    let v = run(&ListEntries { sort: EntrySort::Size, page: page(0, Some(1)), ..entries() });
    assert_eq!(v["rows"][0]["kind"], "mesh resource");
}

#[test]
fn resource_kinds() {
    let v = run(&ListResourceKinds { build: BuildArg::default() });
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0]["kind"].as_str(), rows[0]["count"].as_i64(), rows[0]["total_size"].as_u64()),
        (Some("bitmap data"), Some(1), Some(4_000_000))
    );
}

#[test]
fn tag_arguments() {
    let s = session();
    let default = BuildArg::default();
    let id = |text: &str| args::tag(&s, text, &default).map(|(_, t)| t.global_id);
    assert_eq!(id("0x00000010"), Ok(GlobalId(0x10)));
    assert_eq!(id("0X10"), Ok(GlobalId(0x10)));
    assert_eq!(id(&format!("forge:{MAGNUM}.weapon")), Ok(GlobalId(0x10)));
    assert_eq!(id(r"objects\weapons\rifle\assault_rifle\ASSAULT_RIFLE"), Ok(GlobalId(0x20)));
    // A path shared by two classes is ambiguous.
    let err = id(MAGNUM).unwrap_err();
    assert!(err.message.contains("matches 2 tags"), "{}", err.message);
    assert!(id("0x99").is_err());
    assert!(id("0xZZ").unwrap_err().message.contains("invalid global ID"));
    let xbox = BuildArg { build: Some("xbox".into()) };
    assert!(args::tag(&s, "x", &xbox).unwrap_err().message.contains("unknown build"));
}

#[test]
fn reindex_reports_text_and_json_from_one_report() {
    let mut s = session();
    let out =
        Reindex { what: What::Results, module: None, class: None, build: BuildArg::default() }.run(&mut s).unwrap();
    let Output::Report { items, .. } = &out else { panic!("expected a report") };
    assert_eq!(items[0].key, "results");
    // Both renderings come from the same items (no cache file here: nothing dropped).
    assert_eq!(out.render(Format::Text), "results: dropped modules 0, rebuilt on next use\n");
    let v: Value = serde_json::from_str(&out.render(Format::Json)).unwrap();
    assert_eq!(v["results"]["rebuilt"], "on next use");
}

#[test]
fn broad_searches_show_hits_per_class() {
    let v = run(&search("magnum"));
    assert_eq!(
        v["by_class"]["items"],
        serde_json::json!([{ "name": "bitm", "count": 1 }, { "name": "weap", "count": 1 }])
    );
    assert_eq!(v["by_class"]["more"], 0);
    assert!(text(&search("magnum")).starts_with("total: 2  by class: bitm 1, weap 1\n"));
    // One class only: nothing to narrow down.
    assert!(run(&SearchTags { class: Some("weap".into()), ..search("") }).get("by_class").is_none());
}

#[test]
fn class_breakdown_is_ordered_and_capped() {
    let counts = (0..10).map(|i| (format!("c{i}"), 10 - i)).collect();
    assert_eq!(
        Cell::counts(counts, 3),
        Cell::Counts { items: vec![("c0".into(), 10), ("c1".into(), 9), ("c2".into(), 8)], more: 7 }
    );
}

#[test]
fn empty_searches_say_why() {
    // A typo: close names are suggested.
    let v = run(&search("magnam"));
    assert_eq!(v["total"], 0);
    assert_eq!(v["hints"][0], "no tag path contains 'magnam'; similar names in tag paths: magnum");
    // Nothing close: a next step instead.
    let hint = run(&search("zzzzzz"))["hints"][0].as_str().unwrap().to_owned();
    assert!(hint.ends_with("try a shorter query, or browse a folder"), "{hint}");
    // The query matches, but a filter removed every hit.
    let v = run(&SearchTags { class: Some("lua".into()), ..search("magnum") });
    assert_eq!(v["hints"][0], "no tag matches every filter; without class= and module=, the query matches 2 tags");
    let v = run(&SearchTags { mode: Mode::Regex, ..search("^nothing$") });
    assert!(v["hints"][0].as_str().unwrap().starts_with("no tag path matches the regex"));
}

#[test]
fn copies_hint_names_at_most_five_modules() {
    let modules: Vec<String> = (1..=7).map(|i| format!("m{i}.module")).collect();
    let names: Vec<&str> = modules.iter().map(String::as_str).collect();
    // The copy shown is always named, even past the first five.
    assert_eq!(
        present::other_copies_hint(&names, "m7.module"),
        "the tag has 7 copies: m7.module (this), m1.module, m2.module, m3.module, m4.module (+2 more); use module= to choose another"
    );
}
