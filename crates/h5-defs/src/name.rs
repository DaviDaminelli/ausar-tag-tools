//! The name syntax of definition fields and options (PRD §5.3, extended from
//! the real definitions: docs/format-notes.md "Names"). One parser for both.
//!
//! `display name markers&label{old name}|editor hint:[range]units#help`, read
//! in this order: `#` help, `{…}` old name (it can hold `:` and `|`), `|`
//! editor hint (dropped), `:` units and range, `&` label, trailing markers.

/// Characters that mark a field or option (meaning: PRD §12 Q2).
pub const MARKERS: &str = "^*!~&";
/// Noise in the marker run: dropped, not a marker (one definition writes `name^``).
const STRAY: char = '`';

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Name {
    /// The name without markers, units, help or old name; spaces trimmed.
    pub display: String,
    /// The trailing marker characters, as written (`*~!`).
    pub markers: String,
    /// After `X&Y`: a second, friendlier name (`size class&Analytic Lightmap Size`).
    pub label: Option<String>,
    /// Inside `{…}`: earlier or alternate names (`Radius{Offset}`), in the
    /// written order; a few fields have two.
    pub old_names: Vec<String>,
    pub units: Option<String>,
    /// From `:[min-max]` (also `[0,1]`, `[0,+inf]`), as written inside the brackets.
    pub range: Option<String>,
    pub help: Option<String>,
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

impl Name {
    pub fn parse(raw: &str) -> Name {
        let (head, help) = raw.split_once('#').map_or((raw, None), |(h, help)| (h, non_empty(help)));
        // `{…}` first: it can hold `:` and `|`, and can follow the units.
        let mut rest = head.to_owned();
        let mut old_names = Vec::new();
        while let Some((open, close)) = rest.find('{').and_then(|open| Some((open, open + rest[open..].find('}')?))) {
            old_names.extend(non_empty(&rest[open + 1..close]));
            rest.replace_range(open..=close, "");
        }
        let head = rest;
        // An editor hint (`|CCBBAA`, `|pink`) can follow the name or the units.
        let head = head.split_once('|').map_or(head.as_str(), |(h, _)| h);
        let (mut name, units_part) = head.split_once(':').map_or((head, None), |(n, u)| (n, Some(u)));
        let (range, units) = match units_part.map(str::trim) {
            Some(u) if u.starts_with('[') && u.contains(']') => {
                let (r, rest) = u[1..].split_once(']').expect("checked above");
                (non_empty(r), non_empty(rest))
            }
            Some(u) => (None, non_empty(u)),
            None => (None, None),
        };
        // `X&Y` with no spaces around `&`: a name and a label (a marker may come
        // first: `X^&Y`). A spaced ` & ` is text; a trailing `&` is a marker.
        let mut label = None;
        let joins_words = |i: usize| {
            name[..i].chars().next_back().is_some_and(|p| !p.is_whitespace())
                && name[i + 1..].chars().next().is_some_and(|n| !n.is_whitespace())
        };
        if let Some((i, _)) = name.char_indices().find(|&(i, c)| c == '&' && joins_words(i)) {
            label = non_empty(&name[i + 1..]);
            name = &name[..i];
        }
        let trimmed = name.trim_end();
        let base = trimmed.trim_end_matches(|c: char| MARKERS.contains(c) || c.is_whitespace() || c == STRAY);
        let markers: String = trimmed[base.len()..].chars().filter(|c| MARKERS.contains(*c)).collect();
        Name { display: base.trim().to_owned(), markers, label, old_names, units, range, help }
    }

    /// Provisional rule (PRD §5.3, §12 Q2): a `!` marker hides the field or option.
    pub fn is_hidden(&self) -> bool {
        self.markers.contains('!')
    }

    /// `^`: this field labels its tag block's entries.
    pub fn labels_entries(&self) -> bool {
        self.markers.contains('^')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(raw: &str) -> Name {
        Name::parse(raw)
    }

    #[test]
    fn plain_names_and_markers() {
        assert_eq!(name("rounds per second").display, "rounds per second");
        let n = name("vtable space*~!");
        assert_eq!((n.display.as_str(), n.markers.as_str()), ("vtable space", "*~!"));
        assert!(n.is_hidden());
        let n = name("name^");
        assert!(n.labels_entries() && !n.is_hidden());
        assert_eq!(name("  padded  ").display, "padded");
        // Marker characters that aren't at the end are text.
        assert_eq!(name("can you see me???^").display, "can you see me???");
        assert_eq!(name("<unused0>!").display, "<unused0>");
        // A stray backtick in the marker run (one definition has `name^``) is dropped.
        let n = name("name^`");
        assert_eq!((n.display.as_str(), n.markers.as_str()), ("name", "^"));
    }

    #[test]
    fn help_units_and_range() {
        let n = name("auto-exposure delay:[0.1-1]seconds");
        assert_eq!(n.display, "auto-exposure delay");
        assert_eq!((n.range.as_deref(), n.units.as_deref()), (Some("0.1-1"), Some("seconds")));
        let n = name("player shield damage threshold:[0,1]#Damage greater than threshold is considered 'extended'");
        assert_eq!(n.range.as_deref(), Some("0,1"));
        assert_eq!(n.units, None);
        assert_eq!(n.help.as_deref(), Some("Damage greater than threshold is considered 'extended'"));
        let n = name("extended shield damage threshold:%#Amount of shield damage");
        assert_eq!(n.units.as_deref(), Some("%"));
        let n = name("node name#empty mean object space");
        assert_eq!((n.display.as_str(), n.help.as_deref()), ("node name", Some("empty mean object space")));
        // Help may itself contain ':' and '#'.
        let n = name("a#b: c # d");
        assert_eq!((n.units, n.help.as_deref()), (None, Some("b: c # d")));
    }

    #[test]
    fn old_names_in_braces() {
        let n = name("Radius{Offset}");
        assert_eq!((n.display.as_str(), n.old_names.first().map(String::as_str)), ("Radius", Some("Offset")));
        let n = name("decimate to!{max decimation percentage}");
        assert_eq!((n.display.as_str(), n.markers.as_str()), ("decimate to", "!"));
        assert_eq!(n.old_names.first().map(String::as_str), Some("max decimation percentage"));
        let n = name("horizontal acceleration scale{acceleration scale}:[0,+inf]");
        assert_eq!(n.display, "horizontal acceleration scale");
        assert_eq!(n.range.as_deref(), Some("0,+inf"));
        // Markers inside the braces belong to the note, not the field.
        let n = name("use dual mixing boards{costly! Don't use it unless you need it!!!!}");
        assert_eq!((n.display.as_str(), n.markers.as_str()), ("use dual mixing boards", ""));
        assert_eq!(name("skulls^{primary skulls}").markers, "^");
        // Braces can hold `:` and `|`, and can follow the units.
        let n = name("overlay group{pose overlay|overlay}");
        assert_eq!(
            (n.display.as_str(), n.old_names.first().map(String::as_str)),
            ("overlay group", Some("pose overlay|overlay"))
        );
        let n = name("compiled shader{..:dx11 compiled shader}");
        assert_eq!((n.units, n.old_names.first().map(String::as_str)), (None, Some("..:dx11 compiled shader")));
        let n = name("falloff begin:wus{sphere falloff begin}");
        assert_eq!((n.display.as_str(), n.units.as_deref()), ("falloff begin", Some("wus")));
        assert_eq!(n.old_names.first().map(String::as_str), Some("sphere falloff begin"));
        // Several renames: every brace group is an earlier name, in the written order.
        let n = name("Only render if editable{only render in editor}{only visible in editor}#help");
        assert_eq!(n.display, "Only render if editable");
        assert_eq!(n.old_names, ["only render in editor", "only visible in editor"]);
    }

    #[test]
    fn labels_after_an_ampersand() {
        let n = name("size class&Analytic Lightmap Size");
        assert_eq!((n.display.as_str(), n.label.as_deref()), ("size class", Some("Analytic Lightmap Size")));
        assert_eq!(name("Direction 1&Direction(D)").label.as_deref(), Some("Direction(D)"));
        // A marker can come right before the label.
        let n = name("structure lighting_info^&Structure Lights");
        assert_eq!((n.display.as_str(), n.markers.as_str()), ("structure lighting_info", "^"));
        assert_eq!(n.label.as_deref(), Some("Structure Lights"));
        // A spaced ampersand is text; a trailing one is a marker.
        let n = name("GOAL & RESPAWN ZONE OBJECT BOUNDARY");
        assert_eq!((n.display.as_str(), n.label.as_deref()), ("GOAL & RESPAWN ZONE OBJECT BOUNDARY", None));
        let n = name("local tag handle!~");
        assert_eq!(n.markers, "!~");
        assert_eq!(name("thing&").markers, "&");
        assert_eq!(name("thing!&").markers, "!&");
    }

    #[test]
    fn editor_hints_are_dropped() {
        let n = name("skeleton bones*|ABCDCC");
        assert_eq!((n.display.as_str(), n.markers.as_str()), ("skeleton bones", "*"));
        assert_eq!(name("all fields|pink").display, "all fields");
        let n = name("pain screen region fade out duration:seconds|CCBBAA#The time it takes to fade out");
        assert_eq!(n.units.as_deref(), Some("seconds"));
        assert_eq!(n.help.as_deref(), Some("The time it takes to fade out"));
    }
}
