//! `~/.config/mpc-commander/config.toml`: the last host, and the window size.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The on-disk configuration; every field has a default so a partial file loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The addin's host name or address; empty until the user types one.
    pub host: String,
    pub port: u16,
    /// Connect to `host` when the app starts.
    pub connect_on_start: bool,
    pub window: Window,
}

/// The window size in logical pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Window {
    pub width: f32,
    pub height: f32,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            width: 1400.0,
            height: 860.0,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: commander_protocol::DEFAULT_PORT,
            connect_on_start: true,
            window: Window::default(),
        }
    }
}

/// `$XDG_CONFIG_HOME` or `~/.config`, then `mpc-commander/config.toml`.
pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("mpc-commander").join("config.toml"))
}

/// `$XDG_CACHE_HOME` or `~/.cache`, then `mpc-commander`.
pub fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("mpc-commander"))
}

impl Config {
    /// The saved configuration, or the defaults when there is none or it does not parse.
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("{}: {e}; using defaults", path.display());
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.new");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial_files() {
        let c = Config {
            host: "mpc.local".into(),
            port: 6731,
            connect_on_start: false,
            window: Window {
                width: 800.0,
                height: 600.0,
            },
        };
        let text = toml::to_string_pretty(&c).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), c);
        let partial: Config = toml::from_str("host = \"a\"\n").unwrap();
        assert_eq!(partial.host, "a");
        assert_eq!(partial.port, commander_protocol::DEFAULT_PORT);
        assert!(partial.connect_on_start);
    }
}
