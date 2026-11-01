//! Tests against Lord Zedd's H5Tags definitions (not in the repo). Ignored by
//! default; `H5_DEFS` points at the `tags` folder (default
//! `reference/H5Tags/tags`), and `tag_resources` is its sibling.
//!
//! ```text
//! cargo test -p h5-defs --test real_definitions -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use h5_defs::definition::{Definition, Field, FieldKind};

fn tags_dir() -> PathBuf {
    std::env::var_os("H5_DEFS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/H5Tags/tags"))
}

fn walk<'a>(fields: &'a [Field], out: &mut Vec<&'a Field>) {
    for f in fields {
        out.push(f);
        walk(&f.children, out);
    }
}

/// PRD §5.3: every definition parses, and the field type table knows every
/// field type they use.
#[test]
#[ignore]
fn every_definition_parses_and_every_type_is_known() {
    let tags = tags_dir();
    let dirs = [tags.clone(), tags.join("../tag_resources")];
    let start = Instant::now();
    let (mut files, mut fields) = (0usize, 0usize);
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
    let mut no_size: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for dir in &dirs {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        paths.sort();
        for path in paths.iter().filter(|p| p.extension().is_some_and(|x| x == "xml")) {
            files += 1;
            let xml = std::fs::read_to_string(path).unwrap();
            let def = match Definition::parse(&xml) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            let mut all = Vec::new();
            walk(&def.fields, &mut all);
            fields += all.len();
            for f in &all {
                if let FieldKind::Unknown(name) = &f.kind {
                    *unknown.entry(name.clone()).or_default() += 1;
                }
            }
            if def.size().is_none() && no_size.len() < 5 {
                no_size.push(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    println!("{files} definition files, {fields} fields, parsed in {:.2?}", start.elapsed());
    println!("parse errors: {}", errors.len());
    for e in errors.iter().take(5) {
        println!("  {e}");
    }
    println!("unknown field types: {unknown:?}");
    println!("definitions without a computable size (first 5): {no_size:?}");
    assert!(errors.is_empty() && unknown.is_empty(), "see above");
}

/// PRD §5.3 name syntax and definition rules, over every definition.
#[test]
#[ignore]
fn names_and_rules_over_every_definition() {
    use h5_defs::definition::{Custom, custom_role, unique_names};
    use h5_defs::name::Name;

    #[derive(Default)]
    struct Tally {
        names: usize,
        help: usize,
        units: usize,
        range: usize,
        old_name: usize,
        two_old_names: usize,
        label: usize,
        hidden: usize,
        labels_entries: usize,
        unclean: Vec<String>,
        roles: BTreeMap<&'static str, usize>,
        duplicates: usize,
        duplicate_example: Option<String>,
        internal: usize,
        mapping: usize,
        unnamed: usize,
    }
    fn count(t: &mut Tally, n: &Name, raw: &str) {
        t.names += 1;
        t.help += usize::from(n.help.is_some());
        t.units += usize::from(n.units.is_some());
        t.range += usize::from(n.range.is_some());
        t.old_name += usize::from(!n.old_names.is_empty());
        t.two_old_names += usize::from(n.old_names.len() > 1);
        t.label += usize::from(n.label.is_some());
        t.hidden += usize::from(n.is_hidden());
        t.labels_entries += usize::from(n.labels_entries());
        if n.display.contains(['^', '*', '!', '~', '{', '}', '|']) && t.unclean.len() < 8 {
            t.unclean.push(format!("{raw:?} → {:?}", n.display));
        }
    }
    fn visit(fields: &[Field], file: &str, t: &mut Tally) {
        for (i, f) in fields.iter().enumerate() {
            count(t, &f.name, &f.raw_name);
            for o in &f.options {
                count(t, o, &o.display);
            }
            if let Some(role) = custom_role(fields, i) {
                let key = match role {
                    Custom::ClassSection(_) => "class section",
                    Custom::Group(_) => "group heading",
                    Custom::Widget => "widget",
                };
                *t.roles.entry(key).or_default() += 1;
            }
            t.internal += usize::from(f.is_internal());
            t.mapping += usize::from(f.is_mapping_function());
            let is_text = matches!(
                f.kind,
                FieldKind::Known(h5_defs::field_type::FieldType::Explanation | h5_defs::field_type::FieldType::Custom)
            );
            t.unnamed += usize::from(!is_text && f.name.display.is_empty());
            visit(&f.children, file, t);
        }
        for n in unique_names(fields).into_iter().flatten().filter(|n| n.contains('#')) {
            t.duplicates += 1;
            t.duplicate_example.get_or_insert(format!("{file}: {n}"));
        }
    }
    let mut t = Tally::default();
    let mut classes_with_internal_top = 0;
    let mut paths: Vec<PathBuf> = std::fs::read_dir(tags_dir()).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    for p in &paths {
        let def = Definition::parse(&std::fs::read_to_string(p).unwrap()).unwrap();
        classes_with_internal_top +=
            usize::from(def.fields.len() >= 2 && def.fields[0].is_internal() && def.fields[1].is_internal());
        visit(&def.fields, &p.file_name().unwrap().to_string_lossy(), &mut t);
    }
    println!(
        "{} class definitions; {classes_with_internal_top} start with vtable space + internal struct",
        paths.len()
    );
    println!(
        "names (fields + options): {}; help {}, units {}, range {}, old names {} (two: {}), label {}; hidden (!) {}, labels entries (^) {}",
        t.names, t.help, t.units, t.range, t.old_name, t.two_old_names, t.label, t.hidden, t.labels_entries
    );
    println!("display names still holding ^*!~{{}}| : {:?}", t.unclean);
    println!("customs by role: {:?}", t.roles);
    println!("fields renamed #2, #3…: {} (e.g. {:?})", t.duplicates, t.duplicate_example);
    println!(
        "internal fields {}, mapping functions {}, value fields with an empty name {}",
        t.internal, t.mapping, t.unnamed
    );
    assert!(t.unclean.is_empty(), "see above");
}
