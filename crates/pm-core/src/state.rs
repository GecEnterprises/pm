//! Application state: everything the diff viewer knows that isn't a pixel.
//! No gpui. The view layer (`pm-ui`) owns an `AppState` plus its own render state.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::content::{FileKind, ImageKind};
use crate::diff::{side_by_side, DiffRow};
use crate::git::{self, CommitInfo, DiffTarget, FileChange, Repo, TreeEntry};
use crate::highlight::{Highlighter, Line};
use crate::pm::{self, PmData};
use crate::text::{BufferPos, DiffCursor, DiffText};

/// Hard cap on the number of diff rows laid out.
pub const MAX_ROWS: usize = 200_000;

/// How the open file should be presented. `Text` means the line-diff fields
/// (`rows` / `old_lines` / `new_lines` / `text`) are populated; the other
/// variants leave them empty and drive a dedicated viewer.
#[derive(Default)]
pub enum Content {
    #[default]
    Text,
    Image {
        kind: ImageKind,
        /// Raw image bytes per side (`None` when that side doesn't exist).
        old: Option<Vec<u8>>,
        new: Option<Vec<u8>>,
    },
    /// A binary blob pm won't try to diff or render.
    Binary,
}

/// Display names for the Changes list: just the file name, disambiguated with the
/// parent directory only when two changed files share a base name.
fn compute_change_names(changes: &[FileChange]) -> Vec<String> {
    let mut counts: std::collections::HashMap<&OsStr, usize> = std::collections::HashMap::new();
    for c in changes {
        if let Some(n) = c.rel.file_name() {
            *counts.entry(n).or_default() += 1;
        }
    }
    changes
        .iter()
        .map(|c| {
            let base = c
                .rel
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ambiguous = c
                .rel
                .file_name()
                .and_then(|n| counts.get(n))
                .is_some_and(|&n| n > 1);
            match c.rel.parent().filter(|p| !p.as_os_str().is_empty()) {
                Some(parent) if ambiguous => {
                    format!("{base}   {}", parent.to_string_lossy())
                }
                _ => base,
            }
        })
        .collect()
}

pub struct AppState {
    pub repo: Repo,
    pub hl: Highlighter,
    pub changes: Vec<FileChange>,
    pub change_names: Vec<String>,
    /// Changed files plus every ancestor directory (O(1) tree tint tests).
    pub changed: HashSet<PathBuf>,
    /// Repo-relative path currently shown in the diff.
    pub open: Option<PathBuf>,
    pub rows: Vec<DiffRow>,
    /// Highlighted lines of the HEAD and working-tree versions of the open file.
    pub old_lines: Vec<Line>,
    pub new_lines: Vec<Line>,
    /// Raw source of both sides — the caret's buffer (see [`DiffText`]).
    pub text: DiffText,
    /// Per column, `file_row -> display row` (index into `rows`).
    pub col_display: [Vec<usize>; 2],
    // Explorer tree
    pub tree: Vec<TreeEntry>,
    pub expanded: HashSet<PathBuf>,
    pub visible: Vec<usize>,
    pub tree_selected: Option<PathBuf>,
    /// Read-only caret + selection over the open diff.
    pub caret: Option<DiffCursor>,
    /// Checked-out branch name, refreshed alongside git state.
    pub branch: Option<String>,
    /// Which viewer the open file needs (text / image / unviewable).
    pub content: Content,
    /// What the diff currently compares (working tree, or a commit vs its parent).
    pub target: DiffTarget,
    /// Recent commits, newest first — the Commit History pane.
    pub commits: Vec<CommitInfo>,
    /// The in-repo ticket store (`.pm/pm.json5`).
    pub pm: PmData,
    /// Last `pm.json5` load/save error, surfaced in the Tickets pane.
    pub pm_error: Option<String>,
    /// Default author for tickets / comments filed from this window (PM-15).
    /// Seeded from config / git; the Tickets pane "as:" field can override it
    /// per action.
    pub author: String,
    /// Directory that holds this project's `pm.json5` (PM-34) — `<root>/.pm` by
    /// default, or an out-of-repo store under `~/.pm/`.
    pub store_dir: PathBuf,
}

impl AppState {
    /// `store_dir` is the directory that holds `pm.json5` (PM-34); pass
    /// `repo.root().join(".pm")` for the in-repo default.
    pub fn new(repo: Repo, store_dir: PathBuf) -> Self {
        let target = DiffTarget::WorkingTree;
        let changes = repo.changes(target);
        let change_names = compute_change_names(&changes);
        let changed = repo.changed_set(target);
        let tree = repo.walk_tree();
        let branch = repo.branch();
        let commits = repo.log(git::LOG_LIMIT);
        let author = crate::identity::resolve_author(None, &repo);
        let (pm, pm_error) = match pm::load_in(&store_dir) {
            Ok(d) => (d, None),
            Err(pm::LoadError::Parse(s)) => (PmData::default(), Some(s)),
            // A read failure at startup is almost always transient; start empty
            // and let the first watcher tick pick the file up.
            Err(pm::LoadError::Io(s)) => {
                eprintln!("pm: {s}");
                (PmData::default(), None)
            }
        };
        let mut s = Self {
            repo,
            hl: Highlighter::new(),
            changes,
            change_names,
            changed,
            branch,
            target,
            commits,
            open: None,
            rows: Vec::new(),
            old_lines: Vec::new(),
            new_lines: Vec::new(),
            text: DiffText::default(),
            col_display: [Vec::new(), Vec::new()],
            tree,
            expanded: HashSet::new(),
            visible: Vec::new(),
            tree_selected: None,
            caret: None,
            content: Content::Text,
            pm,
            pm_error,
            author,
            store_dir,
        };
        s.rebuild_visible();
        // Open the first changed file, or — with no git — the first file in the
        // tree, so the window isn't blank.
        let first = s.changes.first().map(|c| c.rel.clone()).or_else(|| {
            s.tree
                .iter()
                .find(|e| !e.is_dir)
                .map(|e| e.rel.clone())
        });
        if let Some(rel) = first {
            s.tree_selected = Some(rel.clone());
            s.open_path(rel);
        }
        s
    }

    /// Whether the open folder is inside a git repository.
    pub fn is_git(&self) -> bool {
        self.repo.is_git()
    }

    /// Absolute path of the ticket store.
    pub fn pm_path(&self) -> PathBuf {
        self.store_dir.join("pm.json5")
    }

    /// Whether `p` names this project's `pm.json5`. Matches on the file name plus
    /// its parent directory's name — the watcher can spell the rest of the path
    /// differently (separators / prefix) on Windows.
    fn is_pm_store(&self, p: &Path) -> bool {
        if p.file_name() != Some(OsStr::new("pm.json5")) {
            return false;
        }
        let Some(parent) = p.parent() else { return false };
        parent == self.store_dir || parent.file_name() == self.store_dir.file_name()
    }

    /// Re-read `pm.json5` from disk. A read hiccup (watcher fired mid-write, an
    /// antivirus scan holding the file) keeps the last-good data with no error —
    /// the next watcher tick picks it up. Only a genuine parse failure surfaces.
    pub fn reload_pm(&mut self) {
        match pm::load_in(&self.store_dir) {
            Ok(d) => {
                self.pm = d;
                self.pm_error = None;
            }
            Err(pm::LoadError::Parse(s)) => self.pm_error = Some(s),
            Err(pm::LoadError::Io(_)) => {}
        }
    }

    fn save_pm(&mut self) {
        match self.pm.save_in(&self.store_dir) {
            Ok(()) => self.pm_error = None,
            Err(e) => self.pm_error = Some(e.to_string()),
        }
    }

    /// Resolve the author to record: an explicit non-empty name wins, else this
    /// window's default (`self.author`).
    fn author_or_default(&self, explicit: Option<String>) -> String {
        explicit
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.author.clone())
    }

    /// Create a ticket and persist. Returns its new id. `author` overrides this
    /// window's default when non-empty (PM-15).
    pub fn create_ticket(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        author: Option<String>,
    ) -> u64 {
        let author = self.author_or_default(author);
        let id = self.pm.create_ticket(title, body, author, pm::now_unix());
        self.save_pm();
        id
    }

    /// Add a comment to a ticket and persist. `author` overrides this window's
    /// default when non-empty (PM-15).
    pub fn add_comment(&mut self, ticket_id: u64, body: impl Into<String>, author: Option<String>) {
        let author = self.author_or_default(author);
        if self.pm.add_comment(ticket_id, author, body, pm::now_unix()) {
            self.save_pm();
        }
    }

    /// Change a ticket's status and persist. `author` overrides this window's
    /// default when non-empty (PM-15).
    pub fn set_ticket_status(
        &mut self,
        ticket_id: u64,
        status: crate::pm::Status,
        author: Option<String>,
    ) {
        let author = self.author_or_default(author);
        if self.pm.set_status(ticket_id, status, author, pm::now_unix()) {
            self.save_pm();
        }
    }

        pub fn open_path(&mut self, rel: PathBuf) {
        self.open = Some(rel.clone());
        self.caret = None;

        let old_bytes = self.repo.old_bytes(self.target, &rel);
        let new_bytes = self.repo.new_bytes(self.target, &rel);

        match FileKind::detect(&rel, &old_bytes, &new_bytes) {
            FileKind::Text => {
                let old = self.repo.old_content(self.target, &rel);
                let new = self.repo.new_content(self.target, &rel);
                self.old_lines = self.hl.highlight(&rel, &old);
                self.new_lines = self.hl.highlight(&rel, &new);
                self.rows = side_by_side(&old, &new);
                self.rebuild_col_display();
                self.text = DiffText::build(old, new);
                self.content = Content::Text;
            }
            FileKind::Image(kind) => {
                self.clear_text_content();
                self.content = Content::Image {
                    kind,
                    old: (!old_bytes.is_empty()).then_some(old_bytes),
                    new: (!new_bytes.is_empty()).then_some(new_bytes),
                };
            }
            FileKind::Binary => {
                self.clear_text_content();
                self.content = Content::Binary;
            }
        }
    }

    /// Drop the line-diff working set (used when the open file isn't text).
    fn clear_text_content(&mut self) {
        self.rows.clear();
        self.old_lines.clear();
        self.new_lines.clear();
        self.text = DiffText::default();
        self.col_display = [Vec::new(), Vec::new()];
    }

        /// Rebuild `col_display` from `rows` (call whenever `rows` changes).
    pub fn rebuild_col_display(&mut self) {
        let [l, r] = &mut self.col_display;
        l.clear();
        r.clear();
        for (di, row) in self.rows.iter().enumerate() {
            if row.left_no.is_some() {
                l.push(di);
            }
            if row.right_no.is_some() {
                r.push(di);
            }
        }
    }

        /// Recompute `visible` (indices into `tree`) from the expanded-dir set.
    pub fn rebuild_visible(&mut self) {
        self.visible.clear();
        for (i, e) in self.tree.iter().enumerate() {
            let shown = match e.rel.parent() {
                None => true,
                Some(parent) => {
                    parent.as_os_str().is_empty()
                        || parent
                            .ancestors()
                            .all(|a| a.as_os_str().is_empty() || self.expanded.contains(a))
                }
            };
            if shown {
                self.visible.push(i);
            }
        }
    }

    /// Re-derive git state; reopen the current file or clear if it's gone.
    pub fn refresh(&mut self) {
        self.reload_pm();
        self.commits = self.repo.log(git::LOG_LIMIT);
        // A pinned commit may have been rewritten/dropped — fall back to the
        // working tree if it's no longer in the log.
        if let DiffTarget::Commit(oid) = self.target {
            if !self.commits.iter().any(|c| c.id == oid) {
                self.target = DiffTarget::WorkingTree;
            }
        }
        self.changes = self.repo.changes(self.target);
        self.change_names = compute_change_names(&self.changes);
        self.changed = self.repo.changed_set(self.target);
        self.tree = self.repo.walk_tree();
        self.branch = self.repo.branch();
        self.rebuild_visible();
        match self.open.clone() {
            Some(rel) if self.changes.iter().any(|c| c.rel == rel) => self.open_path(rel),
            Some(rel)
                if self.target == DiffTarget::WorkingTree
                    && self.repo.root().join(&rel).exists() =>
            {
                self.open_path(rel)
            }
            _ => self.close_open(),
        }
    }

    fn close_open(&mut self) {
        self.open = None;
        self.clear_text_content();
        self.caret = None;
        self.content = Content::Text;
    }

    /// Switch what the diff compares, recompute the change list, and reopen.
    pub fn set_target(&mut self, target: DiffTarget) {
        self.target = target;
        self.changes = self.repo.changes(target);
        self.change_names = compute_change_names(&self.changes);
        self.changed = self.repo.changed_set(target);
        self.rebuild_visible();
        match self.open.clone() {
            Some(rel) if self.changes.iter().any(|c| c.rel == rel) => self.open_path(rel),
            _ => match self.changes.first().map(|c| c.rel.clone()) {
                Some(rel) => {
                    self.tree_selected = Some(rel.clone());
                    self.open_path(rel);
                }
                None => self.close_open(),
            },
        }
    }

    /// Re-derive git state after a filesystem change. Returns the path to reload
    /// (open file or `.git` touched), if any. No-op while a commit is pinned —
    /// working-tree edits don't affect a historical diff.
    pub fn apply_fs_change(&mut self, changed: &[PathBuf]) -> Option<PathBuf> {
        // Match by trailing components, not exact PathBuf — the watcher's paths
        // and `pm_path()` can differ by prefix / separators on Windows.
        if changed.iter().any(|p| self.is_pm_store(p)) {
            self.reload_pm();
        }
        if self.target != DiffTarget::WorkingTree {
            return None;
        }
        self.changes = self.repo.changes(self.target);
        self.change_names = compute_change_names(&self.changes);
        self.changed = self.repo.changed_set(self.target);
        self.tree = self.repo.walk_tree();
        self.branch = self.repo.branch();
        self.rebuild_visible();

        let rel = self.open.clone()?;
        let root = self.repo.root().to_path_buf();
        let abs = root.join(&rel);
        let git_touched = changed.iter().any(|p| {
            p.strip_prefix(&root)
                .map(|r| r.starts_with(".git"))
                .unwrap_or(false)
        });
        (git_touched || changed.contains(&abs)).then_some(rel)
    }

    /// The repo-relative path of a file in `changed` that shows in the current
    /// change list — the jump target for watch-jump mode (PM-30). Returns the
    /// last qualifying path in the batch; `None` if none qualify or the batch
    /// touched `.git` (a checkout / commit, not a plain edit). Matching against
    /// `self.changes` also excludes gitignored / untracked-ignored paths.
    pub fn changed_in_batch(&self, changed: &[PathBuf]) -> Option<PathBuf> {
        let root = self.repo.root();
        let touches_git = changed.iter().any(|p| {
            p.strip_prefix(root)
                .map(|r| r.starts_with(".git"))
                .unwrap_or(false)
        });
        if touches_git {
            return None;
        }
        changed.iter().rev().find_map(|p| {
            let rel = p.strip_prefix(root).ok()?;
            self.changes
                .iter()
                .any(|c| c.rel == rel)
                .then(|| rel.to_path_buf())
        })
    }

    pub fn diff_rows(&self) -> usize {
        self.rows.len().min(MAX_ROWS)
    }

    // ---- pure caret helpers -------------------------------------------------

    pub fn line_len(&self, col: usize, file_row: usize) -> usize {
        self.text.line(col, file_row).len()
    }

    /// Display row where `(col, file_row)`'s line is painted.
    pub fn disp_row(&self, col: usize, file_row: usize) -> Option<usize> {
        self.col_display[col].get(file_row).copied()
    }

    /// File line shown at `disp_row` in `col`; snaps to the nearest real line
    /// when that column has no line there.
    pub fn snap_file_row(&self, col: usize, disp_row: usize) -> usize {
        let no = |i: usize| match col {
            0 => self.rows[i].left_no,
            _ => self.rows[i].right_no,
        };
        if disp_row < self.rows.len() {
            if let Some(n) = no(disp_row) {
                return n - 1;
            }
        }
        for d in 1..self.rows.len().max(1) {
            if disp_row >= d {
                if let Some(n) = no(disp_row - d) {
                    return n - 1;
                }
            }
            if disp_row + d < self.rows.len() {
                if let Some(n) = no(disp_row + d) {
                    return n - 1;
                }
            }
        }
        0
    }

    pub fn word_bounds(&self, col: usize, file_row: usize, byte: usize) -> (usize, usize) {
        let s = self.text.line(col, file_row);
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut start = byte.min(s.len());
        while start > 0 {
            let prev = s[..start].chars().next_back().unwrap();
            if is_word(prev) {
                start -= prev.len_utf8();
            } else {
                break;
            }
        }
        let mut end = byte.min(s.len());
        while end < s.len() {
            let next = s[end..].chars().next().unwrap();
            if is_word(next) {
                end += next.len_utf8();
            } else {
                break;
            }
        }
        (start, end)
    }

    pub fn pos_left(&self, col: usize, p: BufferPos) -> BufferPos {
        if p.byte > 0 {
            let s = self.text.line(col, p.file_row);
            let mut b = p.byte.min(s.len()).saturating_sub(1);
            while b > 0 && !s.is_char_boundary(b) {
                b -= 1;
            }
            BufferPos { file_row: p.file_row, byte: b }
        } else if p.file_row > 0 {
            BufferPos {
                file_row: p.file_row - 1,
                byte: self.line_len(col, p.file_row - 1),
            }
        } else {
            p
        }
    }

    pub fn pos_right(&self, col: usize, p: BufferPos) -> BufferPos {
        let s = self.text.line(col, p.file_row);
        if p.byte < s.len() {
            let mut b = (p.byte + 1).min(s.len());
            while b < s.len() && !s.is_char_boundary(b) {
                b += 1;
            }
            BufferPos { file_row: p.file_row, byte: b }
        } else if p.file_row + 1 < self.text.line_count(col) {
            BufferPos { file_row: p.file_row + 1, byte: 0 }
        } else {
            p
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let cur = self.caret?;
        if !cur.has_selection() {
            return None;
        }
        let (a, b) = cur.ordered();
        Some(self.text.slice(cur.col, a, b).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_in_batch_guards_and_filters() {
        let repo = Repo::discover(Path::new(".")).unwrap();
        let store_dir = repo.root().join(".pm");
        let st = AppState::new(repo, store_dir);
        let root = st.repo.root().to_path_buf();

        // A `.git` touch is a checkout/commit, never a jump target.
        assert_eq!(st.changed_in_batch(&[root.join(".git/HEAD")]), None);
        // A path that isn't in the change list is ignored.
        assert_eq!(st.changed_in_batch(&[root.join("LICENSE")]), None);
        // A `.git` path anywhere in the batch vetoes the whole batch.
        let real = st.changes.first().map(|c| root.join(&c.rel));
        if let Some(real) = &real {
            assert_eq!(st.changed_in_batch(&[real.clone(), root.join(".git/x")]), None);
            // …but on its own it's the target.
            let want = st.changes.first().map(|c| c.rel.clone());
            assert_eq!(st.changed_in_batch(std::slice::from_ref(real)), want);
        }
    }
}
