//! One build in memory (decisions D8): tags by global ID and by path, classes
//! and copies, derived from the index. Domain only: typed inputs, typed results.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use h5_defs::{DefinitionFile, Definitions};
use h5_module::index::{EntryKind, EntryRecord, Index, ModuleRecord};
use h5_module::names::split_tag_name;
use h5_module::{FourCc, GlobalId};

use crate::config::BuildConfig;
use crate::references::ScanSlot;
use crate::stored::{Store, TagCache};

/// One copy of a tag: a module and the file entry in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Copy {
    pub module: u32,
    pub entry: u32,
}

#[derive(Clone, Debug)]
pub struct Tag {
    pub global_id: GlobalId,
    pub path: String,
    /// `path` in ASCII lowercase: same byte length, so offsets carry over.
    pub path_lower: String,
    pub class: FourCc,
    /// In module order (sorted by module path).
    pub copies: Vec<Copy>,
}

#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub code: FourCc,
    pub name: String,
    pub tag_count: usize,
    pub definition: Option<PathBuf>,
}

/// How a tag is named in a call: by global ID, or by tag path / display name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagSpec {
    Id(GlobalId),
    Name(String),
}

impl TagSpec {
    /// Text starting with `0x` is a global ID; anything else is a name.
    pub fn parse(text: &str) -> Result<TagSpec, String> {
        let text = text.trim();
        if text.starts_with("0x") || text.starts_with("0X") {
            return text.parse().map(TagSpec::Id);
        }
        Ok(TagSpec::Name(text.to_owned()))
    }
}

#[derive(Debug)]
pub enum TagLookupError<'a> {
    NotFound,
    /// The same path is used by tags of several classes.
    Ambiguous(Vec<&'a Tag>),
}

/// What is known about a class asked for by code or name.
#[derive(Debug)]
pub enum ClassLookup<'a> {
    /// Tags of this class are in the indexed modules.
    Found(&'a ClassInfo),
    /// No indexed tags, but a definition exists: the class is real.
    DefinitionOnly(&'a DefinitionFile),
    /// Neither tags nor a definition.
    Unknown,
}

pub struct Build {
    pub name: String,
    pub config: BuildConfig,
    pub index: Index,
    pub defs: Definitions,
    /// Sorted by tag path (case-insensitive), then class code.
    pub tags: Vec<Tag>,
    by_id: HashMap<GlobalId, usize>,
    by_path: HashMap<String, Vec<usize>>,
    pub classes: BTreeMap<FourCc, ClassInfo>,
    /// What is kept between calls (PRD §13).
    pub store: Store,
    /// The header scan (PRD §6.2), loaded or built on first need.
    pub(crate) scan: ScanSlot,
}

impl std::fmt::Debug for Build {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Build({}, {} modules, {} tags)", self.name, self.index.modules.len(), self.tags.len())
    }
}

/// Every tag with its copies, and the class name of each class code as found
/// in the tags' file names (decisions D13).
fn collect_tags(index: &Index) -> (Vec<Tag>, HashMap<FourCc, String>) {
    let mut tags: Vec<Tag> = Vec::new();
    let mut by_id: HashMap<GlobalId, usize> = HashMap::new();
    let mut names: HashMap<FourCc, String> = HashMap::new();
    for (m, module) in index.modules.iter().enumerate() {
        for (e, entry) in module.entries.iter().enumerate().filter(|(_, e)| e.kind() == EntryKind::Tag) {
            let copy = Copy { module: m as u32, entry: e as u32 };
            let (path, suffix) = split_tag_name(&entry.name);
            if let Some(suffix) = suffix {
                let known = names.entry(entry.class_code).or_insert_with(|| suffix.to_owned());
                if known != suffix {
                    tracing::warn!(class = %entry.class_code, first = %known, other = suffix, "class code has two names");
                }
            }
            match by_id.get(&entry.global_id) {
                Some(&t) => tags[t].copies.push(copy),
                None => {
                    by_id.insert(entry.global_id, tags.len());
                    tags.push(Tag {
                        global_id: entry.global_id,
                        path: path.to_owned(),
                        path_lower: path.to_ascii_lowercase(),
                        class: entry.class_code,
                        copies: vec![copy],
                    });
                }
            }
        }
    }
    tags.sort_by(|a, b| a.path_lower.cmp(&b.path_lower).then(a.class.cmp(&b.class)));
    (tags, names)
}

/// Class info for every class with tags. Name fallbacks: definition file
/// name, then the code (PRD §6.2).
fn collect_classes(tags: &[Tag], names: &HashMap<FourCc, String>, defs: &Definitions) -> BTreeMap<FourCc, ClassInfo> {
    let mut classes: BTreeMap<FourCc, ClassInfo> = BTreeMap::new();
    for t in tags {
        let info = classes.entry(t.class).or_insert_with(|| {
            let name = names.get(&t.class).cloned();
            let def = defs.find(t.class, name.as_deref());
            ClassInfo {
                code: t.class,
                name: name.or_else(|| def.map(|d| d.class_name.clone())).unwrap_or_else(|| t.class.to_string()),
                tag_count: 0,
                definition: def.map(|d| d.path.clone()),
            }
        });
        info.tag_count += 1;
    }
    classes
}

impl Build {
    /// A build with a tag cache of its own (the default size); a session gives
    /// all its builds one ([`Build::new_sharing`]).
    pub fn new(name: String, config: BuildConfig, index: Index, defs: Definitions) -> Build {
        Build::new_sharing(name, config, index, defs, Arc::new(TagCache::default()))
    }

    pub fn new_sharing(
        name: String,
        config: BuildConfig,
        index: Index,
        defs: Definitions,
        tag_cache: Arc<TagCache>,
    ) -> Build {
        let (tags, names) = collect_tags(&index);
        let classes = collect_classes(&tags, &names, &defs);
        let by_id = tags.iter().enumerate().map(|(i, t)| (t.global_id, i)).collect();
        let mut by_path: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, t) in tags.iter().enumerate() {
            by_path.entry(t.path_lower.clone()).or_default().push(i);
        }
        Build {
            name,
            config,
            index,
            defs,
            tags,
            by_id,
            by_path,
            classes,
            store: Store::new(tag_cache),
            scan: ScanSlot::default(),
        }
    }

    /// Where the header scan is cached (next to the index).
    pub fn with_scan_cache(mut self, path: PathBuf) -> Build {
        self.scan.cache = Some(path);
        self
    }

    pub fn scan_cache(&self) -> Option<&std::path::Path> {
        self.scan.cache.as_deref()
    }

    /// Borrowed from the class list; the code itself if the class is unknown.
    pub fn class_name(&self, code: FourCc) -> Cow<'_, str> {
        self.classes.get(&code).map_or_else(|| Cow::Owned(code.to_string()), |c| Cow::Borrowed(c.name.as_str()))
    }

    /// Tag path + `.` + class name (PRD §4).
    pub fn display_name(&self, tag: &Tag) -> String {
        format!("{}.{}", tag.path, self.class_name(tag.class))
    }

    pub fn module(&self, copy: Copy) -> &ModuleRecord {
        &self.index.modules[copy.module as usize]
    }

    /// Where `module` is on disk: its index path (`\`-separated, PRD §6.1)
    /// under the deploy folder.
    pub fn module_file(&self, module: &ModuleRecord) -> PathBuf {
        module.path_in(&self.config.deploy)
    }

    pub fn entry(&self, copy: Copy) -> &EntryRecord {
        &self.module(copy).entries[copy.entry as usize]
    }

    pub fn tag_by_id(&self, id: GlobalId) -> Option<&Tag> {
        self.by_id.get(&id).map(|&i| &self.tags[i])
    }

    /// Tags whose path equals `path` (case-insensitive).
    fn tags_by_path(&self, path: &str) -> Vec<&Tag> {
        self.by_path
            .get(&path.to_ascii_lowercase())
            .map_or_else(Vec::new, |v| v.iter().map(|&i| &self.tags[i]).collect())
    }

    /// Finds a tag by global ID, tag path or display name (PRD §6.1).
    pub fn find_tag(&self, spec: &TagSpec) -> Result<&Tag, TagLookupError<'_>> {
        let name = match spec {
            TagSpec::Id(id) => return self.tag_by_id(*id).ok_or(TagLookupError::NotFound),
            TagSpec::Name(name) => name,
        };
        let mut candidates = self.tags_by_path(name);
        if candidates.is_empty()
            && let (path, Some(class_name)) = split_tag_name(name)
        {
            candidates.extend(
                self.tags_by_path(path)
                    .into_iter()
                    .filter(|t| self.class_name(t.class).eq_ignore_ascii_case(class_name)),
            );
        }
        match candidates.len() {
            0 => Err(TagLookupError::NotFound),
            1 => Ok(candidates[0]),
            _ => Err(TagLookupError::Ambiguous(candidates)),
        }
    }

    /// Finds a class with tags by code or name (exact, case-insensitive).
    pub fn find_class(&self, query: &str) -> Option<&ClassInfo> {
        let code = FourCc::from_code(query);
        self.classes.values().find(|c| Some(c.code) == code || c.name.eq_ignore_ascii_case(query)).or_else(|| {
            // Codes are case-sensitive in files (`GobS`), but users type them in any case.
            self.classes.values().find(|c| c.code.to_string().trim_end().eq_ignore_ascii_case(query.trim_end()))
        })
    }

    /// What is known about class `query`: tags, only a definition, or nothing.
    pub fn lookup_class(&self, query: &str) -> ClassLookup<'_> {
        if let Some(c) = self.find_class(query) {
            return ClassLookup::Found(c);
        }
        let def = FourCc::from_code(query).and_then(|code| self.defs.find(code, Some(query))).or_else(|| {
            self.defs
                .files()
                .iter()
                .find(|d| d.class_name.eq_ignore_ascii_case(query) || d.file_code.eq_ignore_ascii_case(query))
        });
        def.map_or(ClassLookup::Unknown, ClassLookup::DefinitionOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_specs() {
        assert_eq!(TagSpec::parse("0x00000010"), Ok(TagSpec::Id(GlobalId(0x10))));
        assert_eq!(TagSpec::parse(" 0X10 "), Ok(TagSpec::Id(GlobalId(0x10))));
        assert_eq!(TagSpec::parse(r"a\b.weapon"), Ok(TagSpec::Name(r"a\b.weapon".into())));
        assert!(TagSpec::parse("0xZZ").is_err());
    }
}
