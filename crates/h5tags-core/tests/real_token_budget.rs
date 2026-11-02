//! The token budget test (PRD §9, first step of M5a; decisions D49): replays
//! the h5tags calls an AI made in real sessions (the M4 acceptance session and
//! its retests) and fails when a call's text result grows past its budget,
//! the size measured when the call was added plus 20%. It keeps later output
//! changes from quietly making answers more expensive. Ignored by default.
//!
//! ```text
//! H5_DEPLOY=<Forge deploy folder> cargo test --release -p h5tags-core --test real_token_budget -- --ignored --nocapture
//! ```
//!
//! The calls are the AI's own, with their tag paths (the user: tests may check
//! real names). A budget of 0 means "not measured yet": the run prints the
//! budget to put there.

mod common;

use h5tags_core::{Command, Format};

/// (tool, MCP arguments, budget in characters of the text result).
const CALLS: &[(&str, &str, usize)] = &[
    ("search_tags", r#"{"query":"assault_rifle","limit":200}"#, 28653),
    ("search_tags", r#"{"query":"assault rifle","mode":"exact"}"#, 180),
    ("search_tags", r#"{"query":"assault_rifle","limit":100,"offset":200}"#, 5140),
    (
        "search_tags",
        r#"{"query":"(\\\\ar_|_ar_|_ar\\.|\\\\ar\\\\|assaultrifle|assault-rifle)","mode":"regex","limit":200}"#,
        16487,
    ),
    ("browse", r#"{"path":"ui\\wpf\\anubisui\\hud\\player_huds\\precision_aim\\human\\ar"}"#, 612),
    ("browse", r#"{"path":"ui\\hud\\weapons\\human\\ar"}"#, 168),
    ("browse", r#"{"path":"__chore\\pc__\\ui\\wpf\\anubisui\\hud\\player_huds\\precision_aim\\human\\ar"}"#, 186),
    ("browse", r#"{"path":"objects\\weapons\\rifle"}"#, 1011),
    ("browse", r#"{"path":"objects\\weapons\\rifle\\br"}"#, 623),
    ("outline", r#"{"tag":"objects\\weapons\\rifle\\br\\br.weapon","depth":2}"#, 12414),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\br\\br.weapon","path":"barrels[0].firing","detail":"normal"}"#,
        759,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\br\\br.weapon","path":"new triggers[0]","detail":"normal","options":"all","depth":1}"#,
        2069,
    ),
    ("browse", r#"{"path":"objects\\weapons\\rifle\\smg"}"#, 651),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels","detail":"brief","depth":2}"#,
        6371,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels","detail":"brief","depth":2}"#,
        6149,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].projectile error","detail":"normal"}"#,
        503,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].Recoil view kick acceleration","detail":"brief","depth":3}"#,
        2042,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].Recoil view kick acceleration","detail":"brief","depth":3}"#,
        2022,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].Recoil gun kick acceleration","detail":"brief","depth":3}"#,
        1898,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].Recoil gun kick acceleration","detail":"brief","depth":3}"#,
        1868,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].procedural animation fp fire impulse","detail":"brief","depth":3}"#,
        726,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].procedural animation fp fire impulse","detail":"brief","depth":3}"#,
        444,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels","detail":"brief","depth":3}"#,
        9112,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels","detail":"brief","depth":3}"#,
        8584,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].accuracy penalties.single wield penalties.firing penalty function","detail":"normal","depth":3}"#,
        471,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].accuracy penalties.single wield penalties.firing penalty function","detail":"normal","depth":3}"#,
        447,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].Recoil view kick acceleration","detail":"normal","depth":4}"#,
        2061,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].Recoil view kick acceleration","detail":"normal","depth":4}"#,
        2044,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].Recoil gun kick acceleration.gunkick hip settings","detail":"normal","depth":4}"#,
        1018,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].Recoil gun kick acceleration.gunkick hip settings","detail":"normal","depth":4}"#,
        995,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].procedural animation fp fire impulse.procedural animation fp fire kickback impulse hip","detail":"normal","depth":3}"#,
        597,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].automatic fire sound.automatic fire variations","detail":"brief"}"#,
        648,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].automatic fire sound.automatic fire variations","detail":"brief"}"#,
        603,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].automatic fire sound.automatic fire variations[*].firing loop for automatics","detail":"brief"}"#,
        2226,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].automatic fire sound.automatic fire variations[*].firing loop for automatics","detail":"brief"}"#,
        2030,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].Recoil view kick acceleration","detail":"brief","depth":4}"#,
        2042,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].Recoil view kick acceleration","detail":"brief","depth":4}"#,
        2022,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].Recoil gun kick acceleration","detail":"brief","depth":4}"#,
        1898,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].Recoil gun kick acceleration","detail":"brief","depth":4}"#,
        1868,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].procedural animation fp fire impulse.procedural animation fp fire kickback impulse hip[0].procedural animation fp fire kickback impulse","detail":"normal"}"#,
        485,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].accuracy penalties.single wield penalties.firing penalty function","detail":"full","depth":3}"#,
        478,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels[0].accuracy penalties.single wield penalties.firing penalty function","detail":"full","depth":3}"#,
        454,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels[0].accuracy penalties.single wield penalties","detail":"full","depth":1}"#,
        1200,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\assault_rifle\\assault_rifle.weapon","path":"barrels","detail":"brief","depth":5}"#,
        11166,
    ),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\smg\\smg.weapon","path":"barrels","detail":"brief","depth":5}"#,
        10774,
    ),
    // M5a acceptance session: "Which tags reference the battle rifle's bullet?"
    ("browse", r#"{"path":"objects\\weapons\\rifle\\br\\projectiles"}"#, 347),
    (
        "read_tag",
        r#"{"tag":"objects\\weapons\\rifle\\br\\br.weapon","path":"barrels[*].projectile","detail":"brief"}"#,
        304,
    ),
    (
        "find_references",
        r#"{"tag":"objects\\weapons\\rifle\\br\\projectiles\\br_bullet.projectile","direction":"in","limit":200}"#,
        261,
    ),
    (
        "find_references",
        r#"{"tag":"objects\\weapons\\rifle\\br\\projectiles\\br_bullet_hardlight.projectile","direction":"in","limit":200}"#,
        188,
    ),
];

/// Budgets are the measured size plus this share.
const MARGIN: f64 = 0.2;

#[test]
#[ignore]
fn ai_calls_stay_within_their_token_budgets() {
    let (mut s, _cache) = common::real_session("core-token-budget");
    let mut over = Vec::new();
    let mut unset = 0;
    let (mut total, mut budgets) = (0usize, 0usize);
    println!("| size | budget | tool | arguments |");
    println!("| ---: | ---: | --- | --- |");
    for (tool, args, budget) in CALLS {
        let args: serde_json::Value = serde_json::from_str(args).unwrap_or_else(|e| panic!("{tool} {args}: {e}"));
        let text = match Command::from_json(tool, args.clone()).and_then(|c| c.run(&mut s)) {
            Ok(out) => out.render(Format::Text),
            Err(e) => e.render(Format::Text),
        };
        let size = text.chars().count();
        total += size;
        budgets += budget;
        if *budget == 0 {
            unset += 1;
            println!("| {size} | set {} | {tool} | {args} |", (size as f64 * (1.0 + MARGIN)).ceil() as usize);
        } else {
            println!("| {size} | {budget} | {tool} | {args} |");
            if size > *budget {
                over.push(format!("{tool} {args}: {size} > {budget}"));
            }
        }
    }
    println!("total {total} characters (≈ {} tokens) of {budgets} budgeted; {unset} unset", total / 4);
    assert_eq!(unset, 0, "{unset} calls have no budget yet: copy the printed ones in");
    assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
}
