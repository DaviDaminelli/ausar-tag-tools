//! How long calls take when one session makes them again and again, as the
//! MCP server does (PRD §13: what a long-running process keeps in memory).
//! Prints the first and the repeated time of each call; ignored by default.
//!
//! ```text
//! H5_DEPLOY=<Forge deploy folder> cargo test --release -p h5tags-core --test real_repeat_calls -- --ignored --nocapture
//! ```
//!
//! Tags are named by global ID (game file names stay out of code).

mod common;

use std::time::{Duration, Instant};

use clap::Parser;
use h5tags_core::{Command, Format};

/// The calls of the simulated battery session (`docs/progress.md`), as CLI command lines.
const CALLS: &[&str] = &[
    "read-tag 0x00008593",
    "read-tag 0x00008593 --path barrels[0] --depth 1",
    "read-tag 0x000041B7 --path barrels[0] --depth 1",
    "outline --tag 0x00008593 --depth 1",
    "get-definition weap --path barrels",
    "search-definitions energy --class weap",
    "search-definitions rounds",
];

const ROUNDS: usize = 3;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

fn parse(line: &str) -> Command {
    // Splits on spaces; a quoted argument would need more, none is used here.
    Cli::try_parse_from(std::iter::once("h5tags").chain(line.split(' ')))
        .unwrap_or_else(|e| panic!("{line}: {e}"))
        .command
}

#[test]
#[ignore]
fn repeated_calls() {
    let (mut s, _cache) = common::real_session("core-repeat-calls");
    let commands: Vec<Command> = CALLS.iter().map(|l| parse(l)).collect();
    let mut times = vec![Vec::<Duration>::new(); CALLS.len()];
    for _ in 0..ROUNDS {
        for (c, t) in commands.iter().zip(&mut times) {
            let start = Instant::now();
            let out = c.run(&mut s).unwrap_or_else(|e| panic!("{}: {e} ({:?})", c.name(), e.hint));
            out.render(Format::Text);
            t.push(start.elapsed());
        }
    }
    println!("| call | first | repeated (mean of {}) |", ROUNDS - 1);
    println!("| --- | --- | --- |");
    for (line, t) in CALLS.iter().zip(&times) {
        let repeated = t[1..].iter().sum::<Duration>() / (ROUNDS as u32 - 1);
        println!("| `{line}` | {:.1?} | {:.1?} |", t[0], repeated);
    }
}
