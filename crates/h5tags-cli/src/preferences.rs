//! Per-person display preferences for the CLI (decisions D25): an optional
//! `preferences.toml` in the user's config folder. Written only by the
//! `h5tags preferences set|unset` command, never on its own.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    /// Colors in the terminal (`table` format only). Default: true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<bool>,
}

/// The default when a preference isn't set.
pub const DEFAULT_COLOR: bool = true;

const HEADER: &str = "\
# h5tags preferences (CLI display only; the AI never sees colors).
# Change them with `h5tags preferences set|unset`, or edit this file.
# `h5tags preferences set|unset` rewrites the whole file: comments added here are not kept.
";

impl Preferences {
    /// Reads `path`; a missing file means defaults.
    pub fn load(path: &Path) -> Result<Preferences, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Writes `path` (and its folder) with a short header.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let body = toml::to_string(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(path, format!("{HEADER}{body}")).map_err(|e| format!("{}: {e}", path.display()))
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

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("h5tags-prefs-test-{}", std::process::id()));
        let path = dir.join("sub").join("preferences.toml");
        let prefs = Preferences { color: Some(false) };
        prefs.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# h5tags preferences") && text.ends_with("color = false\n"), "{text}");
        assert_eq!(Preferences::load(&path), Ok(prefs));
        // Unset: an empty preferences file is valid and means defaults.
        Preferences::default().save(&path).unwrap();
        assert_eq!(Preferences::load(&path), Ok(Preferences::default()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
