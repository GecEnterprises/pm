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
    /// Display name written as the author of tickets and comments (PM-15).
    /// Empty means "not set" — callers fall back to the git `user.name`.
    #[serde(default)]
    pub author: String,
    /// Recently opened project roots, most-recent-first (PM-48). Surfaced in
    /// File → Open Recent Projects and the "Nothing opened" screen.
    #[serde(default)]
    pub recent: Vec<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self { version: 1, ui_scale: 1.0, author: String::new(), recent: Vec::new() }
    }
}

/// How many recent projects [`Config::push_recent`] keeps.
pub const RECENT_CAP: usize = 10;

impl Config {
    /// `ui_scale` clamped to a sane range.
    pub fn ui_scale(&self) -> f32 {
        self.ui_scale.clamp(0.5, 3.0)
    }

    /// Record `path` as the most-recently-opened project: move it to the front,
    /// de-duplicate, cap the list. `path` is resolved to an absolute path first
    /// (without touching the filesystem) so `.` and a full path don't both land
    /// in the list. Returns whether anything changed.
    pub fn push_recent(&mut self, path: &Path) -> bool {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if self.recent.first() == Some(&path) {
            return false;
        }
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(RECENT_CAP);
        true
    }

    /// Pretty JSON text with a trailing newline.
    pub fn to_pretty(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s
    }

    /// Load from `path`. Missing file → `Ok(default)`. Unreadable or malformed →
    /// `Err(reason)` (the caller still gets to fall back to defaults, but can
    /// tell the user). Bytes are decoded lossily so one stray byte can't wipe
    /// the file (same lesson as `pm.json5`).
    pub fn try_load_from(path: &Path) -> std::result::Result<Config, String> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                serde_json::from_str(&text)
                    .map_err(|e| format!("{} is not valid JSON: {e}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("could not read {}: {e}", path.display())),
        }
    }

    /// Infallible load from `path` — errors become logged defaults.
    pub fn load_from(path: &Path) -> Self {
        Self::try_load_from(path).unwrap_or_else(|why| {
            eprintln!("pm: {why}; using default config");
            Config::default()
        })
    }

    /// Load from the default location, reporting *why* on failure.
    pub fn try_load() -> std::result::Result<Config, String> {
        match config_path() {
            Some(p) => Self::try_load_from(&p),
            None => Err("no home directory".to_string()),
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
        let c = Config {
            version: 1,
            ui_scale: 1.3,
            author: "alice".into(),
            recent: vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")],
        };
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
        let mk = |s| Config { ui_scale: s, ..Config::default() };
        assert_eq!(mk(99.0).ui_scale(), 3.0);
        assert_eq!(mk(0.1).ui_scale(), 0.5);
    }

    #[test]
    fn push_recent_dedupes_and_caps() {
        let abs = |i: usize| {
            std::path::absolute(std::env::temp_dir().join(format!("pm-recent-{i}"))).unwrap()
        };
        let mut c = Config::default();
        for i in 0..15 {
            assert!(c.push_recent(&abs(i)));
        }
        assert_eq!(c.recent.len(), RECENT_CAP);
        assert_eq!(c.recent[0], abs(14));
        assert!(!c.push_recent(&abs(14))); // already at front
        assert!(c.push_recent(&abs(9))); // resurface
        assert_eq!(c.recent[0], abs(9));
        assert_eq!(c.recent.iter().filter(|p| **p == abs(9)).count(), 1);
    }

    #[test]
    fn push_recent_normalises_relative() {
        let mut c = Config::default();
        c.push_recent(std::path::Path::new("."));
        assert!(c.recent[0].is_absolute());
    }
}
