//! Application state: everything the diff viewer knows that isn't a pixel.
//! No gpui. The view layer (`pm-ui`) owns an `AppState` plus its own render state.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;

use crate::diff::{side_by_side, DiffRow};
use crate::git::{FileChange, Repo, TreeEntry};
use crate::highlight::{Highlighter, Line};
use crate::text::{BufferPos, DiffCursor, DiffText};

/// Hard cap on the number of diff rows laid out.
pub const MAX_ROWS: usize = 200_000;

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
}

impl AppState {
    pub fn new(repo: Repo) -> Self {
        let changes = repo.changes();
        let change_names = compute_change_names(&changes);
        let changed = repo.changed_set();
        let tree = repo.walk_tree();
        let mut s = Self {
            repo,
            hl: Highlighter::new(),
            changes,
            change_names,
            changed,
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
        };
        s.rebuild_visible();
        if let Some(first) = s.changes.first().map(|c| c.rel.clone()) {
            s.tree_selected = Some(first.clone());
            s.open_path(first);
        }
        s
    }

        pub fn open_path(&mut self, rel: PathBuf) {
        let old = self.repo.head_content(&rel);
        let new = self.repo.working_content(&rel);
        self.old_lines = self.hl.highlight(&rel, &old);
        self.new_lines = self.hl.highlight(&rel, &new);
        self.rows = side_by_side(&old, &new);
        self.rebuild_col_display();
        self.text = DiffText::build(old, new);
        self.open = Some(rel);
        self.caret = None;
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
        self.changes = self.repo.changes();
        self.change_names = compute_change_names(&self.changes);
        self.changed = self.repo.changed_set();
        self.tree = self.repo.walk_tree();
        self.rebuild_visible();
        match self.open.clone() {
            Some(rel)
                if self.repo.root().join(&rel).exists()
                    || self.changes.iter().any(|c| c.rel == rel) =>
            {
                self.open_path(rel)
            }
            _ => {
                self.open = None;
                self.rows.clear();
                self.old_lines.clear();
                self.new_lines.clear();
                self.text = DiffText::default();
                self.col_display = [Vec::new(), Vec::new()];
                self.caret = None;
            }
        }
    }

    /// Re-derive git state after a filesystem change. Returns the path to reload
    /// (open file or `.git` touched), if any.
    pub fn apply_fs_change(&mut self, changed: &[PathBuf]) -> Option<PathBuf> {
        self.changes = self.repo.changes();
        self.change_names = compute_change_names(&self.changes);
        self.changed = self.repo.changed_set();
        self.tree = self.repo.walk_tree();
        self.rebuild_visible();

        let rel = self.open.clone()?;
        let root = self.repo.root().to_path_buf();
        let abs = root.join(&rel);
        let git_touched = changed.iter().any(|p| {
            p.strip_prefix(&root)
                .map(|r| r.starts_with(".git"))
                .unwrap_or(false)
        });
        (git_touched || changed.iter().any(|p| *p == abs)).then_some(rel)
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
