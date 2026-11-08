//! M5d step 2 check (decisions D70): a main struct whose definition has
//! another size is read up to its last anchor the tag header confirms. With
//! two builds on the same deploy, `a` with definitions that don't fit some
//! classes and `b` with definitions that do, every value the partial read of
//! `a` shows must equal the value at the same path in `b`'s whole read.
//! Counts only. Ignored by default.
//!
//! ```text
//! H5_CONFIG=<config: a, b on one deploy> cargo test --release -p h5tags-core \
//!   --test real_partial_read_survey -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use h5_meta::{Body, Content, Problem, Value, read_tag};
use h5_module::FourCc;
use h5tags_core::def_check::definition_check;
use h5tags_core::stored::{load_definition, read_stored_tag};
use h5tags_core::{Config, Session};

/// Every value read, by field path (`a.b[2].c`).
fn values<'r>(body: &'r Body<'_>, path: &str, out: &mut HashMap<String, &'r Value>) {
    for n in &body.children {
        let p = if path.is_empty() {
            n.name.unwrap_or_default().to_owned()
        } else {
            format!("{path}.{}", n.name.unwrap_or_default())
        };
        match &n.content {
            Content::Value(v) => {
                out.insert(p, v);
            }
            Content::Struct(b) | Content::Array(b) => values(b, &p, out),
            Content::Block(k) => {
                for (i, e) in k.entries.iter().enumerate() {
                    values(e, &format!("{p}[{i}]"), out);
                }
            }
            Content::Heading => {}
        }
    }
}

#[derive(Default)]
struct PerClass {
    copies: usize,
    read_some: usize,
    bytes_read: u64,
    bytes: u64,
    values: usize,
    equal: usize,
    different: usize,
    not_in_b: usize,
    /// Copies whose main struct doesn't fit b's definition either.
    b_too: usize,
    last_anchors: BTreeMap<String, usize>,
}

#[test]
#[ignore]
fn partial_reads_match_a_definition_that_fits() {
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let s = Session::open(Config::load(&config).unwrap()).unwrap();
    let (a, b) = (&s.builds()[0], &s.builds()[1]);
    let (check, _) = definition_check(a);
    let mismatched: Vec<_> = check
        .modules
        .iter()
        .flat_map(|m| &m.tags)
        .filter(|t| t.size_mismatches.iter().any(String::is_empty))
        .map(|t| (t.global_id, t.class))
        .collect();
    println!("{}: {} copies with a main struct size mismatch", a.name, mismatched.len());
    let mut by_class: BTreeMap<FourCc, PerClass> = BTreeMap::new();
    let mut shown = 0;
    for (id, class) in mismatched {
        let (Some(ta), Some(tb)) = (a.tag_by_id(id), b.tag_by_id(id)) else { continue };
        let c = by_class.entry(class).or_default();
        // Every copy of the tag, in both builds (same deploy, same copies).
        for (&ca, &cb) in ta.copies.iter().zip(&tb.copies) {
            let (Ok(Some(da)), Ok(Some(db))) = (load_definition(a, ta), load_definition(b, tb)) else { continue };
            let (Ok(sa), Ok(sb)) = (read_stored_tag(a, ca), read_stored_tag(b, cb)) else { continue };
            let (ra, rb) = (read_tag(&da, &sa.header, &sa.tag_data), read_tag(&db, &sb.header, &sb.tag_data));
            let Some(Problem::SizeMismatch { tag, read_up_to, .. }) = &ra.root.problem else { continue };
            c.copies += 1;
            c.bytes += u64::from(*tag);
            if let Some(r) = read_up_to {
                c.read_some += 1;
                c.bytes_read += u64::from(r.bytes);
                *c.last_anchors.entry(r.last_anchor.clone()).or_default() += 1;
            }
            if rb.root.problem.is_some() {
                // b's definition doesn't fit either: nothing to compare with.
                c.b_too += 1;
                continue;
            }
            let (mut va, mut vb) = (HashMap::new(), HashMap::new());
            values(&ra.root, "", &mut va);
            values(&rb.root, "", &mut vb);
            for (p, v) in &va {
                c.values += 1;
                match vb.get(p) {
                    Some(w) if w == v => c.equal += 1,
                    Some(w) => {
                        c.different += 1;
                        if shown < 10 {
                            shown += 1;
                            println!("  differs: {} {p}: {v:?} vs {w:?}", ta.path);
                        }
                    }
                    None => c.not_in_b += 1,
                }
            }
        }
    }
    for (class, c) in &by_class {
        println!(
            "== {class}: {} copies, {} read in part ({} of {} bytes); not fitting {} either {}; compared values {}: equal {}, different {}, path not in {} {}",
            c.copies,
            c.read_some,
            c.bytes_read,
            c.bytes,
            b.name,
            c.b_too,
            c.values,
            c.equal,
            c.different,
            b.name,
            c.not_in_b
        );
        println!("   last anchor: {:?}", c.last_anchors);
    }
}
