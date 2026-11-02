use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::index::{Index, UpdateReport};
use crate::testutil::{TempDir, TestEntry, build_module, build_tag, sample_tag};
use crate::{FourCc, GlobalId, Version};

const WEAPON_ID: i32 = 0x10;
const BROKEN_ID: i32 = 0x11;

fn class(code: &str) -> FourCc {
    FourCc::from_code(code).unwrap()
}

/// One module: a weapon (the sample tag) and a tag whose header doesn't
/// parse. The weapon's `entries[0]` reference has no dependency entry; with
/// `undeclared` it names that tag anyway (Q12; the builder writes -1 for such
/// a field, so its ID and class are patched into the tag data), else it is null.
fn write_module(deploy: &Path, undeclared: Option<i32>) {
    let (header, mut tag_data) = build_tag(&sample_tag());
    if let Some(id) = undeclared {
        // entries[0].target: block 1 (after block 0's 0x60 bytes), offset 0; ID at +0x0C, class at +0x18.
        tag_data[0x60 + 0x0C..0x60 + 0x10].copy_from_slice(&id.to_le_bytes());
        tag_data[0x60 + 0x18..0x60 + 0x1C].copy_from_slice(&class("bitm").0.to_le_bytes());
    }
    let mut weapon = TestEntry::tag(r"objects\test\test.weapon", WEAPON_ID, "weap", &tag_data);
    weapon.sections.0 = header;
    let broken = TestEntry::tag(r"objects\test\broken.weapon", BROKEN_ID, "weap", b"not a tag");
    fs::write(deploy.join("test-rtx-1.module"), build_module(Version::V27, &[weapon, broken], &[])).unwrap();
}

fn deploy(dir: &TempDir, undeclared: Option<i32>) -> PathBuf {
    let deploy = dir.0.join("deploy");
    fs::create_dir_all(&deploy).unwrap();
    write_module(&deploy, undeclared);
    deploy
}

fn scan(deploy: &Path, previous: Option<HeaderScan>) -> (HeaderScan, UpdateReport) {
    let (index, _) = Index::update(deploy, None, &|_| false).unwrap();
    HeaderScan::update(deploy, &index, previous, &|_| false, &|_| {})
}

fn weapon(s: &HeaderScan) -> &TagScan {
    s.modules[0].tags.iter().find(|t| t.global_id == GlobalId(WEAPON_ID)).unwrap()
}

/// PRD §6.2, Q12: every non-null reference field, from the tag data, whether
/// or not it has a dependency entry; null ones are left out.
#[test]
fn a_scan_keeps_every_reference_field_with_its_target() {
    let dir = TempDir::new("scan-references");
    let (s, report) = scan(&deploy(&dir, Some(0x0040_3000)), None);
    assert_eq!((report.read, report.reused, report.failed), (1, 0, 0));
    assert_eq!(s.modules[0].rel_path, "test-rtx-1.module");
    let refs: Vec<(GlobalId, FourCc, u32, bool)> =
        weapon(&s).references.iter().map(|r| (r.target, r.class, r.reference, r.has_dependency)).collect();
    assert_eq!(
        refs,
        [
            (GlobalId(0x0040_1000), class("proj"), 0, true),
            (GlobalId(0x0040_3000), class("bitm"), 1, false),
            (GlobalId(0x0040_2000), class("effe"), 2, true),
        ]
    );
    assert_eq!(weapon(&s).dependencies, [GlobalId(0x0040_1000), GlobalId(0x0040_2000)]);
    let names: Vec<String> =
        weapon(&s).references.iter().map(|r| weapon(&s).listed_name(r).unwrap_or_default().into_owned()).collect();
    assert_eq!(names, [r"objects\weapons\rifle\ar\bullet", "", r"fx\impact\bullet_hit"]);
    assert!(String::from_utf8_lossy(&weapon(&s).strings).contains(r"objects\weapons\rifle\ar\bullet"));
    // A tag whose header doesn't parse is recorded, not dropped silently.
    let problems: Vec<GlobalId> = s.modules[0].problems.iter().map(|p| p.0).collect();
    assert_eq!(problems, [GlobalId(BROKEN_ID)]);

    let dir = TempDir::new("scan-null");
    let (s, _) = scan(&deploy(&dir, None), None);
    assert_eq!(weapon(&s).references.iter().map(|r| r.reference).collect::<Vec<_>>(), [0, 2]);
}

#[test]
fn an_update_rescans_only_changed_modules_and_round_trips_its_cache() {
    let dir = TempDir::new("scan-update");
    let deploy = deploy(&dir, Some(0x0040_3000));
    let (first, _) = scan(&deploy, None);
    let (again, report) = scan(&deploy, Some(first.clone()));
    assert_eq!((report.read, report.reused), (0, 1));
    assert_eq!(again, first);

    // Another file (another size): rescanned.
    write_module(&deploy, Some(0x0040_4000));
    let mut bytes = fs::read(deploy.join("test-rtx-1.module")).unwrap();
    bytes.extend([0; 16]);
    fs::write(deploy.join("test-rtx-1.module"), bytes).unwrap();
    let (changed, report) = scan(&deploy, Some(again));
    assert_eq!((report.read, report.reused), (1, 0));
    assert_eq!(weapon(&changed).references[1].target, GlobalId(0x0040_4000));

    let cache = dir.0.join("cache").join("default.scan");
    changed.save(&cache).unwrap();
    assert_eq!(HeaderScan::load(&cache), Some(changed));
}
