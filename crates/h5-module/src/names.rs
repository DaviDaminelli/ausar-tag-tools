//! File name conventions (verified on v23, `docs/format-notes.md` "File names"):
//! - tag: `<tag path>.<class name>`
//! - resource: `<owner file name>[<index>:<kind>]`

/// The file listing the global IDs a map loads (AusarDocs `Module.md`).
pub const LOAD_MANIFEST: &str = "loadmanifest";

/// Splits a tag's file name into tag path and the class name suffix after the
/// last `.` of the last path component. Names without a suffix return `None`.
pub fn split_tag_name(name: &str) -> (&str, Option<&str>) {
    let last_sep = name.rfind('\\').map_or(0, |i| i + 1);
    match name[last_sep..].rfind('.') {
        Some(dot) if dot > 0 && last_sep + dot + 1 < name.len() => {
            let dot = last_sep + dot;
            (&name[..dot], Some(&name[dot + 1..]))
        }
        _ => (name, None),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceName<'a> {
    /// The owning tag's file name.
    pub owner: &'a str,
    pub index: u32,
    /// The kind as stored, with any trailing markers (`mesh resource!*`).
    pub raw_kind: &'a str,
    /// The kind without trailing markers (`mesh resource`).
    pub kind: &'a str,
}

/// Name markers, as in definition names (PRD §5.3). Their meaning is unconfirmed.
const MARKERS: &[char] = &['^', '*', '!', '~', '&'];

/// Parses `<owner>[<index>:<kind>]`. Returns `None` if the name doesn't end that way.
pub fn parse_resource_name(name: &str) -> Option<ResourceName<'_>> {
    let inner = name.strip_suffix(']')?;
    let open = inner.rfind('[')?;
    let (index, raw_kind) = inner[open + 1..].split_once(':')?;
    Some(ResourceName {
        owner: &name[..open],
        index: index.parse().ok()?,
        raw_kind,
        kind: raw_kind.trim_end_matches(MARKERS).trim(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_name_split() {
        let name = r"__chore\gen__\levels\campaignworld010\w1_meridian\w1_meridian_elevator_intro{pm}{b2p}.scenario_structure_bsp";
        let (path, class) = split_tag_name(name);
        assert_eq!(path, r"__chore\gen__\levels\campaignworld010\w1_meridian\w1_meridian_elevator_intro{pm}{b2p}");
        assert_eq!(class, Some("scenario_structure_bsp"));
    }

    #[test]
    fn dots_in_folders_are_not_suffixes() {
        assert_eq!(split_tag_name(r"a.b\c"), (r"a.b\c", None));
        assert_eq!(split_tag_name(r"a.b\c.weapon"), (r"a.b\c", Some("weapon")));
    }

    #[test]
    fn no_suffix() {
        assert_eq!(split_tag_name("loadmanifest"), ("loadmanifest", None));
        assert_eq!(split_tag_name(r"x\.hidden"), (r"x\.hidden", None));
        assert_eq!(split_tag_name("trailing."), ("trailing.", None));
    }

    #[test]
    fn resource_names() {
        let r = parse_resource_name(r"a\b.scenario_structure_bsp[0:mesh resource!*]").unwrap();
        assert_eq!(r.owner, r"a\b.scenario_structure_bsp");
        assert_eq!(r.index, 0);
        assert_eq!(r.raw_kind, "mesh resource!*");
        assert_eq!(r.kind, "mesh resource");

        let r = parse_resource_name(r"a\b.scenario_structure_bsp[1:tag_resources]").unwrap();
        assert_eq!((r.index, r.kind), (1, "tag_resources"));
    }

    #[test]
    fn not_resource_names() {
        assert_eq!(parse_resource_name("loadmanifest"), None);
        assert_eq!(parse_resource_name("a[x:kind]"), None);
        assert_eq!(parse_resource_name("a[0kind]"), None);
    }
}
