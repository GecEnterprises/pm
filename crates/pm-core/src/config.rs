//! Per-user configuration, persisted at `~/.pm/config.json`.
//!
//! This is *user* data — it follows the user across repositories (window
//! scale, colour scheme, …). Project tickets live separately in each repo's
//! `.pm/pm.json5`. A missing or broken config file never stops pm starting;
//! it falls back to defaults and logs.
//!
//! The layer is deliberately a plain JSON key/value document (no sqlite) — see
//! PM-34. New settings are just `#[serde(default)]` fields.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

fn default_version() -> u32 {
    1
}
fn default_scale() -> f32 {
    1.0
}

/// The whole user config document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Schema version, reserved for future migrations.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Whole-window zoom factor, `1.0` = 100% (applied by the view layer — PM-36).
    #[serde(default = "default_scale")]
    pub ui_scale: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self { version: 1, ui_scale: 1.0 }
    }
}

impl Config {
    /// `ui_scale` clamped to a sane range.
    pub fn ui_scale(&self) -> f32 {
        self.ui_scale.clamp(0.5, 3.0)
    }

    /// Pretty JSON text with a trailing newline.
    pub fn to_pretty(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s
    }

    /// Load from `path`. Missing → default; unreadable / malformed → default
    /// (logged). Bytes are decoded lossily so one stray byte can't wipe the
    /// file (same lesson as `pm.json5`).
    pub fn load_from(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                serde_json::from_str(&text).unwrap_or_else(|e| {
                    eprintln!("pm: {} is malformed ({e}); using default config", path.display());
                    Config::default()
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => {
                eprintln!("pm: reading {} failed ({e}); using default config", path.display());
                Config::default()
            }
        }
    }

    /// Write to `path`, creating the directory. Atomic: temp file + rename.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let dir = path.parent().context("config path has no parent")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let tmp = dir.join(".config.json.tmp");
        std::fs::write(&tmp, self.to_pretty())
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .or_else(|_| {
                let r = std::fs::write(path, self.to_pretty());
                let _ = std::fs::remove_file(&tmp);
                r
            })
            .with_context(|| format!("replacing {}", path.display()))
    }

    /// Load from the default location (`~/.pm/config.json`).
    pub fn load() -> Self {
        match config_path() {
            Some(p) => Self::load_from(&p),
            None => {
                eprintln!("pm: no home directory; using default config");
                Config::default()
            }
        }
    }

    /// Save to the default location.
    pub fn save(&self) -> Result<()> {
        let path = config_path().context("no home directory")?;
        self.save_to(&path)
    }
}

/// `~/.pm/config.json`, or `None` when the home directory can't be resolved.
pub fn config_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".pm").join("config.json"))
}

fn home_dir() -> Option<PathBuf> {
    #[allow(deprecated)] // un-deprecated since Rust 1.85
    std::env::home_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn tmp() -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!(
            "pm-config-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        d.join("config.json")
    }

    #[test]
    fn missing_is_default() {
        assert_eq!(Config::load_from(&tmp()), Config::default());
    }

    #[test]
    fn round_trips() {
        let p = tmp();
        let c = Config { version: 1, ui_scale: 1.3 };
        c.save_to(&p).unwrap();
        assert_eq!(Config::load_from(&p), c);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn malformed_is_default_not_panic() {
        let p = tmp();
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(Config::load_from(&p), Config::default());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn unknown_fields_ignored() {
        let p = tmp();
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, r#"{ "ui_scale": 2.0, "future_setting": "hi" }"#).unwrap();
        assert_eq!(Config::load_from(&p).ui_scale, 2.0);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn ui_scale_is_clamped() {
        assert_eq!(Config { version: 1, ui_scale: 99.0 }.ui_scale(), 3.0);
        assert_eq!(Config { version: 1, ui_scale: 0.1 }.ui_scale(), 0.5);
    }
}
