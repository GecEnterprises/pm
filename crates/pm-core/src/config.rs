//! Per-user configuration, persisted at `~/.pm/config.json`.
//!
//! This is *user* data — it follows the user across repositories (window
//! scale, colour scheme, …). Project tickets live separately in each repo's
//! `.pm/pm.json5`. A missing or broken config file never stops pm starting;
//! it falls back to defaults and logs.
//!
//! The layer is deliberately a plain JSON key/value document (no sqlite) — see
//! PM-34. New settings are just `#[serde(default)]` fields.

use std::hash::{Hash, Hasher};
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
    /// Watch-jump mode: when on, a filesystem change to any file in the change
    /// list opens that file in the diff pane (PM-30). Toggled from the footer.
    #[serde(default)]
    pub watchjump: bool,
    /// Per-project ticket-store locations (PM-34). A project whose root isn't
    /// listed here uses the in-repo default (`<root>/.pm`).
    #[serde(default)]
    pub stores: Vec<ProjectStore>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            ui_scale: 1.0,
            author: String::new(),
            recent: Vec::new(),
            watchjump: false,
            stores: Vec::new(),
        }
    }
}

/// Where a project's `pm.json5` lives (PM-34).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreLocation {
    /// `<project-root>/.pm/` — the default; committed alongside the code.
    InRepo,
    /// `~/.pm/projects/<slug>/` — the repo is left untouched.
    Home,
}

/// A remembered choice of where one project keeps its ticket store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectStore {
    /// Absolute project root (the key).
    pub root: PathBuf,
    pub location: StoreLocation,
    /// Absolute directory that contains `pm.json5`. Stored explicitly so a
    /// future change to the slug scheme can't strand an existing store.
    pub dir: PathBuf,
}

/// How many recent projects [`Config::push_recent`] keeps.
pub const RECENT_CAP: usize = 10;

/// Absolutise a path without touching the filesystem (matches `push_recent`).
fn abs(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A filesystem-safe directory name derived from a project root — a sanitised
/// rendering of the absolute path plus a short hash so distinct paths that
/// sanitise the same still get distinct slugs (PM-34).
pub fn project_slug(root: &Path) -> String {
    let full = abs(root);
    let text = full.to_string_lossy();

    let mut slug = String::new();
    let mut last_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    // Keep the tail (the project folder name) when the path is very long.
    let tail: String = {
        let chars: Vec<char> = slug.chars().collect();
        let start = chars.len().saturating_sub(60);
        chars[start..].iter().collect::<String>()
    };

    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    format!("{}-{:08x}", tail.trim_matches('-'), h.finish() as u32)
}

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

    /// The remembered ticket-store choice for `root`, if any (PM-34).
    pub fn store_for(&self, root: &Path) -> Option<&ProjectStore> {
        let root = abs(root);
        self.stores.iter().find(|s| s.root == root)
    }

    /// The directory that holds this project's `pm.json5` — the remembered
    /// choice, or the in-repo default `<root>/.pm` (PM-34).
    pub fn resolve_store_dir(&self, root: &Path) -> PathBuf {
        self.store_for(root)
            .map(|s| s.dir.clone())
            .unwrap_or_else(|| abs(root).join(".pm"))
    }

    /// Record where `root` keeps its ticket store, computing the directory.
    /// Replaces any existing entry for the same root. Returns the resolved dir.
    pub fn set_store(&mut self, root: &Path, location: StoreLocation) -> PathBuf {
        let root = abs(root);
        let dir = match location {
            StoreLocation::InRepo => root.join(".pm"),
            StoreLocation::Home => home_dir()
                .unwrap_or_else(|| root.clone())
                .join(".pm")
                .join("projects")
                .join(project_slug(&root)),
        };
        self.stores.retain(|s| s.root != root);
        self.stores.push(ProjectStore { root, location, dir: dir.clone() });
        dir
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
            watchjump: true,
            stores: vec![ProjectStore {
                root: PathBuf::from("/tmp/proj"),
                location: StoreLocation::Home,
                dir: PathBuf::from("/home/u/.pm/projects/tmp-proj-0"),
            }],
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
    fn slug_is_stable_and_distinct() {
        let a = project_slug(Path::new("/home/u/Projects/pm"));
        assert_eq!(a, project_slug(Path::new("/home/u/Projects/pm")));
        // Distinct paths that sanitise the same still differ (the hash suffix).
        assert_ne!(
            project_slug(Path::new("/home/u/a-b")),
            project_slug(Path::new("/home/u/a/b"))
        );
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')));
    }

    #[test]
    fn store_dir_default_vs_registered() {
        let root = Path::new("/tmp/some/proj");
        let abs_pm = std::path::absolute(root).unwrap().join(".pm");
        let mut c = Config::default();
        assert_eq!(c.resolve_store_dir(root), abs_pm);

        let dir = c.set_store(root, StoreLocation::Home);
        assert_eq!(c.resolve_store_dir(root), dir);
        assert!(dir.ends_with(project_slug(root)));

        c.set_store(root, StoreLocation::InRepo);
        assert_eq!(c.stores.len(), 1); // replaced, not appended
        assert_eq!(c.resolve_store_dir(root), abs_pm);
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
