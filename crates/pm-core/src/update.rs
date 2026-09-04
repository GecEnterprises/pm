//! GitHub-release update check + Windows self-replace (`pm update`) — PM-14.
//!
//! The check is a plain blocking HTTPS GET (no async runtime); the UI runs it on
//! a background thread. `run_self_update` is Windows-only for now.

use anyhow::{anyhow, bail, Result};

use crate::buildinfo as build;

const REPO: &str = "GecEnterprises/pm";
const UA: &str = concat!("pm/", env!("CARGO_PKG_VERSION"));

/// The latest published GitHub release.
#[derive(Clone, Debug)]
pub struct Release {
    /// Tag as published, e.g. `"v0.2.0"`.
    pub tag: String,
    /// Parsed `(major, minor, patch)`.
    pub version: (u32, u32, u32),
    /// Release body (markdown).
    pub notes: String,
    /// Download URL of the `pm.exe` asset, if present.
    pub exe_url: Option<String>,
}

impl Release {
    /// Whether this release is strictly newer than the running build.
    pub fn is_newer_than_current(&self) -> bool {
        parse_semver(build::VERSION).is_some_and(|cur| self.version > cur)
    }
}

/// `"v1.2.3"` / `"1.2.3"` / `"1.2"` → `(1, 2, 3)`. Extra pre-release/build
/// metadata after the patch is ignored.
pub fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s
        .split(['-', '+'])
        .next()
        .unwrap_or(s);
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// Query `releases/latest`. Network / parse failures are returned as errors for
/// the caller to swallow (offline is not exceptional).
pub fn latest() -> Result<Release> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = ureq::get(&url)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .call()?
        .into_string()?;
    let v: serde_json::Value = serde_json::from_str(&body)?;

    let tag = v["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow!("release has no tag_name"))?
        .to_string();
    let version = parse_semver(&tag).ok_or_else(|| anyhow!("tag {tag:?} is not semver"))?;
    let notes = v["body"].as_str().unwrap_or_default().to_string();
    let exe_url = v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"].as_str() == Some("pm.exe"))
        .and_then(|a| a["browser_download_url"].as_str())
        .map(str::to_string);

    Ok(Release { tag, version, notes, exe_url })
}

/// Delete a leftover `pm.exe.old` next to the running exe (call once at startup).
pub fn cleanup_stale() {
    if let Ok(exe) = std::env::current_exe() {
        let old = exe.with_extension("exe.old");
        let _ = std::fs::remove_file(old);
    }
}

/// Download the latest release over the running executable and relaunch it.
/// Returns `Ok(())` only when it did *not* need to relaunch (already current);
/// otherwise it execs the new binary and never returns.
#[cfg(windows)]
pub fn run_self_update() -> Result<()> {
    let rel = latest()?;
    if !rel.is_newer_than_current() {
        println!("pm is up to date ({}).", build::VERSION);
        return Ok(());
    }
    let url = rel
        .exe_url
        .ok_or_else(|| anyhow!("release {} has no pm.exe asset", rel.tag))?;

    let exe = std::env::current_exe()?;
    let new = exe.with_extension("exe.new");
    let old = exe.with_extension("exe.old");

    println!("downloading pm {}…", rel.tag);
    let mut reader = ureq::get(&url)
        .set("User-Agent", UA)
        .timeout(std::time::Duration::from_secs(120))
        .call()?
        .into_reader();
    let mut file = std::fs::File::create(&new)?;
    std::io::copy(&mut reader, &mut file)?;
    drop(file);

    // Windows can't overwrite a running exe, but it *can* rename it.
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| {
        anyhow!("could not move the current pm.exe aside ({e}); re-run the install script instead")
    })?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe); // roll back
        bail!("could not put the new pm.exe in place ({e})");
    }

    println!("updated to {}. restarting…", rel.tag);
    std::process::Command::new(&exe).spawn()?;
    std::process::exit(0);
}

#[cfg(not(windows))]
pub fn run_self_update() -> Result<()> {
    bail!("`pm update` is Windows-only for now — use your package manager or rebuild from source");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parsing() {
        assert_eq!(parse_semver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_semver("0.0.1-rc.1"), Some((0, 0, 1)));
        assert_eq!(parse_semver("nightly"), None);
    }

    #[test]
    fn newer_comparison() {
        let mk = |v: (u32, u32, u32)| Release {
            tag: "t".into(),
            version: v,
            notes: String::new(),
            exe_url: None,
        };
        let cur = parse_semver(build::VERSION).unwrap();
        assert!(mk((cur.0, cur.1, cur.2 + 1)).is_newer_than_current());
        assert!(!mk(cur).is_newer_than_current());
        assert!(!mk((0, 0, 0)).is_newer_than_current());
    }
}
