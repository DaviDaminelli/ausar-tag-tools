//! A real deploy folder for tests that read tag data: one module written with
//! `h5-module`'s builders and indexed like a real build.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use h5_defs::Definitions;
use h5_module::Version;
use h5_module::index::Index;
use h5_module::murmur3::murmur3_x86_32;
use h5_module::testutil::{TempDir, TestEntry, TestTag, build_module, build_tag, sample_tag};

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

/// The first script tag [`TestDeploy::with_scripts`] adds: `scripts\script_<i>.lua`
/// (class `hsc*`), global ID `SCRIPT_ID + i`.
pub const SCRIPT_ID: i32 = 0x50;

/// An `hsc*` definition with the sample tag's layout: its data reference is
/// `source file.source`, where the real `hsc*` definition keeps the script.
pub const SCRIPT_DEFINITION: &str = r#"<plugin>
	<_field_block_64 name="entries" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_tag_reference_64 name="target" />
	</_field_block_64>
	<_field_pad name="pad" length="4" />
	<_field_tag_reference_64 name="projectile" />
	<_field_struct name="source file">
		<_field_data_64 name="source" />
	</_field_struct>
	<_field_long_integer name="count" />
</plugin>"#;

/// The first text tag [`TestDeploy::with_texts`] adds:
/// `ui\strings\list_<i>.multilingual_unicode_string_list` (class `unic`),
/// global ID `TEXT_ID + i`.
pub const TEXT_ID: i32 = 0x70;

/// A `unic` definition with the sample tag's layout and three of the real
/// definition's languages: `string references` (a string_id and one offset per
/// language) in the block, `string data utf8` in the data reference.
pub const TEXT_DEFINITION: &str = r#"<plugin>
	<_field_block_64 name="string references*" srcguid="02020202-0202-0202-0202-020202020202">
		<_field_string_id name="string id^*" />
		<_field_long_integer name="english offset*" />
		<_field_long_integer name="german offset*" />
		<_field_long_integer name="french offset*" />
	</_field_block_64>
	<_field_pad name="pad" length="4" />
	<_field_tag_reference_64 name="projectile" />
	<_field_data_64 name="string data utf8*" />
	<_field_long_integer name="count" />
</plugin>"#;

/// A test `unic` tag's string references: (string_id name, its english,
/// german and french texts; `None` = no text in that language).
pub type TestTexts<'a> = &'a [(&'a str, [Option<&'a str>; 3])];

/// A second copy of the weapon, in an update-layer module.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SecondCopy {
    None,
    Identical,
    /// Its `count` differs.
    Changed,
    /// Its main struct is 4 bytes longer (zeros after `count`): a layout
    /// that changed in an update.
    Grown,
}

pub struct TestDeploy {
    /// Deleted when the test ends.
    pub dir: TempDir,
    pub session: Session,
}

/// Two test builds in one session ([`TestDeploy::two_builds`]).
pub struct TwoBuilds {
    pub session: Session,
    _dirs: [TempDir; 2],
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
    /// Script tags' scripts (see [`TestDeploy::with_scripts`]).
    scripts: &'a [&'a [u8]],
    /// Text tags' string references (see [`TestDeploy::with_texts`]).
    texts: &'a [TestTexts<'a>],
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options {
            second: SecondCopy::None,
            tag_cache_bytes: megabytes(DEFAULT_TAG_CACHE_MB),
            counts: &[],
            map: None,
            definition: WEAPON_DEFINITION,
            scripts: &[],
            texts: &[],
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

    /// Both: a second copy, and `definition`.
    pub fn with_layers_and_definition(name: &str, second: SecondCopy, definition: &str) -> TestDeploy {
        TestDeploy::build(name, Options { second, definition, ..Options::default() })
    }

    /// With script tags after the weapons, in the same module: `scripts\script_<i>.lua`
    /// (class `hsc*`, global ID `SCRIPT_ID + i`), each the sample tag with
    /// `scripts[i]` as the data its `source file.source` points to.
    pub fn with_scripts(name: &str, scripts: &[&[u8]]) -> TestDeploy {
        TestDeploy::build(name, Options { scripts, ..Options::default() })
    }

    /// With text tags after the scripts, in the same module (see [`TEXT_ID`]),
    /// each holding `texts[i]`, its texts stored in order.
    pub fn with_texts(name: &str, texts: &[TestTexts<'_>]) -> TestDeploy {
        TestDeploy::build(name, Options { texts, ..Options::default() })
    }

    fn build(name: &str, o: Options<'_>) -> TestDeploy {
        let Options { second, tag_cache_bytes, counts, map, definition, scripts, texts } = o;
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
        for (i, script) in scripts.iter().enumerate() {
            let mut t = sample_tag();
            t.blocks[2] = script.to_vec();
            let (header, tag_data) = build_tag(&t);
            let mut e = TestEntry::tag(&format!(r"scripts\script_{i}.lua"), SCRIPT_ID + i as i32, "hsc*", &tag_data);
            e.sections.0 = header;
            entries.push(e);
        }
        for (i, list) in texts.iter().enumerate() {
            let (header, tag_data) = build_tag(&text_tag(list));
            let path = format!(r"ui\strings\list_{i}.multilingual_unicode_string_list");
            let mut e = TestEntry::tag(&path, TEXT_ID + i as i32, "unic", &tag_data);
            e.sections.0 = header;
            entries.push(e);
        }
        let module = build_module(Version::V27, &entries, &[]);
        fs::write(deploy.join("any").join("levels").join("test-rtx-1.module"), module).unwrap();
        if second != SecondCopy::None {
            if second == SecondCopy::Changed {
                tag.blocks[0][0x5C..0x60].copy_from_slice(&9i32.to_le_bytes());
            }
            if second == SecondCopy::Grown {
                tag.blocks[0].extend([0u8; 4]);
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
        if !scripts.is_empty() {
            fs::write(defs.join("(hsc_)lua.xml"), SCRIPT_DEFINITION).unwrap();
        }
        if !texts.is_empty() {
            fs::write(defs.join("(unic)multilingual_unicode_string_list.xml"), TEXT_DEFINITION).unwrap();
        }

        let (index, _) = Index::update(&deploy, None, &|_| false).unwrap();
        let definitions = Definitions::load(&defs).unwrap();
        let cfg = BuildConfig { name: "forge".into(), deploy, defs, resource_defs: None };
        let build =
            Build::new_sharing("forge".into(), cfg, index, definitions, Arc::new(TagCache::new(tag_cache_bytes)))
                .with_scan_cache(dir.0.join("cache").join("forge.scan"))
                .with_check_cache(dir.0.join("cache").join("forge.check"));
        TestDeploy { session: Session::from_builds(dir.0.join("cache"), vec![build]), dir }
    }

    /// Two builds in one session (M6): `forge` with more weapons `a`, `xbox`
    /// with more weapons `b` (see [`TestDeploy::with_weapons`]); the folders
    /// are kept for the session's life.
    pub fn two_builds(name: &str, a: &[Option<i32>], b: &[Option<i32>]) -> TwoBuilds {
        TestDeploy::two_builds_with_definition(name, a, b, WEAPON_DEFINITION)
    }

    /// [`TestDeploy::two_builds`], `xbox` with `definition` as the weapon's definition.
    pub fn two_builds_with_definition(name: &str, a: &[Option<i32>], b: &[Option<i32>], definition: &str) -> TwoBuilds {
        let first = TestDeploy::with_weapons(&format!("{name}-a"), a);
        let second = TestDeploy::build(&format!("{name}-b"), Options { counts: b, definition, ..Options::default() });
        let mut builds = first.session.into_builds();
        let mut xbox = second.session.into_builds().remove(0);
        xbox.name = "xbox".into();
        xbox.config.name = "xbox".into();
        builds.push(xbox);
        TwoBuilds { session: Session::from_builds(first.dir.0.join("cache"), builds), _dirs: [first.dir, second.dir] }
    }

    pub fn module_file(&self) -> PathBuf {
        self.dir.0.join("deploy").join("any").join("levels").join("test-rtx-1.module")
    }
}

/// The sample tag holding `list`'s string references in its block (16 bytes
/// each: the string_id's hash, then three offsets, -1 = none) and its texts,
/// NUL-terminated, in its data. The block's own tag references are dropped:
/// they would overwrite the entries.
fn text_tag(list: TestTexts<'_>) -> TestTag {
    let mut t = sample_tag();
    let (mut refs, mut data) = (Vec::new(), Vec::new());
    for (name, texts) in list {
        refs.extend(murmur3_x86_32(name.as_bytes(), 0).to_le_bytes());
        for text in texts {
            let offset = text.map_or(-1, |s| {
                let at = data.len() as i32;
                data.extend(s.as_bytes());
                data.push(0);
                at
            });
            refs.extend(offset.to_le_bytes());
        }
        t.string_ids.push((*name).to_owned());
    }
    t.blocks[1] = refs;
    t.blocks[2] = data;
    t.structs[1].5 = list.len() as i32;
    t.tag_refs.truncate(1);
    t
}
