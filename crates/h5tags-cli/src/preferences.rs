//! Per-person display preferences for the CLI (decisions D25): an optional
//! `preferences.toml` in the user's config folder. `h5tags` never writes it.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    /// Colors in the terminal (`table` format only). Default: true.
    pub color: Option<bool>,
}

impl Preferences {
    /// Reads `path`; a missing file means defaults.
    pub fn load(path: &Path) -> Result<Preferences, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

/// `%APPDATA%\h5tags\preferences.toml` (Windows),
/// `~/Library/Application Support/h5tags/preferences.toml` (macOS),
/// `$XDG_CONFIG_HOME/h5tags/preferences.toml` or `~/.config/h5tags/…` (others).
pub fn default_path() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    };
    base.map(|b| b.join("h5tags").join("preferences.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_color() {
        assert_eq!(toml::from_str::<Preferences>("color = false").unwrap(), Preferences { color: Some(false) });
        assert_eq!(toml::from_str::<Preferences>("").unwrap(), Preferences::default());
    }

    #[test]
    fn rejects_typos_and_wrong_types() {
        assert!(toml::from_str::<Preferences>("colour = false").is_err());
        assert!(toml::from_str::<Preferences>("color = \"never\"").is_err());
    }

    #[test]
    fn missing_file_means_defaults() {
        let path = std::env::temp_dir().join("h5tags-no-such-preferences.toml");
        assert_eq!(Preferences::load(&path), Ok(Preferences::default()));
    }
}
