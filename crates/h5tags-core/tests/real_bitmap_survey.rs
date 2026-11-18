//! M10 survey (bitmaps, PRD §6.8): before any bitmap code, what a bitmap's
//! resource holds and where its pixels are. For every `bitm` tag's default
//! copy: each `bitmaps` entry (size, type, format, mips), its resource file
//! entry (sections, whether it is stored like a tag, read with
//! `(0008)bitmap_data_resource_definition.xml`), the chunk entries, and the
//! tile mode (PRD Q4). Counts only; conclusions go to format-notes.
//! Ignored by default.
//!
//! ```text
//! H5_CONFIG=<config> cargo test --release -p h5tags-core --test real_bitmap_survey -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use h5_meta::{Body, Content, Value, read_tag};
use h5_module::tag::TagHeader;
use h5_module::{FourCc, Module, Section, map_file};
use h5tags_core::query::choose_copy;
use h5tags_core::stored::{load_definition, load_definition_file, read_stored_tag};
use h5tags_core::{Config, Session};

fn session() -> (Session, PathBuf) {
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let cfg = Config::load(&config).unwrap();
    let defs = cfg.builds[0].resource_defs.clone().expect("resource_defs in the config");
    (Session::open(cfg).unwrap(), defs)
}

/// A field's value by name in `body` (top level).
fn value<'a>(body: &'a Body<'_>, name: &str) -> Option<&'a Value> {
    body.children.iter().find(|n| n.name == Some(name)).and_then(|n| match &n.content {
        Content::Value(v) => Some(v),
        _ => None,
    })
}

fn int(body: &Body<'_>, name: &str) -> i64 {
    try_int(body, name).unwrap_or_else(|| panic!("{name}: {:?}", value(body, name)))
}

fn try_int(body: &Body<'_>, name: &str) -> Option<i64> {
    match value(body, name)? {
        Value::Int(i) => Some(*i),
        Value::Enum(e) => Some(*e),
        Value::Flags(f) => Some(*f as i64),
        _ => None,
    }
}

/// One `bitmaps` entry: (width, height, depth, type, format, mip count).
type Image = (i64, i64, i64, i64, i64, i64);

/// A format's storage (by the `format` option's value): (block width and
/// height in pixels, bytes per block). From the option names (`bc7_unorm`,
/// `b8g8r8a8_unorm`…) and the formats' public specifications.
fn storage(format: i64) -> Option<(u64, u64)> {
    Some(match format {
        1 => (1, 1),                       // r8_unorm_rrr1
        11 | 51 => (1, 4),                 // b8g8r8a8_unorm, r11g11b10_float
        25 => (1, 8),                      // r16g16b16a16_float
        14 | 36 | 43 => (4, 8),            // bc1, bc4 (two variants)
        16 | 39 | 45 | 47 | 49 => (4, 16), // bc3, bc5 snorm, bc5 unorm, bc6h, bc7
        _ => return None,
    })
}

/// Bytes of one mip of one face or slice.
fn mip_bytes(format: i64, w: i64, h: i64, mip: i64) -> Option<u64> {
    let (block, bytes) = storage(format)?;
    let (w, h) = (((w >> mip).max(1)) as u64, ((h >> mip).max(1)) as u64);
    Some(w.div_ceil(block) * h.div_ceil(block) * bytes)
}

/// Faces or slices: 6 for a cube map, `depth` for an array, else 1.
fn layers(img: &Image) -> i64 {
    match img.3 {
        2 => 6,
        3 => img.2,
        _ => 1,
    }
}

#[test]
#[ignore]
fn what_a_bitmap_resource_holds() {
    let (s, rdefs) = session();
    let rdef = load_definition_file(&rdefs.join("(0008)bitmap_data_resource_definition.xml")).unwrap();
    let bitm = FourCc::from_code("bitm").unwrap();
    for b in s.builds() {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut add = |k: String| *counts.entry(k).or_default() += 1;
        let mut examples: BTreeMap<String, String> = BTreeMap::new();
        for t in b.tags.iter().filter(|t| t.class == bitm) {
            let Some(copy) = choose_copy(b, t, None) else { continue };
            let def = load_definition(b, t).unwrap().unwrap();
            let stored = read_stored_tag(b, copy).unwrap();
            let tag = read_tag(&def, &stored.header, &stored.tag_data);
            let Some(Content::Block(block)) =
                tag.root.children.iter().find(|n| n.name == Some("bitmaps")).map(|n| &n.content)
            else {
                add("tag without a bitmaps block".into());
                continue;
            };
            let images: Vec<Image> = block
                .entries
                .iter()
                .map(|e| {
                    let f = |n| int(e, n);
                    (f("width"), f("height"), f("depth") & 0xFF, f("type"), f("format"), f("mipmap count"))
                })
                .collect();
            let file = map_file(&b.module_file(b.module(copy))).unwrap();
            let module = Module::parse(&file).unwrap();
            let resources = module.entry_resources(copy.entry).unwrap().to_vec();
            let names: Vec<String> = resources.iter().map(|&r| module.name(r).unwrap().to_owned()).collect();
            let main: Vec<usize> =
                (0..names.len()).filter(|&i| names[i].ends_with("bitmap resource handle*]")).collect();
            add(format!("images {} resources (main) {}", if images.len() == main.len() { "=" } else { "!=" }, ""));
            for (i, img) in images.iter().enumerate() {
                let Some(&k) = main.get(i) else { continue };
                let r = resources[k];
                let entry = module.entry(r).unwrap().clone();
                let header = module.read_section(r, Section::Header).unwrap();
                let data = module.read_section(r, Section::TagData).unwrap();
                let rdata = module.read_section(r, Section::ResourceData).unwrap();
                add(format!("resource name index matches the bitmaps entry: {}", names[k].contains(&format!("[{i}:"))));
                add(format!("resource header magic {:?}", String::from_utf8_lossy(&header[..4.min(header.len())])));
                add(format!("resource data section empty: {}", rdata.is_empty()));
                // Chunks are the resource entry's own resources.
                let chunk_ids = module.entry_resources(r).unwrap().to_vec();
                let chunks: Vec<String> = chunk_ids.iter().map(|&c| module.name(c).unwrap().to_owned()).collect();
                add(format!("chunk entries {}", chunks.len()));
                let Ok(h) = TagHeader::parse(&header) else {
                    add("resource header doesn't parse as a tag header".into());
                    continue;
                };
                let res = read_tag(&rdef, &h, &data);
                add(format!(
                    "resource read problems {}: {:?}",
                    res.divergences.len(),
                    res.root.problem.as_ref().map(|p| format!("{p:?}").chars().take(80).collect::<String>())
                ));
                let (Some(tile), Some(hw), Some(flags)) = (
                    try_int(&res.root, "tileMode"),
                    try_int(&res.root, "hardware format"),
                    try_int(&res.root, "bitmap data resource flags"),
                ) else {
                    add(format!("resource fields not read (data section {} B)", data.len()));
                    continue;
                };
                let pixels = match value(&res.root, "pixels") {
                    Some(Value::Data(d)) => d.size as usize,
                    other => panic!("pixels: {other:?}"),
                };
                let streaming =
                    match res.root.children.iter().find(|n| n.name == Some("streamingData")).map(|n| &n.content) {
                        Some(Content::Block(bl)) => bl.count as usize,
                        _ => usize::MAX,
                    };
                add(format!("tileMode {tile}"));
                add(format!("flags {flags}"));
                add(format!("format {} → hardware format {hw}", img.4));
                add(format!("streamingData entries = chunk entries: {}", streaming == chunks.len()));
                let chunk_bytes: u64 =
                    chunk_ids.iter().map(|&c| module.entry(c).unwrap().total_uncompressed_size as u64).sum();
                let key = format!(
                    "type {} chunks {}{}",
                    img.3,
                    usize::from(!chunks.is_empty()),
                    if img.4 == 49 { " bc7" } else { "" }
                );
                examples.entry(key).or_insert_with(|| {
                    format!(
                        "{} [{i}]: {}x{}x{} type {} format {} mips {} | resource sections h {} d {} r {} (entry flags {:?}) | \
                         pixels {pixels} B, tileMode {tile}, hw {hw}, flags {flags}, streamingData {streaming}, chunks {} ({chunk_bytes} B)",
                        b.display_name(t),
                        img.0,
                        img.1,
                        img.2,
                        img.3,
                        img.4,
                        img.5,
                        header.len(),
                        data.len(),
                        rdata.len(),
                        entry.flags,
                        chunks.len()
                    )
                });
            }
        }
        println!("\n== {}", b.name);
        for (k, n) in &counts {
            println!("  {k}: {n}");
        }
        for (k, e) in &examples {
            println!("  e.g. {k}: {e}");
        }
    }
}

/// M10 survey, the layout: an image's bytes computed from its format,
/// width, height, mips (count + 1) and faces or slices equal its resource's
/// pixels plus its chunks; which mips each chunk holds (by size).
#[test]
#[ignore]
fn the_stored_bytes_match_the_images() {
    let (s, _) = session();
    let bitm = FourCc::from_code("bitm").unwrap();
    let b = s.builds().iter().find(|b| b.name == "forge").expect("a build named forge");
    let (mut equal, mut differ, mut unknown) = (0, Vec::new(), 0);
    let mut chunk_plans: BTreeMap<String, usize> = BTreeMap::new();
    let mut examples = Vec::new();
    for t in b.tags.iter().filter(|t| t.class == bitm) {
        let Some(copy) = choose_copy(b, t, None) else { continue };
        let def = load_definition(b, t).unwrap().unwrap();
        let stored = read_stored_tag(b, copy).unwrap();
        let tag = read_tag(&def, &stored.header, &stored.tag_data);
        let Some(Content::Block(block)) =
            tag.root.children.iter().find(|n| n.name == Some("bitmaps")).map(|n| &n.content)
        else {
            continue;
        };
        let file = map_file(&b.module_file(b.module(copy))).unwrap();
        let module = Module::parse(&file).unwrap();
        let resources = module.entry_resources(copy.entry).unwrap().to_vec();
        for (i, e) in block.entries.iter().enumerate() {
            let f = |n| int(e, n);
            let img: Image = (f("width"), f("height"), f("depth") & 0xFF, f("type"), f("format"), f("mipmap count"));
            let r = resources[i];
            let pixels = module.entry(r).unwrap().resource_data_size as u64;
            let chunks: Vec<u64> = module
                .entry_resources(r)
                .unwrap()
                .iter()
                .map(|&c| module.entry(c).unwrap().total_uncompressed_size as u64)
                .collect();
            let mips: Option<Vec<u64>> =
                (0..=img.5).map(|m| mip_bytes(img.4, img.0, img.1, m).map(|x| x * layers(&img) as u64)).collect();
            let Some(mips) = mips else {
                unknown += 1;
                continue;
            };
            let expected: u64 = mips.iter().sum();
            if expected == pixels + chunks.iter().sum::<u64>() {
                equal += 1;
            } else {
                differ.push(format!(
                    "{} [{i}] {img:?}: expected {expected}, pixels {pixels} + chunks {chunks:?}",
                    b.display_name(t)
                ));
            }
            if !chunks.is_empty() {
                // Which mips (from the largest) each chunk's size is, and what the resource keeps.
                let mut left: Vec<(usize, u64)> = mips.iter().copied().enumerate().collect();
                let plan: Vec<String> = chunks
                    .iter()
                    .map(|&c| match left.iter().position(|&(_, m)| m == c) {
                        Some(p) => format!("mip {}", left.remove(p).0),
                        None => format!("{c} B (no single mip)"),
                    })
                    .collect();
                let rest: u64 = left.iter().map(|&(_, m)| m).sum();
                let key = format!(
                    "chunks: {} | resource keeps the other {} mips: {}",
                    plan.join(", "),
                    left.len(),
                    rest == pixels
                );
                *chunk_plans.entry(key.clone()).or_default() += 1;
                if examples.len() < 4 {
                    examples.push(format!(
                        "{} [{i}] {img:?}: mips {mips:?}; pixels {pixels}; chunks {chunks:?}",
                        b.display_name(t)
                    ));
                }
            }
        }
    }
    println!(
        "\n== forge: images whose bytes equal the computed size {equal}, differ {}, format not in the table {unknown}",
        differ.len()
    );
    for d in differ.iter().take(8) {
        println!("  differs: {d}");
    }
    for (k, n) in &chunk_plans {
        println!("  {k}: {n}");
    }
    for e in &examples {
        println!("  e.g. {e}");
    }
}

/// IEEE half float to f32 (for `r16g16b16a16_float`).
fn half(bits: u16) -> f32 {
    let (sign, exp, frac) = ((bits >> 15) as u32, ((bits >> 10) & 0x1F) as u32, (bits & 0x3FF) as u32);
    let v = match exp {
        0 => frac as f32 * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + frac as f32 / 1024.0) * 2f32.powi(exp as i32 - 15),
    };
    if sign == 1 { -v } else { v }
}

/// The first channel of pixel (x, y) of a mip starting at `at`, for the
/// uncompressed formats: r8 (1), b8g8r8a8 (11), r16g16b16a16_float (25).
fn sample(format: i64, data: &[u8], at: usize, w: usize, x: usize, y: usize) -> f32 {
    match format {
        1 => data[at + y * w + x] as f32 / 255.0,
        11 => data[at + (y * w + x) * 4 + 2] as f32 / 255.0,
        _ => half(u16::from_le_bytes([data[at + (y * w + x) * 8], data[at + (y * w + x) * 8 + 1]])).clamp(0.0, 4.0),
    }
}

/// M10 survey, the order inside the data: for uncompressed images with mips,
/// which of four orders makes mip 1 look like mip 0 halved: per face (or
/// slice) all its mips, or per mip all faces; largest mip first or smallest.
#[test]
#[ignore]
fn the_order_of_mips_and_faces() {
    let (s, _) = session();
    let bitm = FourCc::from_code("bitm").unwrap();
    let b = s.builds().iter().find(|b| b.name == "forge").expect("a build named forge");
    let orders =
        ["per face, largest first", "per face, smallest first", "per mip, largest first", "per mip, smallest first"];
    let mut wins: BTreeMap<(&str, i64), usize> = BTreeMap::new();
    for t in b.tags.iter().filter(|t| t.class == bitm) {
        let Some(copy) = choose_copy(b, t, None) else { continue };
        let def = load_definition(b, t).unwrap().unwrap();
        let stored = read_stored_tag(b, copy).unwrap();
        let tag = read_tag(&def, &stored.header, &stored.tag_data);
        let Some(Content::Block(block)) =
            tag.root.children.iter().find(|n| n.name == Some("bitmaps")).map(|n| &n.content)
        else {
            continue;
        };
        let file = map_file(&b.module_file(b.module(copy))).unwrap();
        let module = Module::parse(&file).unwrap();
        let resources = module.entry_resources(copy.entry).unwrap().to_vec();
        for (i, e) in block.entries.iter().enumerate() {
            let f = |n| int(e, n);
            let img: Image = (f("width"), f("height"), f("depth") & 0xFF, f("type"), f("format"), f("mipmap count"));
            if ![1, 11, 25].contains(&img.4) || img.5 < 1 || !module.entry_resources(resources[i]).unwrap().is_empty() {
                continue;
            }
            let data = module.read_section(resources[i], Section::ResourceData).unwrap();
            let n = layers(&img) as usize;
            let sizes: Vec<usize> = (0..=img.5).map(|m| mip_bytes(img.4, img.0, img.1, m).unwrap() as usize).collect();
            let per_face: usize = sizes.iter().sum();
            let offset = |order: usize, face: usize, mip: usize| -> usize {
                match order {
                    0 => face * per_face + sizes[..mip].iter().sum::<usize>(),
                    1 => face * per_face + sizes[mip + 1..].iter().sum::<usize>(),
                    2 => sizes[..mip].iter().sum::<usize>() * n + face * sizes[mip],
                    _ => sizes[mip + 1..].iter().sum::<usize>() * n + face * sizes[mip],
                }
            };
            let (w0, h0) = (img.0 as usize, img.1 as usize);
            let (w1, h1) = ((w0 / 2).max(1), (h0 / 2).max(1));
            let score = |order: usize| -> f32 {
                let mut err = 0.0;
                for face in [0, n - 1] {
                    let (a, z) = (offset(order, face, 0), offset(order, face, 1));
                    for y in (0..h1).step_by((h1 / 32).max(1)) {
                        for x in (0..w1).step_by((w1 / 32).max(1)) {
                            let avg = (0..4)
                                .map(|k| {
                                    sample(
                                        img.4,
                                        &data,
                                        a,
                                        w0,
                                        (2 * x + k % 2).min(w0 - 1),
                                        (2 * y + k / 2).min(h0 - 1),
                                    )
                                })
                                .sum::<f32>()
                                / 4.0;
                            err += (avg - sample(img.4, &data, z, w1, x, y)).abs();
                        }
                    }
                }
                err
            };
            let scores: Vec<f32> = (0..4).map(score).collect();
            let best = (0..4).min_by(|&x, &y| scores[x].total_cmp(&scores[y])).unwrap();
            *wins.entry((orders[best], img.3)).or_default() += 1;
            if wins.values().sum::<usize>() <= 3 {
                println!("  {} [{i}] {img:?}: errors {scores:?}", b.display_name(t));
            }
        }
    }
    println!("\n== forge: the order that fits best, by image type (0 2D, 2 cube, 3 array): {wins:?}");
}

/// A BC1 or BC3 block's rough brightness: the mean of its two endpoint
/// colours' red channel (5 bits), from the block's colour part.
fn block_red(format: i64, data: &[u8], at: usize) -> f32 {
    let c = if format == 16 { at + 8 } else { at };
    let e = |o: usize| ((u16::from_le_bytes([data[c + o], data[c + o + 1]]) >> 11) & 31) as f32 / 31.0;
    (e(0) + e(2)) / 2.0
}

/// M10 survey, the order inside the data, from BC1 / BC3 images with mips:
/// a mip-1 block's endpoint brightness against the four mip-0 blocks it
/// covers, in each of the four orders. Ties (blank data) are counted apart.
#[test]
#[ignore]
fn the_order_of_mips_and_faces_in_block_data() {
    let (s, _) = session();
    let bitm = FourCc::from_code("bitm").unwrap();
    let b = s.builds().iter().find(|b| b.name == "forge").expect("a build named forge");
    let orders =
        ["per face, largest first", "per face, smallest first", "per mip, largest first", "per mip, smallest first"];
    let mut wins: BTreeMap<(&str, i64), usize> = BTreeMap::new();
    let mut ties = 0;
    for t in b.tags.iter().filter(|t| t.class == bitm) {
        let Some(copy) = choose_copy(b, t, None) else { continue };
        let def = load_definition(b, t).unwrap().unwrap();
        let stored = read_stored_tag(b, copy).unwrap();
        let tag = read_tag(&def, &stored.header, &stored.tag_data);
        let Some(Content::Block(block)) =
            tag.root.children.iter().find(|n| n.name == Some("bitmaps")).map(|n| &n.content)
        else {
            continue;
        };
        let file = map_file(&b.module_file(b.module(copy))).unwrap();
        let module = Module::parse(&file).unwrap();
        let resources = module.entry_resources(copy.entry).unwrap().to_vec();
        for (i, e) in block.entries.iter().enumerate() {
            let f = |n| int(e, n);
            let img: Image = (f("width"), f("height"), f("depth") & 0xFF, f("type"), f("format"), f("mipmap count"));
            let chunked = !module.entry_resources(resources[i]).unwrap().is_empty();
            if ![14, 16].contains(&img.4) || img.5 < 1 || img.0 < 16 || img.1 < 16 || chunked {
                continue;
            }
            let data = module.read_section(resources[i], Section::ResourceData).unwrap();
            let n = layers(&img) as usize;
            let bytes = if img.4 == 16 { 16 } else { 8 };
            let sizes: Vec<usize> = (0..=img.5).map(|m| mip_bytes(img.4, img.0, img.1, m).unwrap() as usize).collect();
            let per_face: usize = sizes.iter().sum();
            let offset = |order: usize, face: usize, mip: usize| -> usize {
                match order {
                    0 => face * per_face + sizes[..mip].iter().sum::<usize>(),
                    1 => face * per_face + sizes[mip + 1..].iter().sum::<usize>(),
                    2 => sizes[..mip].iter().sum::<usize>() * n + face * sizes[mip],
                    _ => sizes[mip + 1..].iter().sum::<usize>() * n + face * sizes[mip],
                }
            };
            let (bw0, bh0) = ((img.0 as usize).div_ceil(4), (img.1 as usize).div_ceil(4));
            let (bw1, bh1) = ((bw0 / 2).max(1), (bh0 / 2).max(1));
            let score = |order: usize| -> f32 {
                let mut err = 0.0;
                for face in [0, n - 1] {
                    let (a, z) = (offset(order, face, 0), offset(order, face, 1));
                    for by in (0..bh1).step_by((bh1 / 16).max(1)) {
                        for bx in (0..bw1).step_by((bw1 / 16).max(1)) {
                            let big = (0..4)
                                .map(|k| block_red(img.4, &data, a + ((2 * by + k / 2) * bw0 + 2 * bx + k % 2) * bytes))
                                .sum::<f32>()
                                / 4.0;
                            err += (big - block_red(img.4, &data, z + (by * bw1 + bx) * bytes)).abs();
                        }
                    }
                }
                err
            };
            let mut scores: Vec<(f32, usize)> = (0..4).map(|o| (score(o), o)).collect();
            scores.sort_by(|x, y| x.0.total_cmp(&y.0));
            if scores[1].0 - scores[0].0 < 0.05 * scores[1].0.max(1e-6) || scores[1].0 == 0.0 {
                ties += 1;
                continue;
            }
            *wins.entry((orders[scores[0].1], img.3)).or_default() += 1;
        }
    }
    println!(
        "\n== forge: the order that fits best (BC1/BC3), by image type (0 2D, 2 cube, 3 array): {wins:?}; ties {ties}"
    );
}
