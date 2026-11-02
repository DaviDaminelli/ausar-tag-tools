//! A real deploy folder for tests that read tag data: one module written with
//! `h5-module`'s builders and indexed like a real build.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use h5_defs::Definitions;
use h5_module::Version;
use h5_module::index::Index;
use h5_module::testutil::{TempDir, TestEntry, build_module, build_tag, sample_tag};

use crate::catalog::Build;
use crate::config::{BuildConfig, DEFAULT_TAG_CACHE_MB, megabytes};
use crate::session::Session;
use crate::stored::TagCache;

/// The sample tag (`h5_module::testutil::sample_tag`), stored as a weapon.
pub const WEAPON: &str = r"objects\test\test.weapon";
pub const WEAPON_ID: i32 = 0x10;
/// The first of the weapons [`TestDeploy::with_weapons`] adds.
pub const MORE_WEAPON_ID: i32 = 0x30;
/// Its first dependency, indexed (listed in the tag as `objects\weapons\rifle\ar\bullet`).
/// The second (`fx\impact\bullet_hit`, an effect) is not indexed.
pub const BULLET: &str = r"objects\weapons\rifle\ar\bullet.projectile";
pub const BULLET_ID: i32 = 0x0040_1000;
/// The map [`TestDeploy::with_map`] adds (its stack).
pub const MAP: &str = r"any\levels\menu\menu";
/// Listed in the map's manifests, but no indexed tag.
pub const NOT_INDEXED_ID: i32 = 0x0077_0000;

/// The weapon's definition: the sample tag's layout (a block of tag references,
/// padding, a tag reference, a data reference and an int).
pub const WEAPON_DEFINITION: &str = r#"<plugin>
	<_field_custom name="WEAPON" />
	<_field_block_64 name="entries" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_tag_reference_64 name="target" />
	</_field_block_64>
	<_field_pad name="pad" length="4" />
	<_field_tag_reference_64 name="projectile" validtags="proj" />
	<_field_data_64 name="data" />
	<_field_long_integer name="count" />
</plugin>"#;

/// A second copy of the weapon, in an update-layer module.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SecondCopy {
    None,
    Identical,
    /// Its `count` differs.
    Changed,
}

pub struct TestDeploy {
    /// Deleted when the test ends.
    pub dir: TempDir,
    pub session: Session,
}

/// What a test deploy holds besides the weapon, its bullet and the mesh.
struct Options<'a> {
    second: SecondCopy,
    tag_cache_bytes: usize,
    /// More weapons (see [`TestDeploy::with_weapons`]).
    counts: &'a [Option<i32>],
    /// The folders the map's modules go in, if it is added.
    map: Option<&'a [&'a str]>,
    /// The weapon's definition.
    definition: &'a str,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options {
            second: SecondCopy::None,
            tag_cache_bytes: megabytes(DEFAULT_TAG_CACHE_MB),
            counts: &[],
            map: None,
            definition: WEAPON_DEFINITION,
        }
    }
}

impl TestDeploy {
    /// `name` keeps tests running in parallel apart.
    pub fn new(name: &str) -> TestDeploy {
        TestDeploy::build(name, Options::default())
    }

    pub fn with(name: &str, second: SecondCopy) -> TestDeploy {
        TestDeploy::build(name, Options { second, ..Options::default() })
    }

    /// With a tag cache of `tag_cache_bytes`.
    pub fn with_tag_cache(name: &str, second: SecondCopy, tag_cache_bytes: usize) -> TestDeploy {
        TestDeploy::build(name, Options { second, tag_cache_bytes, ..Options::default() })
    }

    /// With more weapons after the first, in the same module: `more_<i>.weapon`
    /// (global ID `MORE_WEAPON_ID + i`), each the sample weapon with `count` = `counts[i]`;
    /// `None` = a weapon whose data is no tag (it can't be read).
    pub fn with_weapons(name: &str, counts: &[Option<i32>]) -> TestDeploy {
        TestDeploy::build(name, Options { counts, ..Options::default() })
    }

    /// With a map, [`MAP`]: a base module whose load manifest lists the weapon,
    /// the bullet and [`NOT_INDEXED_ID`], and an update layer whose manifest
    /// drops the weapon.
    pub fn with_map(name: &str) -> TestDeploy {
        TestDeploy::build(name, Options { map: Some(&["any", "levels", "menu"]), ..Options::default() })
    }

    /// The same map, its modules at the top of the deploy folder.
    pub fn with_flat_map(name: &str) -> TestDeploy {
        TestDeploy::build(name, Options { map: Some(&[]), ..Options::default() })
    }

    /// With `definition` as the weapon's definition instead of [`WEAPON_DEFINITION`].
    pub fn with_definition(name: &str, definition: &str) -> TestDeploy {
        TestDeploy::build(name, Options { definition, ..Options::default() })
    }

    fn build(name: &str, o: Options<'_>) -> TestDeploy {
        let Options { second, tag_cache_bytes, counts, map, definition } = o;
        let dir = TempDir::new(&format!("core-{name}"));
        let deploy = dir.0.join("deploy");
        fs::create_dir_all(deploy.join("any").join("levels")).unwrap();
        // The weapon owns one resource (file entry 2), loaded by its `__default__` zoneset.
        let mut tag = sample_tag();
        tag.zonesets[0].2 = vec![0];
        let (header, tag_data) = build_tag(&tag);
        let mut weapon = TestEntry::tag(WEAPON, WEAPON_ID, "weap", &tag_data);
        weapon.sections.0 = header;
        weapon.resources = vec![2];
        let bullet = TestEntry::tag(BULLET, BULLET_ID, "proj", b"bullet data");
        let mesh = TestEntry::resource(&format!("{WEAPON}[0:mesh resource]"), 0, b"mesh data");
        let mut entries = vec![weapon, bullet, mesh];
        for (i, count) in counts.iter().enumerate() {
            let path = format!(r"objects\test\more_{i}.weapon");
            let id = MORE_WEAPON_ID + i as i32;
            let Some(count) = count else {
                entries.push(TestEntry::tag(&path, id, "weap", b"not a tag"));
                continue;
            };
            let mut more = tag.clone();
            more.blocks[0][0x5C..0x60].copy_from_slice(&count.to_le_bytes());
            let (header, tag_data) = build_tag(&more);
            let mut e = TestEntry::tag(&path, id, "weap", &tag_data);
            e.sections.0 = header;
            entries.push(e);
        }
        let module = build_module(Version::V27, &entries, &[]);
        fs::write(deploy.join("any").join("levels").join("test-rtx-1.module"), module).unwrap();
        if second != SecondCopy::None {
            if second == SecondCopy::Changed {
                tag.blocks[0][0x5C..0x60].copy_from_slice(&9i32.to_le_bytes());
            }
            let (header, tag_data) = build_tag(&tag);
            let mut copy = TestEntry::tag(WEAPON, WEAPON_ID, "weap", &tag_data);
            copy.sections.0 = header;
            let module = build_module(Version::V27, &[copy], &[]);
            fs::write(deploy.join("any").join("levels").join("test-rtx-11587-1.module"), module).unwrap();
        }
        if let Some(folders) = map {
            let folder = folders.iter().fold(deploy.clone(), |path, f| path.join(f));
            fs::create_dir_all(&folder).unwrap();
            for (layer, ids) in
                [("1", &[WEAPON_ID, BULLET_ID, NOT_INDEXED_ID][..]), ("11587-1", &[BULLET_ID, NOT_INDEXED_ID])]
            {
                let manifest: Vec<u8> = ids.iter().flat_map(|id| id.to_le_bytes()).collect();
                let module = build_module(Version::V27, &[TestEntry::raw("loadmanifest", &manifest)], ids);
                fs::write(folder.join(format!("menu-rtx-{layer}.module")), module).unwrap();
            }
        }
        let defs = dir.0.join("defs");
        fs::create_dir_all(&defs).unwrap();
        fs::write(defs.join("(weap)weapon.xml"), definition).unwrap();

        let (index, _) = Index::update(&deploy, None, &|_| false).unwrap();
        let definitions = Definitions::load(&defs).unwrap();
        let cfg = BuildConfig { name: "forge".into(), deploy, defs, resource_defs: None };
        let build =
            Build::new_sharing("forge".into(), cfg, index, definitions, Arc::new(TagCache::new(tag_cache_bytes)))
                .with_scan_cache(dir.0.join("cache").join("forge.scan"))
                .with_check_cache(dir.0.join("cache").join("forge.check"));
        TestDeploy { session: Session::from_builds(dir.0.join("cache"), vec![build]), dir }
    }

    pub fn module_file(&self) -> PathBuf {
        self.dir.0.join("deploy").join("any").join("levels").join("test-rtx-1.module")
    }
}
