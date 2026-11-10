//! How long building the header scan takes on real builds (PRD §6.2), from
//! nothing: each round uses a fresh cache folder, so the index and the scan
//! are both built. For measuring a change to the scan (M6: the data
//! fingerprint). Ignored by default; run it in release.
//!
//! ```text
//! H5_CONFIG=<config> cargo test --release -p h5tags-core --test real_scan_timing -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;

use h5_module::testutil::TempDir;
use h5tags_core::references::header_scan;
use h5tags_core::{Config, Session};

const ROUNDS: usize = 3;

#[test]
#[ignore]
fn header_scan_from_nothing() {
    let path = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    for round in 1..=ROUNDS {
        let cache = TempDir::new(&format!("scan-timing-{round}"));
        let mut config = Config::load(&path).unwrap();
        config.cache_dir = Some(cache.0.clone());
        let start = Instant::now();
        let s = Session::open(config).unwrap();
        let index = start.elapsed();
        for b in s.builds() {
            let (scan, built) = header_scan(b);
            let built = built.expect("a fresh cache folder: the scan is built");
            let copies: usize = scan.scan.modules.iter().map(|m| m.tags.len()).sum();
            println!(
                "round {round}: {} scan {:.2} s ({copies} copies; index of every build {:.2} s)",
                b.name,
                built.elapsed.as_secs_f64(),
                index.as_secs_f64()
            );
        }
    }
}

/// D81: what the tag data fingerprint adds to the scan, measured steadily:
/// the hash's speed, times the bytes of the tag entries the scan reads (their
/// whole size: header and tag data, an upper bound on what is hashed).
#[test]
#[ignore]
fn fingerprint_cost() {
    use h5_module::index::EntryKind;
    use h5_module::murmur3::murmur3_x64_128;
    let buffer: Vec<u8> = (0..256u32 << 20).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
    let start = Instant::now();
    let print = murmur3_x64_128(&buffer, 0);
    let per_second = buffer.len() as f64 / start.elapsed().as_secs_f64();
    println!("MurmurHash3 x64-128: {:.0} MB/s (print {print:x?})", per_second / 1e6);
    let path = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let s = Session::open(Config::load(&path).unwrap()).unwrap();
    for b in s.builds() {
        let bytes: u64 = b
            .index
            .modules
            .iter()
            .flat_map(|m| &m.entries)
            .filter(|e| e.kind() == EntryKind::Tag)
            .map(|e| e.uncompressed_size)
            .sum();
        println!(
            "{}: tag entries {:.0} MB → at most {:.2} s of hashing",
            b.name,
            bytes as f64 / 1e6,
            bytes as f64 / per_second
        );
    }
}
