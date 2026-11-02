//! `h5tags.toml` (PRD §6.1).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// `tag_cache_mb` when the config doesn't set it (PRD §6.1).
pub const DEFAULT_TAG_CACHE_MB: u64 = 256;

/// `large_read_tokens` when the config doesn't set it (PRD §6.1).
pub const DEFAULT_LARGE_READ_TOKENS: usize = 3000;

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
    /// The size guard on whole reads, in tokens (PRD §6.4.1). Default 3000.
    pub large_read_tokens: Option<usize>,
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

/// Where the config comes from, as both programs take it on their command
/// line (`h5tags` and `h5tags-mcp`): a config file, or one build given by its
/// folders.
#[derive(Clone, Debug, Default, clap::Args)]
pub struct ConfigArgs {
    /// Config file (default: h5tags.toml next to the executable).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    /// One build without a config file: its deploy folder (needs --defs).
    #[arg(long, global = true)]
    pub deploy: Option<PathBuf>,
    /// With --deploy: Lord Zedd's H5Tags `tags` folder.
    #[arg(long, global = true)]
    pub defs: Option<PathBuf>,
    /// With --deploy: Lord Zedd's H5Tags `tag_resources` folder.
    #[arg(long, global = true)]
    pub resource_defs: Option<PathBuf>,
    /// Where the index cache is kept (overrides the config).
    #[arg(long, global = true)]
    pub cache_dir: Option<PathBuf>,
}

impl ConfigArgs {
    /// The config these arguments name; the message (with a `hint:` line
    /// where one helps) when they don't name a usable one.
    pub fn load(&self) -> Result<Config, String> {
        let mut config = match (&self.deploy, &self.config) {
            (Some(_), Some(_)) => return Err("use either --config or --deploy, not both".into()),
            (Some(deploy), None) => {
                let defs = self.defs.clone().ok_or("--deploy needs --defs <Lord Zedd's H5Tags tags folder>")?;
                Config::single(deploy.clone(), defs, self.resource_defs.clone(), None)
            }
            (None, path) => {
                let path = match path {
                    Some(p) => p.clone(),
                    None => std::env::current_exe()
                        .ok()
                        .and_then(|exe| exe.parent().map(|d| d.join("h5tags.toml")))
                        .ok_or("can't find the executable's folder; pass --config")?,
                };
                if !path.exists() {
                    return Err(format!(
                        "no config file at {}\nhint: pass --config <file>, or --deploy <folder> --defs <folder>",
                        path.display()
                    ));
                }
                Config::load(&path).map_err(|e| e.to_string())?
            }
        };
        if let Some(dir) = &self.cache_dir {
            config.cache_dir = Some(dir.clone());
        }
        Ok(config)
    }
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
            large_read_tokens: None,
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
large_read_tokens = 5000

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
        assert_eq!(c.large_read_tokens, Some(5000));
        assert_eq!(default.large_read_tokens, None);
    }

    /// The CLI's and the MCP server's config arguments (PRD §6.1).
    #[test]
    fn config_arguments_choose_a_file_or_one_build() {
        let args = |deploy: Option<&str>, defs: Option<&str>, config: Option<&str>| ConfigArgs {
            config: config.map(PathBuf::from),
            deploy: deploy.map(PathBuf::from),
            defs: defs.map(PathBuf::from),
            resource_defs: None,
            cache_dir: Some("c".into()),
        };
        let c = args(Some("d"), Some("t"), None).load().unwrap();
        assert_eq!((c.builds[0].name.as_str(), c.cache_dir()), ("default", PathBuf::from("c")));
        assert_eq!(
            args(Some("d"), None, None).load().unwrap_err(),
            "--deploy needs --defs <Lord Zedd's H5Tags tags folder>"
        );
        assert!(args(Some("d"), Some("t"), Some("x.toml")).load().unwrap_err().contains("not both"));
        assert!(
            args(None, None, Some("no-such.toml")).load().unwrap_err().starts_with("no config file at no-such.toml")
        );
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
