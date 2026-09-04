//! Who a ticket / comment is attributed to (PM-15).
//!
//! Authorship in `pm` is a free-form, unverified string: the GUI and the MCP
//! server may both write any name. This module just resolves the *default* name
//! to use when the caller didn't pick one explicitly.

use crate::config::Config;
use crate::git::Repo;

/// The author name to record, in precedence order:
/// 1. `explicit` (a name the caller passed — e.g. the GUI "as:" field or an MCP
///    tool argument), if non-empty;
/// 2. the `author` field of the user config (`~/.pm/config.json`);
/// 3. the repository's git `user.name`;
/// 4. `"unknown"`.
pub fn resolve_author(explicit: Option<&str>, repo: &Repo) -> String {
    if let Some(name) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return name.to_string();
    }
    let cfg = Config::load();
    if !cfg.author.trim().is_empty() {
        return cfg.author.trim().to_string();
    }
    repo.user_name()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}
