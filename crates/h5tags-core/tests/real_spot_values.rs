//! PRD §9 check 9: values the user knows from playing, read with `read_tag`
//! as the AI reads them. Forge build only; ignored by default.
//!
//! ```text
//! H5_DEPLOY=<Forge deploy folder> cargo test --release -p h5tags-core --test real_spot_values -- --ignored
//! ```
//!
//! Tags are named by global ID (game file names stay out of code).

use std::path::PathBuf;

use h5_module::testutil::TempDir;
use h5tags_core::tag_view::{Detail, Options};
use h5tags_core::tools::args::BuildArg;
use h5tags_core::tools::{ReadTag, Tool};
use h5tags_core::{Config, Format, Session};
use serde_json::Value;

const BATTLE_RIFLE: &str = "0x00008593";
const ASSAULT_RIFLE: &str = "0x00008595";
const COVENANT_CARBINE: &str = "0x00073246";
const RAILGUN: &str = "0x0000D3C7";
const GRENADE_LIST: &str = "0x00000057";

fn session() -> (Session, TempDir) {
    let deploy = PathBuf::from(std::env::var("H5_DEPLOY").expect("set H5_DEPLOY to the Forge deploy folder"));
    let defs = std::env::var("H5_DEFS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/H5Tags/tags"));
    let cache = TempDir::new("core-spot-values");
    (Session::open(Config::single(deploy, defs, None, Some(cache.0.clone()))).unwrap(), cache)
}

/// The value of each field `path` selects in `tag` (JSON `value`).
fn values(tag: &str, path: &str) -> Vec<Value> {
    let (mut s, _cache) = session();
    let t = ReadTag {
        tag: tag.into(),
        path: Some(path.into()),
        section: None,
        depth: 0,
        detail: Detail::Normal,
        options: Options::None,
        show_hidden: false,
        module: None,
        build: BuildArg::default(),
    };
    let out = t.run(&mut s).unwrap_or_else(|e| panic!("{tag} {path}: {e} ({:?})", e.hint));
    let v: Value = serde_json::from_str(&out.render(Format::Json)).unwrap();
    // fields[0] is the `(path)` heading; then one node per selected field.
    v["fields"].as_array().unwrap().iter().skip(1).map(|f| f["value"].clone()).collect()
}

fn one(tag: &str, path: &str) -> Value {
    let v = values(tag, path);
    assert_eq!(v.len(), 1, "{tag} {path}: {v:?}");
    v[0].clone()
}

#[test]
#[ignore]
fn battle_rifle_has_36_in_a_magazine_and_fires_3_round_bursts() {
    assert_eq!(one(BATTLE_RIFLE, "magazines[0].rounds loaded maximum"), 36);
    // The burst is the shots per trigger pull, not `rounds per burst` (0 on every weapon).
    assert_eq!(one(BATTLE_RIFLE, "barrels[0].firing.shots per fire"), serde_json::json!({ "min": 3, "max": 3 }));
}

#[test]
#[ignore]
fn assault_rifle_has_36_in_a_magazine() {
    assert_eq!(one(ASSAULT_RIFLE, "magazines[0].rounds loaded maximum"), 36);
}

#[test]
#[ignore]
fn covenant_carbine_has_20_in_a_magazine() {
    assert_eq!(one(COVENANT_CARBINE, "magazines[0].rounds loaded maximum"), 20);
}

#[test]
#[ignore]
fn railgun_holds_one_shot() {
    assert_eq!(one(RAILGUN, "magazines[0].rounds loaded maximum"), 1);
}

#[test]
#[ignore]
fn players_carry_at_most_2_grenades_of_each_type() {
    for mode in ["campaign", "firefight", "multiplayer"] {
        let max = values(GRENADE_LIST, &format!("grenades[*].maximum count ({mode}).Base Value"));
        assert_eq!(max.len(), 3, "three grenade types ({mode})");
        assert!(max.iter().all(|v| v == 2), "{mode}: {max:?}");
    }
}
