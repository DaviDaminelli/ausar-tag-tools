//! Reading what a module stores for one tag copy (PRD §6.2: only the header
//! and tag data sections; resource data stays untouched), and what a
//! long-running process keeps of it between calls (PRD §13). Domain only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use h5_defs::definition::Definition;
use h5_module::index::FileStamp;
use h5_module::tag::{TagError, TagHeader};
use h5_module::{GlobalId, Module, ModuleError, ModuleLists, Section, map_file};
use lru::LruCache;

use crate::catalog::{Build, Copy, Tag};

/// A tag copy as stored: its parsed header and its tag data section.
#[derive(Debug)]
pub struct StoredTag {
    pub header: TagHeader,
    pub tag_data: Vec<u8>,
    /// The decompressed header's size.
    header_size: usize,
}

impl StoredTag {
    /// The decompressed bytes it was read from (header and tag data): what it
    /// counts for in the tag cache.
    pub fn size(&self) -> usize {
        self.header_size + self.tag_data.len()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("can't open {module}: {error}")]
    Open { module: String, error: std::io::Error },
    /// The file is not the one indexed (size or time differ): its entry
    /// numbers may point elsewhere now.
    #[error("{module} changed since it was indexed")]
    Changed { module: String },
    #[error("{module}: {error}")]
    Module { module: String, error: ModuleError },
    #[error("{module}: tag header: {error}")]
    Header { module: String, error: TagError },
}

/// A cache stays usable after a panic elsewhere: its entries are always whole.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The decompressed tags most recently read, up to a size in bytes
/// (`tag_cache_mb`, PRD §6.1), shared by every build of a session.
pub struct TagCache(Mutex<TagLru>);

struct TagLru {
    tags: LruCache<TagKey, Arc<StoredTag>>,
    bytes: usize,
    limit: usize,
}

/// A build's generation and a copy in it.
type TagKey = (u64, Copy);

impl TagCache {
    pub fn new(limit_bytes: usize) -> TagCache {
        TagCache(Mutex::new(TagLru { tags: LruCache::unbounded(), bytes: 0, limit: limit_bytes }))
    }

    fn get(&self, key: TagKey) -> Option<Arc<StoredTag>> {
        lock(&self.0).tags.get(&key).cloned()
    }

    /// Keeps `tag`, dropping the least recently read tags while over the
    /// limit. A tag larger than the whole limit is not kept.
    fn insert(&self, key: TagKey, tag: Arc<StoredTag>) {
        let mut c = lock(&self.0);
        if tag.size() > c.limit {
            return;
        }
        c.bytes += tag.size();
        if let Some(old) = c.tags.put(key, tag) {
            c.bytes -= old.size();
        }
        while c.bytes > c.limit {
            let Some((_, dropped)) = c.tags.pop_lru() else { break };
            c.bytes -= dropped.size();
        }
    }
}

impl Default for TagCache {
    /// The default size (`tag_cache_mb` unset).
    fn default() -> TagCache {
        TagCache::new(crate::config::megabytes(crate::config::DEFAULT_TAG_CACHE_MB))
    }
}

/// What one build keeps between calls: the parsed lists of each module
/// (most of the cost of a small read, `docs/progress.md`), the parsed
/// definitions, and its share of the session's tag cache. Module files are
/// mapped again for each read, so none stays open between calls.
pub struct Store {
    /// Tells this build's tags apart in the shared tag cache from those of
    /// the build it replaced (`reindex`).
    generation: u64,
    lists: Mutex<HashMap<u32, Arc<ModuleLists>>>,
    /// Load manifests by module (`None`: the module has none).
    manifests: Mutex<HashMap<u32, Option<Arc<Vec<GlobalId>>>>>,
    definitions: Mutex<HashMap<PathBuf, (FileStamp, Arc<Definition>)>>,
    tags: Arc<TagCache>,
}

static GENERATIONS: AtomicU64 = AtomicU64::new(0);

impl Store {
    pub fn new(tags: Arc<TagCache>) -> Store {
        Store {
            generation: GENERATIONS.fetch_add(1, Ordering::Relaxed),
            lists: Mutex::default(),
            manifests: Mutex::default(),
            definitions: Mutex::default(),
            tags,
        }
    }

    fn lists_of(&self, module: u32, bytes: &[u8]) -> Result<Arc<ModuleLists>, ModuleError> {
        let mut lists = lock(&self.lists);
        if let Some(l) = lists.get(&module) {
            return Ok(l.clone());
        }
        let l = Arc::new(ModuleLists::parse(bytes)?);
        lists.insert(module, l.clone());
        Ok(l)
    }

    /// The parsed definition in `path`; parsed again when the file's size or
    /// modification time changes.
    pub fn definition(&self, path: &Path) -> Result<Arc<Definition>, DefinitionLoadError> {
        let meta =
            std::fs::metadata(path).map_err(|error| DefinitionLoadError::Read { path: path.to_owned(), error })?;
        let stamp = FileStamp::of(&meta);
        let mut defs = lock(&self.definitions);
        if let Some((s, d)) = defs.get(path)
            && *s == stamp
        {
            return Ok(d.clone());
        }
        let d = Arc::new(load_definition_file(path)?);
        defs.insert(path.to_owned(), (stamp, d.clone()));
        Ok(d)
    }

    #[cfg(test)]
    pub(crate) fn module_lists(&self, build: &Build, copy: Copy) -> Result<Arc<ModuleLists>, ReadError> {
        let map = map_unchanged(build, copy.module)?;
        self.lists_of(copy.module, &map).map_err(in_module(build, copy.module))
    }
}

/// The path of module `module`'s file (index position), refusing one changed
/// since it was indexed (D28).
fn unchanged_file(build: &Build, module: u32) -> Result<PathBuf, ReadError> {
    let record = &build.index.modules[module as usize];
    let path = build.module_file(record);
    let meta = std::fs::metadata(&path).map_err(|error| ReadError::Open { module: record.rel_path.clone(), error })?;
    if FileStamp::of(&meta) != record.stamp {
        return Err(ReadError::Changed { module: record.rel_path.clone() });
    }
    Ok(path)
}

fn map_unchanged(build: &Build, module: u32) -> Result<impl std::ops::Deref<Target = [u8]>, ReadError> {
    let path = unchanged_file(build, module)?;
    map_file(&path).map_err(in_module(build, module))
}

fn in_module(build: &Build, module: u32) -> impl Fn(ModuleError) -> ReadError + '_ {
    move |error| ReadError::Module { module: build.index.modules[module as usize].rel_path.clone(), error }
}

/// Runs `f` on module `module`, mapped for this call, with its kept lists.
fn with_module<R>(
    build: &Build,
    module: u32,
    f: impl FnOnce(&Module<'_>) -> Result<R, ModuleError>,
) -> Result<R, ReadError> {
    let map = map_unchanged(build, module)?;
    let run = || -> Result<R, ModuleError> {
        let lists = build.store.lists_of(module, &map)?;
        f(&Module::with_lists(&map, &lists)?)
    };
    run().map_err(in_module(build, module))
}

/// The decompressed header and tag data sections of `copy`.
fn read_sections(build: &Build, copy: Copy) -> Result<(Vec<u8>, Vec<u8>), ReadError> {
    with_module(build, copy.module, |m| {
        Ok((m.read_section(copy.entry, Section::Header)?, m.read_section(copy.entry, Section::TagData)?))
    })
}

/// The global IDs in module `module`'s load manifest (`None` without one),
/// kept after the first read; refused, like tags, once the file has changed.
pub fn read_manifest(build: &Build, module: u32) -> Result<Option<Arc<Vec<GlobalId>>>, ReadError> {
    unchanged_file(build, module)?;
    if let Some(ids) = lock(&build.store.manifests).get(&module) {
        return Ok(ids.clone());
    }
    let ids = with_module(build, module, |m| m.read_load_manifest())?.map(Arc::new);
    lock(&build.store.manifests).insert(module, ids.clone());
    Ok(ids)
}

/// Reads the header and tag data of `copy`, from its module file as indexed;
/// from memory when it was read recently and the file hasn't changed.
pub fn read_stored_tag(build: &Build, copy: Copy) -> Result<Arc<StoredTag>, ReadError> {
    let key = (build.store.generation, copy);
    // Checked first, so a kept tag of a changed module is refused too.
    unchanged_file(build, copy.module)?;
    if let Some(tag) = build.store.tags.get(key) {
        return Ok(tag);
    }
    let (header, tag_data) = read_sections(build, copy)?;
    let parsed = TagHeader::parse(&header)
        .map_err(|error| ReadError::Header { module: build.module(copy).rel_path.clone(), error })?;
    let tag = Arc::new(StoredTag { header: parsed, tag_data, header_size: header.len() });
    build.store.tags.insert(key, tag.clone());
    Ok(tag)
}

/// Whether a tag's copies hold the same data (PRD §6.4.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    Single,
    Identical,
    Differ,
}

/// Whether all of a tag's copies hold the same data (from [`copy_versions`]).
pub fn copy_identity(build: &Build, tag: &Tag) -> Result<Identity, ReadError> {
    if tag.copies.len() < 2 {
        return Ok(Identity::Single);
    }
    let versions = copy_versions(build, tag)?;
    Ok(if versions.iter().all(|&v| v == 0) { Identity::Identical } else { Identity::Differ })
}

/// Which copies of `tag` hold the same data: a version per copy, in the
/// copies' order, numbered by first appearance (the first copy is 0). Compared
/// by stored data (PRD §6.4.6): the compression block checksums when every
/// block has one (v27, no decompression), else the decompressed header and
/// tag data (v23 and blockless entries).
pub fn copy_versions(build: &Build, tag: &Tag) -> Result<Vec<usize>, ReadError> {
    if tag.copies.len() < 2 {
        return Ok(vec![0; tag.copies.len()]);
    }
    let checksums = |copy: Copy| {
        with_module(build, copy.module, |m| {
            let blocks = m.entry_blocks(copy.entry)?;
            Ok(blocks.iter().map(|b| Some((b.checksum?, b.uncompressed_size))).collect::<Option<Vec<_>>>())
        })
    };
    let sums: Vec<Option<Vec<(u64, u32)>>> = tag.copies.iter().map(|&c| checksums(c)).collect::<Result<_, _>>()?;
    if sums.iter().all(Option::is_some) {
        return Ok(versions_of(&sums));
    }
    let data: Vec<(Vec<u8>, Vec<u8>)> =
        tag.copies.iter().map(|&c| read_sections(build, c)).collect::<Result<_, _>>()?;
    Ok(versions_of(&data))
}

/// Each item's version: the position of the first equal item among the
/// distinct ones seen so far. A tag has a handful of copies.
fn versions_of<T: PartialEq>(items: &[T]) -> Vec<usize> {
    let mut firsts: Vec<&T> = Vec::new();
    items
        .iter()
        .map(|x| match firsts.iter().position(|f| *f == x) {
            Some(v) => v,
            None => {
                firsts.push(x);
                firsts.len() - 1
            }
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum DefinitionLoadError {
    #[error("can't read the definition {path}: {error}")]
    Read { path: std::path::PathBuf, error: std::io::Error },
    #[error("definition {path}: {error}")]
    Parse { path: std::path::PathBuf, error: h5_defs::definition::DefinitionError },
}

/// The parsed definition of `tag`'s class; `None` if the class has none.
pub fn load_definition(build: &Build, tag: &Tag) -> Result<Option<Arc<Definition>>, DefinitionLoadError> {
    let Some(path) = build.classes.get(&tag.class).and_then(|c| c.definition.as_ref()) else { return Ok(None) };
    build.store.definition(path).map(Some)
}

/// Parses one definition file (callers with a build use [`Store::definition`]).
pub fn load_definition_file(path: &Path) -> Result<Definition, DefinitionLoadError> {
    let xml =
        std::fs::read_to_string(path).map_err(|error| DefinitionLoadError::Read { path: path.to_owned(), error })?;
    Definition::parse(&xml).map_err(|error| DefinitionLoadError::Parse { path: path.to_owned(), error })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use h5_module::GlobalId;

    use super::*;
    use crate::testdeploy::{SecondCopy, TestDeploy, WEAPON_ID};

    fn weapon_copy(d: &TestDeploy) -> Copy {
        d.session.builds()[0].tag_by_id(GlobalId(WEAPON_ID)).unwrap().copies[0]
    }

    #[test]
    fn reads_the_header_and_the_tag_data() {
        let d = TestDeploy::new("stored-read");
        let tag = read_stored_tag(&d.session.builds()[0], weapon_copy(&d)).unwrap();
        assert_eq!(tag.header.dependencies.len(), 2);
        assert_eq!(tag.tag_data.len(), 0xB0);
        assert!(tag.header.check_tag_data(&tag.tag_data).is_empty());
    }

    #[test]
    fn refuses_a_module_changed_since_indexing() {
        let d = TestDeploy::new("stored-changed");
        let mut bytes = std::fs::read(d.module_file()).unwrap();
        bytes.extend([0; 16]);
        std::fs::write(d.module_file(), bytes).unwrap();
        let err = read_stored_tag(&d.session.builds()[0], weapon_copy(&d)).unwrap_err();
        assert!(matches!(err, ReadError::Changed { .. }), "{err}");
    }

    #[test]
    fn a_missing_module_is_an_error() {
        let d = TestDeploy::new("stored-missing");
        std::fs::remove_file(d.module_file()).unwrap();
        let err = read_stored_tag(&d.session.builds()[0], weapon_copy(&d)).unwrap_err();
        assert!(matches!(err, ReadError::Open { .. }), "{err}");
    }

    fn weapon(d: &TestDeploy) -> &crate::catalog::Tag {
        d.session.builds()[0].tag_by_id(GlobalId(WEAPON_ID)).unwrap()
    }

    #[test]
    fn loads_the_definition_of_a_tags_class() {
        let d = TestDeploy::new("stored-definition");
        let b = &d.session.builds()[0];
        let def = load_definition(b, weapon(&d)).unwrap().expect("a weapon definition");
        assert_eq!(def.fields[1].name.display, "entries");
    }

    // PRD §13: what a long-running process keeps between calls.

    #[test]
    fn a_modules_lists_are_parsed_once() {
        let d = TestDeploy::new("stored-lists-once");
        let b = &d.session.builds()[0];
        let first = b.store.module_lists(b, weapon_copy(&d)).unwrap();
        let again = b.store.module_lists(b, weapon_copy(&d)).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
    }

    #[test]
    fn a_tag_read_twice_comes_from_memory() {
        let d = TestDeploy::new("stored-tag-kept");
        let b = &d.session.builds()[0];
        let first = read_stored_tag(b, weapon_copy(&d)).unwrap();
        assert!(Arc::ptr_eq(&first, &read_stored_tag(b, weapon_copy(&d)).unwrap()));
    }

    #[test]
    fn over_its_limit_the_tag_cache_drops_the_least_recently_read_tag() {
        // Room for one copy of the weapon (header + tag data) only.
        let d = TestDeploy::with("stored-tag-limit", SecondCopy::Identical);
        let b = &d.session.builds()[0];
        let [one, two] = [weapon(&d).copies[0], weapon(&d).copies[1]];
        let size = read_stored_tag(b, one).unwrap().size();
        let d = TestDeploy::with_tag_cache("stored-tag-limit", SecondCopy::Identical, size);
        let b = &d.session.builds()[0];
        let first = read_stored_tag(b, one).unwrap();
        assert!(Arc::ptr_eq(&first, &read_stored_tag(b, one).unwrap()), "one tag fits");
        read_stored_tag(b, two).unwrap();
        assert!(!Arc::ptr_eq(&first, &read_stored_tag(b, one).unwrap()), "reading the other copy dropped it");
    }

    #[test]
    fn a_module_changed_after_its_tag_was_kept_is_still_refused() {
        let d = TestDeploy::new("stored-kept-changed");
        let b = &d.session.builds()[0];
        read_stored_tag(b, weapon_copy(&d)).unwrap();
        let mut bytes = std::fs::read(d.module_file()).unwrap();
        bytes.extend([0; 16]);
        std::fs::write(d.module_file(), bytes).unwrap();
        let err = read_stored_tag(b, weapon_copy(&d)).unwrap_err();
        assert!(matches!(err, ReadError::Changed { .. }), "{err}");
    }

    #[test]
    fn a_definition_is_parsed_again_only_when_its_file_changes() {
        let d = TestDeploy::new("stored-definition-kept");
        let b = &d.session.builds()[0];
        let first = load_definition(b, weapon(&d)).unwrap().unwrap();
        assert!(Arc::ptr_eq(&first, &load_definition(b, weapon(&d)).unwrap().unwrap()));
        let file = d.dir.0.join("defs").join("(weap)weapon.xml");
        let edited = crate::testdeploy::WEAPON_DEFINITION.replace(r#"name="count""#, r#"name="rounds""#);
        std::fs::write(&file, edited).unwrap();
        let again = load_definition(b, weapon(&d)).unwrap().unwrap();
        assert_eq!(again.fields.last().unwrap().name.display, "rounds");
    }

    /// `tag_copies` (PRD §6.6): which copies hold the same data, as versions
    /// in the copies' order (the first copy is version 0).
    #[test]
    fn copies_with_the_same_data_share_a_version() {
        let d = TestDeploy::new("stored-versions-one");
        assert_eq!(copy_versions(&d.session.builds()[0], weapon(&d)).unwrap(), [0]);
        let d = TestDeploy::with("stored-versions-same", SecondCopy::Identical);
        assert_eq!(copy_versions(&d.session.builds()[0], weapon(&d)).unwrap(), [0, 0]);
        let d = TestDeploy::with("stored-versions-differ", SecondCopy::Changed);
        assert_eq!(copy_versions(&d.session.builds()[0], weapon(&d)).unwrap(), [0, 1]);
    }

    /// PRD §6.4.6: copies are compared by their stored data (v27: block checksums).
    #[test]
    fn tells_identical_copies_from_different_ones() {
        let d = TestDeploy::new("stored-one-copy");
        assert_eq!(copy_identity(&d.session.builds()[0], weapon(&d)).unwrap(), Identity::Single);
        let d = TestDeploy::with("stored-identical", SecondCopy::Identical);
        assert_eq!(copy_identity(&d.session.builds()[0], weapon(&d)).unwrap(), Identity::Identical);
        let d = TestDeploy::with("stored-changed", SecondCopy::Changed);
        assert_eq!(copy_identity(&d.session.builds()[0], weapon(&d)).unwrap(), Identity::Differ);
    }
}
