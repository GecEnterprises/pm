//! Thin wrapper over `git2` for the bits pm needs: which files changed, and the
//! "before" (HEAD) and "after" (working tree) contents of a given file.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use git2::{Delta, DiffOptions, Patch, Repository, Status, StatusOptions};
use ignore::WalkBuilder;

pub struct Repo {
    inner: Repository,
    root: PathBuf,
}

/// VS Code-style working-tree status for a changed file.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Other,
}

impl ChangeStatus {
    pub fn badge(self) -> &'static str {
        match self {
            ChangeStatus::Modified => "M",
            ChangeStatus::Added => "A",
            ChangeStatus::Deleted => "D",
            ChangeStatus::Renamed => "R",
            ChangeStatus::Untracked => "U",
            ChangeStatus::Other => "\u{2022}",
        }
    }

    pub fn color(self) -> u32 {
        match self {
            ChangeStatus::Modified => 0xe2c08d,
            ChangeStatus::Added | ChangeStatus::Untracked => 0x81b88b,
            ChangeStatus::Deleted => 0xc74e39,
            ChangeStatus::Renamed => 0x6ca4dc,
            ChangeStatus::Other => 0x808080,
        }
    }
}

/// One changed file, with its status and line-count delta.
pub struct FileChange {
    pub rel: PathBuf,
    pub status: ChangeStatus,
    pub added: usize,
    pub removed: usize,
}

/// One node of the working-tree file listing.
pub struct TreeEntry {
    /// Path relative to the repo root.
    pub rel: PathBuf,
    pub is_dir: bool,
    /// Number of ancestor directories (0 for repo-root children).
    pub depth: usize,
}

/// Collapse CRLF to LF so the diff compares content, not line endings. HEAD blobs
/// come out of git with LF; the working copy is whatever is on disk (CRLF under
/// `core.autocrlf` on Windows) — without this, every line reads as modified.
fn normalize_eol(s: &str) -> String {
    if s.contains('\r') {
        s.replace("\r\n", "\n")
    } else {
        s.to_owned()
    }
}

/// Pre-order DFS comparison: compare path components in lockstep; a prefix path
/// sorts before its descendants.
fn dfs_cmp(a: &Path, b: &Path) -> Ordering {
    let mut ac = a.components();
    let mut bc = b.components();
    loop {
        match (ac.next(), bc.next()) {
            (Some(x), Some(y)) => match x.as_os_str().cmp(y.as_os_str()) {
                Ordering::Equal => continue,
                other => return other,
            },
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

impl Repo {
    /// Walk up from `path` looking for a `.git` directory.
    pub fn discover(path: &Path) -> Result<Self> {
        let inner = Repository::discover(path)?;
        let root = inner
            .workdir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf());
        Ok(Self { inner, root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Paths (relative to the repo root) with unstaged or staged changes, or that
    /// are untracked. Sorted and de-duplicated.
    pub fn changed_files(&self) -> Result<Vec<PathBuf>> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);

        let interesting = Status::WT_MODIFIED
            | Status::WT_NEW
            | Status::WT_DELETED
            | Status::WT_RENAMED
            | Status::INDEX_MODIFIED
            | Status::INDEX_NEW
            | Status::INDEX_DELETED
            | Status::INDEX_RENAMED;

        let mut out = Vec::new();
        for entry in self.inner.statuses(Some(&mut opts))?.iter() {
            if entry.status().intersects(interesting) {
                if let Some(p) = entry.path() {
                    out.push(PathBuf::from(p));
                }
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// Every changed file (staged, unstaged, or untracked) with its status and
    /// added/removed line counts.
    pub fn changes(&self) -> Vec<FileChange> {
        let head_tree = self.inner.head().ok().and_then(|h| h.peel_to_tree().ok());
        let mut opts = DiffOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true)
            .include_typechange(true);

        let Ok(diff) = self
            .inner
            .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))
        else {
            return Vec::new();
        };

        let mut out = Vec::new();
        for (i, delta) in diff.deltas().enumerate() {
            let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) else {
                continue;
            };
            let status = match delta.status() {
                Delta::Added => ChangeStatus::Added,
                Delta::Deleted => ChangeStatus::Deleted,
                Delta::Modified | Delta::Typechange => ChangeStatus::Modified,
                Delta::Renamed | Delta::Copied => ChangeStatus::Renamed,
                Delta::Untracked => ChangeStatus::Untracked,
                _ => ChangeStatus::Other,
            };
            let (added, removed) = Patch::from_diff(&diff, i)
                .ok()
                .flatten()
                .and_then(|p| p.line_stats().ok())
                .map(|(_, a, d)| (a, d))
                .unwrap_or((0, 0));
            out.push(FileChange {
                rel: path.to_path_buf(),
                status,
                added,
                removed,
            });
        }
        out.sort_by(|a, b| a.rel.cmp(&b.rel));
        out
    }

    /// File contents at HEAD, or empty string if the file is new / unreadable.
    pub fn head_content(&self, rel: &Path) -> String {
        self.try_head_content(rel).unwrap_or_default()
    }

    fn try_head_content(&self, rel: &Path) -> Result<String> {
        let tree = self.inner.head()?.peel_to_tree()?;
        let entry = tree.get_path(rel)?;
        let blob = self.inner.find_blob(entry.id())?;
        Ok(normalize_eol(&String::from_utf8_lossy(blob.content())))
    }

    /// Current on-disk contents, or empty string if the file was deleted.
    pub fn working_content(&self, rel: &Path) -> String {
        std::fs::read_to_string(self.root.join(rel))
            .map(|s| normalize_eol(&s))
            .unwrap_or_default()
    }

    /// Short name of the checked-out branch (`"HEAD"` when detached), or `None`
    /// if HEAD can't be resolved (e.g. an unborn branch in a fresh repo).
    pub fn branch(&self) -> Option<String> {
        self.inner
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(str::to_owned))
    }

    /// gitignore-aware DFS pre-order walk of the working tree. Siblings sorted
    /// by name, directories interleaved with files.
    pub fn walk_tree(&self) -> Vec<TreeEntry> {
        let mut raw: Vec<(PathBuf, bool)> = Vec::new();
        let walker = WalkBuilder::new(&self.root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .filter_entry(|e| e.file_name() != ".git")
            .build();
        for dent in walker {
            let Ok(dent) = dent else { continue };
            let Ok(rel) = dent.path().strip_prefix(&self.root) else {
                continue;
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            let is_dir = dent.file_type().is_some_and(|t| t.is_dir());
            raw.push((rel.to_path_buf(), is_dir));
        }
        raw.sort_by(|a, b| dfs_cmp(&a.0, &b.0));
        raw.into_iter()
            .map(|(rel, is_dir)| {
                let depth = rel.components().count().saturating_sub(1);
                TreeEntry { rel, is_dir, depth }
            })
            .collect()
    }

    /// Every changed file plus all of its ancestor directories, for O(1) tree
    /// tint tests.
    pub fn changed_set(&self) -> HashSet<PathBuf> {
        let mut set = HashSet::new();
        for path in self.changed_files().unwrap_or_default() {
            for ancestor in path.ancestors() {
                if ancestor.as_os_str().is_empty() {
                    break;
                }
                set.insert(ancestor.to_path_buf());
            }
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_own_repo() {
        let repo = Repo::discover(Path::new(".")).unwrap();
        let tree = repo.walk_tree();
        assert!(!tree.is_empty());
        assert!(tree.iter().any(|e| e.rel == Path::new("Cargo.toml") && !e.is_dir));
        assert!(tree.iter().any(|e| e.rel == Path::new("src") && e.is_dir));
        // .git excluded, target/ gitignored
        assert!(!tree.iter().any(|e| e.rel.starts_with(".git")));
        assert!(!tree.iter().any(|e| e.rel.starts_with("target")));
    }

    #[test]
    fn branch_of_own_repo() {
        let repo = Repo::discover(Path::new(".")).unwrap();
        assert!(repo.branch().is_some());
    }
}
