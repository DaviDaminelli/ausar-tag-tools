//! Lord Zedd's H5Tags definitions (PRD §5.3). For M1: the list of definition
//! files and the lookup from a class to its file. XML parsing comes in M3.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use h5_module::FourCc;

/// One `(code)class name.xml` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinitionFile {
    /// The code as written in the file name (`hsc_`, `mat`).
    pub file_code: String,
    pub class_name: String,
    pub path: PathBuf,
}

/// Parses `(code)class name.xml`: the code is the text between the first `(`
/// and the next `)`.
pub fn parse_file_name(name: &str) -> Option<(&str, &str)> {
    let stem = name.strip_suffix(".xml").or_else(|| name.strip_suffix(".XML"))?;
    let rest = stem.strip_prefix('(')?;
    let (code, class_name) = rest.split_once(')')?;
    (!code.is_empty() && !class_name.is_empty()).then_some((code, class_name))
}

/// The definitions folder of a build.
#[derive(Clone, Debug, Default)]
pub struct Definitions {
    by_code: HashMap<FourCc, usize>,
    by_name: HashMap<String, usize>,
    files: Vec<DefinitionFile>,
}

impl Definitions {
    /// Lists `dir`. Files that don't follow the naming pattern are ignored.
    pub fn load(dir: &Path) -> io::Result<Definitions> {
        let mut files = Vec::new();
        for item in std::fs::read_dir(dir)? {
            let path = item?.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if let Some((code, class_name)) = parse_file_name(name) {
                files.push(DefinitionFile {
                    file_code: code.to_owned(),
                    class_name: class_name.to_owned(),
                    path: path.clone(),
                });
            }
        }
        files.sort_by(|a, b| a.class_name.cmp(&b.class_name));
        Ok(Definitions::from_files(files))
    }

    pub fn from_files(files: Vec<DefinitionFile>) -> Definitions {
        let mut by_code = HashMap::new();
        let mut by_name = HashMap::new();
        for (i, f) in files.iter().enumerate() {
            if let Some(code) = FourCc::from_code(&f.file_code) {
                by_code.entry(code).or_insert(i);
            }
            by_name.entry(f.class_name.to_lowercase()).or_insert(i);
        }
        Definitions { by_code, by_name, files }
    }

    pub fn files(&self) -> &[DefinitionFile] {
        &self.files
    }

    /// The definition of a class: by code first, then by class name. File
    /// names trim trailing spaces and replace characters not allowed in file
    /// names with `_` (`(hsc_)lua.xml` is class `hsc*`), so the code alone
    /// isn't always enough (format-notes "Definitions").
    pub fn find(&self, code: FourCc, class_name: Option<&str>) -> Option<&DefinitionFile> {
        self.by_code
            .get(&code)
            .or_else(|| class_name.and_then(|n| self.by_name.get(&n.to_lowercase())))
            .map(|&i| &self.files[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> Definitions {
        let file = |code: &str, name: &str| DefinitionFile {
            file_code: code.into(),
            class_name: name.into(),
            path: PathBuf::from(format!("({code}){name}.xml")),
        };
        Definitions::from_files(vec![
            file("weap", "weapon"),
            file("mat", "material"),
            file("hsc_", "lua"),
            file("_fx_", "sound_effect_template"),
        ])
    }

    #[test]
    fn file_names() {
        assert_eq!(parse_file_name("(weap)weapon.xml"), Some(("weap", "weapon")));
        assert_eq!(parse_file_name("(_fx_)sound_effect_template.xml"), Some(("_fx_", "sound_effect_template")));
        assert_eq!(parse_file_name("(mat)material.xml"), Some(("mat", "material")));
        assert_eq!(
            parse_file_name("(0008)bitmap_data_resource_definition.xml"),
            Some(("0008", "bitmap_data_resource_definition"))
        );
        assert_eq!(parse_file_name("readme.txt"), None);
        assert_eq!(parse_file_name("()x.xml"), None);
        assert_eq!(parse_file_name("(weap).xml"), None);
    }

    #[test]
    fn find_by_code() {
        let d = defs();
        assert_eq!(d.find(FourCc::from_code("weap").unwrap(), None).unwrap().class_name, "weapon");
        // Trimmed trailing space.
        assert_eq!(d.find(FourCc::from_code("mat ").unwrap(), None).unwrap().class_name, "material");
    }

    #[test]
    fn find_by_name_when_code_was_replaced() {
        let d = defs();
        let hsc = FourCc::from_code("hsc*").unwrap();
        assert_eq!(d.find(hsc, None), None);
        assert_eq!(d.find(hsc, Some("lua")).unwrap().file_code, "hsc_");
        assert_eq!(d.find(hsc, Some("LUA")).unwrap().file_code, "hsc_");
    }

    #[test]
    fn unknown_class() {
        assert_eq!(defs().find(FourCc::from_code("zzzz").unwrap(), Some("nothing")), None);
    }
}
