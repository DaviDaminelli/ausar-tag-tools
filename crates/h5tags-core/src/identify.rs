//! `identify` (D63): what a folder of modules is, and a name for it as a
//! build. Domain only; the facts come from `h5_module::identify`.

use h5_module::identify::Folder;

/// What a module format is known as (format-notes "Header": 27 in every
/// Forge PC file, 23 in every Xbox One file).
pub fn format_label(version: u32) -> Option<&'static str> {
    match version {
        27 => Some("forge"),
        23 => Some("xbox"),
        _ => None,
    }
}

/// A build name for the folder: what its most common supported module format
/// is known as (`build-v<N>` for an unsupported one, `build` without
/// modules), then the latest content version in its module names
/// (`xbox-97230`). A suggestion: the person can choose another.
pub fn suggested_name(folder: &Folder) -> String {
    let formats = folder.module_formats();
    let main = formats.iter().max_by_key(|&(&v, &n)| (format_label(v).is_some(), n)).map(|(&v, _)| v);
    let base = match main {
        Some(v) => format_label(v).map_or_else(|| format!("build-v{v}"), str::to_owned),
        None => "build".to_owned(),
    };
    match folder.latest_layer() {
        Some(l) => format!("{base}-{}", l.version),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use h5_module::identify::{Format, ModuleFacts};
    use h5_module::names::module_layer;

    use super::*;

    fn module(path: &str, version: Option<u32>) -> ModuleFacts {
        ModuleFacts {
            rel_path: path.into(),
            format: version
                .map(|version| Format { version, supported: format_label(version).is_some() })
                .ok_or_else(|| "not a module".into()),
            tag_format: None,
            layer: module_layer(path),
        }
    }

    /// D63: the format's name, then the latest content version.
    #[test]
    fn names_from_format_and_version() {
        let xbox = Folder {
            modules: vec![module("globals-rtx-1.module", Some(23)), module("globals-rtx-97230-1.module", Some(23))],
        };
        assert_eq!(suggested_name(&xbox), "xbox-97230");
        let forge = Folder { modules: vec![module(r"any\levels\globals-rtx-1.module", Some(27))] };
        assert_eq!(suggested_name(&forge), "forge");
        // A supported format wins over a more common unsupported one.
        let mixed = Folder {
            modules: vec![
                module("a-rtx-1.module", Some(53)),
                module("b-rtx-1.module", Some(53)),
                module("c-rtx-1.module", Some(27)),
            ],
        };
        assert_eq!(suggested_name(&mixed), "forge");
        let beta = Folder { modules: vec![module("a-rtx-500-1.module", Some(53))] };
        assert_eq!(suggested_name(&beta), "build-v53-500");
        assert_eq!(suggested_name(&Folder { modules: vec![module("x.module", None)] }), "build");
    }
}
