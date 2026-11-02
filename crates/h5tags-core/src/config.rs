//! `h5tags.toml` (PRD §6.1).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// `tag_cache_mb` when the config doesn't set it (PRD §6.1).
pub const DEFAULT_TAG_CACHE_MB: u64 = 256;

pub(crate) fn megabytes(mb: u64) -> usize {
    (mb as usize) << 20
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Where the index and caches are kept. Default: the OS cache folder.
    pub cache_dir: Option<PathBuf>,
    /// Used from the backlog (PRD §10); accepted and ignored for now.
    pub notes_file: Option<PathBuf>,
    /// Definition GUID check mode (PRD §6.4.5); used from M3.
    pub guid_check: Option<String>,
    /// Decompressed tags kept in memory, in MB (PRD §13). Default 256.
    pub tag_cache_mb: Option<u64>,
    #[serde(rename = "build")]
    pub builds: Vec<BuildConfig>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    pub name: String,
    pub deploy: PathBuf,
    pub defs: PathBuf,
    pub resource_defs: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("can't read config file {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("invalid config file {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("invalid config: {0}")]
    Invalid(String),
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read { path: path.into(), source })?;
        let mut config: Config =
            toml::from_str(&text).map_err(|e| ConfigError::Parse { path: path.into(), message: e.to_string() })?;
        // Relative paths are relative to the config file.
        if let Some(base) = path.parent() {
            config.resolve_relative(base);
        }
        config.validate()?;
        Ok(config)
    }

    /// The `--deploy/--defs` shortcut: one build named `default`.
    pub fn single(
        deploy: PathBuf,
        defs: PathBuf,
        resource_defs: Option<PathBuf>,
        cache_dir: Option<PathBuf>,
    ) -> Config {
        Config {
            cache_dir,
            notes_file: None,
            guid_check: None,
            tag_cache_mb: None,
            builds: vec![BuildConfig { name: "default".into(), deploy, defs, resource_defs }],
        }
    }

    fn resolve_relative(&mut self, base: &Path) {
        let fix = |p: &mut PathBuf| {
            if p.is_relative() {
                *p = base.join(&*p);
            }
        };
        if let Some(p) = &mut self.cache_dir {
            fix(p);
        }
        for b in &mut self.builds {
            fix(&mut b.deploy);
            fix(&mut b.defs);
            if let Some(p) = &mut b.resource_defs {
                fix(p);
            }
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.builds.is_empty() {
            return Err(ConfigError::Invalid("no [[build]] configured".into()));
        }
        let mut seen = HashSet::new();
        for b in &self.builds {
            let valid = !b.name.is_empty()
                && b.name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-');
            if !valid {
                return Err(ConfigError::Invalid(format!(
                    "build name '{}' must use only a-z, 0-9, '_' and '-'",
                    b.name
                )));
            }
            if !seen.insert(&b.name) {
                return Err(ConfigError::Invalid(format!("build name '{}' is used twice", b.name)));
            }
        }
        Ok(())
    }

    /// `tag_cache_mb` in bytes.
    pub fn tag_cache_bytes(&self) -> usize {
        megabytes(self.tag_cache_mb.unwrap_or(DEFAULT_TAG_CACHE_MB))
    }

    /// `cache_dir`, or the OS cache folder (decisions D15).
    pub fn cache_dir(&self) -> PathBuf {
        self.cache_dir.clone().unwrap_or_else(default_cache_dir)
    }
}

/// `%LOCALAPPDATA%\h5tags` (Windows), `~/Library/Caches/h5tags` (macOS),
/// `$XDG_CACHE_HOME/h5tags` or `~/.cache/h5tags` (others); `./h5tags-cache` as
/// a last resort.
pub fn default_cache_dir() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Caches"))
    } else {
        env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|h| h.join(".cache")))
    };
    base.map_or_else(|| PathBuf::from("h5tags-cache"), |b| b.join("h5tags"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, ConfigError> {
        let c: Config = toml::from_str(text).map_err(|e| ConfigError::Invalid(e.to_string()))?;
        c.validate()?;
        Ok(c)
    }

    #[test]
    fn prd_example_parses() {
        let c = parse(
            r#"
cache_dir = 'C:\h5tags\cache'
notes_file = 'C:\h5tags\h5tags-notes.toml'
tag_cache_mb = 64

[[build]]
name = "forge"
deploy = 'C:\Forge\deploy'
defs = 'C:\H5Tags\tags'
resource_defs = 'C:\H5Tags\tag_resources'

[[build]]
name = "xbox"
deploy = 'D:\h5_xbox\deploy'
defs = 'C:\H5Tags\tags'
"#,
        )
        .unwrap();
        assert_eq!(c.builds.len(), 2);
        assert_eq!(c.builds[0].name, "forge");
        assert_eq!(c.builds[1].resource_defs, None);
        assert_eq!(c.cache_dir(), PathBuf::from(r"C:\h5tags\cache"));
        assert_eq!(c.tag_cache_bytes(), 64 << 20);
        let default = parse("[[build]]\nname = \"a\"\ndeploy = 'x'\ndefs = 'y'\n").unwrap();
        assert_eq!(default.tag_cache_bytes(), 256 << 20);
    }

    #[test]
    fn rejects_bad_configs() {
        let build = |name: &str| format!("[[build]]\nname = \"{name}\"\ndeploy = 'x'\ndefs = 'y'\n");
        assert!(parse("").is_err());
        assert!(parse(&build("Forge")).is_err());
        assert!(parse(&build("a b")).is_err());
        assert!(parse(&format!("{}{}", build("a"), build("a"))).is_err());
        assert!(parse(&format!("typo = 1\n{}", build("a"))).is_err());
        assert!(parse(&build("forge_pc-2")).is_ok());
    }

    #[test]
    fn single_build_shortcut() {
        let c = Config::single("d".into(), "t".into(), None, None);
        assert_eq!(c.builds[0].name, "default");
        assert!(c.validate().is_ok());
    }
}
