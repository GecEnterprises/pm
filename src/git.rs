//! Thin wrapper over `git2` for the bits pm needs: which files changed, and the
//! "before" (HEAD) and "after" (working tree) contents of a given file.

use std::path::{Path, PathBuf};

use anyhow::Result;
use git2::{Repository, Status, StatusOptions};

pub struct Repo {
    inner: Repository,
    root: PathBuf,
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

    /// File contents at HEAD, or empty string if the file is new / unreadable.
    pub fn head_content(&self, rel: &Path) -> String {
        self.try_head_content(rel).unwrap_or_default()
    }

    fn try_head_content(&self, rel: &Path) -> Result<String> {
        let tree = self.inner.head()?.peel_to_tree()?;
        let entry = tree.get_path(rel)?;
        let blob = self.inner.find_blob(entry.id())?;
        Ok(String::from_utf8_lossy(blob.content()).into_owned())
    }

    /// Current on-disk contents, or empty string if the file was deleted.
    pub fn working_content(&self, rel: &Path) -> String {
        std::fs::read_to_string(self.root.join(rel)).unwrap_or_default()
    }
}
