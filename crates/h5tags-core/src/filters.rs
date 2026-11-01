//! The common filters (PRD §6.3): one meaning everywhere, implemented once
//! (decisions D9). Domain values: tools parse their inputs into these.

use std::str::FromStr;

use h5_module::FourCc;
use h5_module::index::{EntryKind, EntryRecord};
use regex::{Regex, RegexBuilder};
use serde::Deserialize;

use crate::catalog::{Build, Tag};

/// How `query` matches a name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Case-insensitive substring of the tag path.
    #[default]
    Default,
    /// Rust regex, case-insensitive, against the tag path.
    Regex,
    /// The tag path or display name equals the query (case-insensitive).
    Exact,
}

/// A compiled `query` + `mode`.
#[derive(Clone)]
pub enum NameMatcher {
    Contains(String),
    Regex(Regex),
    Exact(String),
}

impl NameMatcher {
    pub fn new(query: &str, mode: Mode) -> Result<NameMatcher, String> {
        Ok(match mode {
            Mode::Default => NameMatcher::Contains(query.to_lowercase()),
            Mode::Exact => NameMatcher::Exact(query.to_lowercase()),
            Mode::Regex => NameMatcher::Regex(
                RegexBuilder::new(query).case_insensitive(true).build().map_err(|e| format!("invalid regex: {e}"))?,
            ),
        })
    }

    /// Matches a tag path, or for `exact` also its display name.
    pub fn matches(&self, path: &str, display_name: &str) -> bool {
        match self {
            NameMatcher::Contains(q) => path.to_lowercase().contains(q),
            NameMatcher::Regex(r) => r.is_match(path),
            NameMatcher::Exact(q) => path.eq_ignore_ascii_case(q) || display_name.eq_ignore_ascii_case(q),
        }
    }

    /// The query text of a `default`-mode matcher.
    pub fn plain_query(&self) -> Option<&str> {
        match self {
            NameMatcher::Contains(q) => Some(q),
            _ => None,
        }
    }
}

/// A size in bytes: `123`, `500KB`, `10 MB`, `1.5GB` (decimal units).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Size(pub u64);

impl FromStr for Size {
    type Err = String;

    fn from_str(s: &str) -> Result<Size, String> {
        let s = s.trim();
        let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
        let (number, unit) = (s[..split].trim(), s[split..].trim());
        let scale = match unit.to_ascii_uppercase().as_str() {
            "" | "B" => 1.0,
            "KB" => 1e3,
            "MB" => 1e6,
            "GB" => 1e9,
            _ => return Err(format!("unknown size unit '{unit}' (use B, KB, MB or GB)")),
        };
        let n: f64 = number.parse().map_err(|_| format!("invalid size '{s}' (e.g. 500KB, 10MB)"))?;
        if !n.is_finite() || n < 0.0 {
            return Err(format!("invalid size '{s}'"));
        }
        Ok(Size((n * scale).round() as u64))
    }
}

impl<'de> Deserialize<'de> for Size {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Size, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bytes(u64),
            Text(String),
        }
        match Raw::deserialize(d)? {
            Raw::Bytes(n) => Ok(Size(n)),
            Raw::Text(s) => s.parse().map_err(serde::de::Error::custom),
        }
    }
}

/// `min_size` / `max_size`, inclusive.
#[derive(Clone, Copy, Debug, Default)]
pub struct SizeRange {
    pub min: Option<Size>,
    pub max: Option<Size>,
}

impl SizeRange {
    pub fn contains(&self, bytes: u64) -> bool {
        self.min.is_none_or(|m| bytes >= m.0) && self.max.is_none_or(|m| bytes <= m.0)
    }
}

/// `module`: case-insensitive substring of the module path.
#[derive(Clone, Debug)]
pub struct ModuleFilter(String);

impl ModuleFilter {
    pub fn new(text: &str) -> ModuleFilter {
        ModuleFilter(text.to_lowercase())
    }

    pub fn matches(&self, module_path: &str) -> bool {
        module_path.to_lowercase().contains(&self.0)
    }
}

/// An entry's kind (PRD §6.3): its class code for a tag, its resource kind
/// without markers for a resource (D12), `file` otherwise.
pub fn entry_kind(e: &EntryRecord) -> String {
    match e.kind() {
        EntryKind::Tag => e.class_code.to_string(),
        EntryKind::Resource => e.resource_name().map_or_else(|| "resource".to_owned(), |r| r.kind.to_owned()),
        EntryKind::File => "file".to_owned(),
    }
}

/// `kind`: a class (code or name), a resource kind, or `file`.
pub struct KindFilter {
    class: Option<FourCc>,
    text: String,
}

impl KindFilter {
    pub fn new(build: &Build, kind: &str) -> KindFilter {
        KindFilter { class: build.find_class(kind).map(|c| c.code), text: kind.trim().to_lowercase() }
    }

    pub fn matches(&self, e: &EntryRecord) -> bool {
        match e.kind() {
            EntryKind::Tag => self.class == Some(e.class_code),
            EntryKind::Resource => entry_kind(e).to_lowercase() == self.text,
            EntryKind::File => self.text == "file",
        }
    }

    /// True if the kind names a class with tags, or `file`: then an empty
    /// result is a plain fact, not a sign the kind doesn't exist.
    pub fn is_known_kind(&self) -> bool {
        self.class.is_some() || self.text == "file"
    }
}

/// The name part of the filters: `query` + `mode`, and for `search_tags` also
/// Reclaimer's rule that the query may be a class code or name (PRD §5.5).
#[derive(Clone)]
pub struct NameFilter {
    pub matcher: NameMatcher,
    pub or_class: Option<FourCc>,
}

impl NameFilter {
    pub fn new(matcher: NameMatcher) -> NameFilter {
        NameFilter { matcher, or_class: None }
    }

    /// Reclaimer's search rule: in `default` mode, a query equal to a class
    /// code or name also matches every tag of that class.
    pub fn reclaimer(build: &Build, matcher: NameMatcher) -> NameFilter {
        let or_class = matcher.plain_query().and_then(|q| build.find_class(q)).map(|c| c.code);
        NameFilter { matcher, or_class }
    }
}

/// The common filters, all optional; every one given must match (AND).
#[derive(Default)]
pub struct Filters {
    pub name: Option<NameFilter>,
    pub class: Option<FourCc>,
    pub kind: Option<KindFilter>,
    pub module: Option<ModuleFilter>,
    pub size: SizeRange,
}

impl Filters {
    /// For tags: name, class and module (a tag matches if any copy is in a
    /// matching module).
    pub fn matches_tag(&self, build: &Build, tag: &Tag) -> bool {
        self.class.is_none_or(|c| tag.class == c)
            && self.module.as_ref().is_none_or(|m| tag.copies.iter().any(|&c| m.matches(&build.module(c).rel_path)))
            && self
                .name
                .as_ref()
                .is_none_or(|n| n.or_class == Some(tag.class) || n.matcher.matches(&tag.path, &build.display_name(tag)))
    }

    /// For file entries: name (a resource matches by its owner's tag path),
    /// kind, module and size.
    pub fn matches_entry(&self, module_path: &str, e: &EntryRecord) -> bool {
        self.module.as_ref().is_none_or(|m| m.matches(module_path))
            && self.kind.as_ref().is_none_or(|k| k.matches(e))
            && self.size.contains(e.uncompressed_size)
            && self.name.as_ref().is_none_or(|n| n.matcher.matches(e.tag_path(), e.owner_name()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse() {
        assert_eq!("123".parse(), Ok(Size(123)));
        assert_eq!("500KB".parse(), Ok(Size(500_000)));
        assert_eq!("10 mb".parse(), Ok(Size(10_000_000)));
        assert_eq!("1.5GB".parse(), Ok(Size(1_500_000_000)));
        assert_eq!("7B".parse(), Ok(Size(7)));
        assert!("10XB".parse::<Size>().is_err());
        assert!("abc".parse::<Size>().is_err());
        assert!("-1MB".parse::<Size>().is_err());
    }

    #[test]
    fn sizes_deserialize_from_number_or_text() {
        assert_eq!(serde_json::from_str::<Size>("2048").unwrap(), Size(2048));
        assert_eq!(serde_json::from_str::<Size>("\"2KB\"").unwrap(), Size(2000));
    }

    #[test]
    fn size_range() {
        let range = |min: Option<u64>, max: Option<u64>| SizeRange { min: min.map(Size), max: max.map(Size) };
        assert!(range(None, None).contains(5));
        assert!(range(Some(5), Some(5)).contains(5));
        assert!(!range(Some(5), None).contains(4));
        assert!(!range(None, Some(5)).contains(6));
    }

    #[test]
    fn name_modes() {
        let path = r"objects\weapons\pistol\magnum\magnum";
        let display = r"objects\weapons\pistol\magnum\magnum.weapon";
        assert!(NameMatcher::new("MAGNUM", Mode::Default).unwrap().matches(path, display));
        assert!(NameMatcher::new(r"magnum|pistol", Mode::Regex).unwrap().matches(path, display));
        assert!(!NameMatcher::new(r"^magnum", Mode::Regex).unwrap().matches(path, display));
        assert!(NameMatcher::new(display, Mode::Exact).unwrap().matches(path, display));
        assert!(NameMatcher::new(&path.to_uppercase(), Mode::Exact).unwrap().matches(path, display));
        assert!(!NameMatcher::new("magnum", Mode::Exact).unwrap().matches(path, display));
        assert!(NameMatcher::new("(", Mode::Regex).is_err());
    }

    #[test]
    fn module_filter() {
        assert!(ModuleFilter::new("GLOBALS").matches(r"any\levels\globals-rtx-1.module"));
        assert!(!ModuleFilter::new("y").matches("x"));
    }
}
