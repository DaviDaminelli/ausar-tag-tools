//! A real deploy folder for tests that read tag data: one module written with
//! `h5-module`'s builders and indexed like a real build.

use std::fs;
use std::path::PathBuf;

use h5_defs::Definitions;
use h5_module::Version;
use h5_module::index::Index;
use h5_module::testutil::{TempDir, TestEntry, build_module, build_tag, sample_tag};

use crate::catalog::Build;
use crate::config::BuildConfig;
use crate::session::Session;

/// The sample tag (`h5_module::testutil::sample_tag`), stored as a weapon.
pub const WEAPON: &str = r"objects\test\test.weapon";
pub const WEAPON_ID: i32 = 0x10;
/// Its first dependency, indexed (listed in the tag as `objects\weapons\rifle\ar\bullet`).
/// The second (`fx\impact\bullet_hit`, an effect) is not indexed.
pub const BULLET: &str = r"objects\weapons\rifle\ar\bullet.projectile";

pub struct TestDeploy {
    /// Deleted when the test ends.
    pub dir: TempDir,
    pub session: Session,
}

impl TestDeploy {
    /// `name` keeps tests running in parallel apart.
    pub fn new(name: &str) -> TestDeploy {
        let dir = TempDir::new(&format!("core-{name}"));
        let deploy = dir.0.join("deploy");
        fs::create_dir_all(deploy.join("any").join("levels")).unwrap();
        let (header, tag_data) = build_tag(&sample_tag());
        let mut weapon = TestEntry::tag(WEAPON, WEAPON_ID, "weap", &tag_data);
        weapon.sections.0 = header;
        let bullet = TestEntry::tag(BULLET, 0x0040_1000, "proj", b"bullet data");
        let module = build_module(Version::V27, &[weapon, bullet], &[]);
        fs::write(deploy.join("any").join("levels").join("test-rtx-1.module"), module).unwrap();

        let (index, _) = Index::update(&deploy, None, &|_| false).unwrap();
        let cfg = BuildConfig { name: "forge".into(), deploy, defs: "defs".into(), resource_defs: None };
        let build = Build::new("forge".into(), cfg, index, Definitions::from_files(vec![]));
        TestDeploy { session: Session::from_builds(dir.0.join("cache"), vec![build]), dir }
    }

    pub fn module_file(&self) -> PathBuf {
        self.dir.0.join("deploy").join("any").join("levels").join("test-rtx-1.module")
    }
}
