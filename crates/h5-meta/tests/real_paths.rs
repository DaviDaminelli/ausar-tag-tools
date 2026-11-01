//! Field paths over every real definition (PRD §6.4.2). Ignored by default;
//! reads `H5_DEFS` (default `reference/H5Tags/tags`).

use std::path::PathBuf;

use h5_defs::definition::{Definition, Field, FieldKind};
use h5_defs::field_type::FieldType;
use h5_meta::{FieldPath, Select, Step, find_sections, parse_path};

fn tags_dir() -> PathBuf {
    std::env::var_os("H5_DEFS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/H5Tags/tags"))
}

/// Every field's path, with `[0]` on the tag blocks it passes through.
fn all_paths(fields: &[Field], prefix: &FieldPath, out: &mut Vec<FieldPath>) {
    for (i, f) in fields.iter().enumerate() {
        if f.step.is_none() {
            continue;
        }
        let mut p = prefix.clone();
        p.steps.push(Step { field: i, select: None });
        out.push(p.clone());
        if !f.children.is_empty() {
            let mut inside = p.clone();
            if f.entry_size().is_some()
                && f.attr("srcguid").is_some()
                && f.kind == h5_defs::definition::FieldKind::Known(h5_defs::field_type::FieldType::Block64)
            {
                inside.steps.last_mut().unwrap().select = Some(Select::Entry(0));
            }
            all_paths(&f.children, &inside, out);
        }
    }
}

/// Every path written out parses back to the same field; every heading finds its section.
#[test]
#[ignore]
fn every_field_path_round_trips() {
    let (mut paths, mut failures, mut headings, mut sections_found) = (0usize, Vec::new(), 0usize, 0usize);
    for p in std::fs::read_dir(tags_dir()).unwrap().map(|e| e.unwrap().path()) {
        let def = Definition::parse(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let mut all = Vec::new();
        all_paths(&def.fields, &FieldPath::default(), &mut all);
        for want in &all {
            paths += 1;
            let text = want.display(&def.fields);
            match parse_path(&def.fields, &text) {
                Ok(got) if &got == want => {}
                other => {
                    if failures.len() < 10 {
                        failures.push(format!("{}: {text:?} → {other:?}", p.file_name().unwrap().to_string_lossy()));
                    }
                }
            }
        }
        fn heads(fields: &[Field], out: &mut Vec<String>) {
            for f in fields {
                if f.kind == FieldKind::Known(FieldType::Explanation) && !f.name.display.is_empty() {
                    out.push(f.name.display.clone());
                }
                heads(&f.children, out);
            }
        }
        let mut names = Vec::new();
        heads(&def.fields, &mut names);
        for h in names {
            headings += 1;
            sections_found += usize::from(!find_sections(&def.fields, &h).is_empty());
        }
    }
    println!("{paths} field paths; round-trip failures: {} {failures:#?}", failures.len());
    println!("explanation headings {headings}, found as sections {sections_found}");
    assert!(failures.is_empty());
    assert_eq!(headings, sections_found);
}
