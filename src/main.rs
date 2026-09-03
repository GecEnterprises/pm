//! pm (Plus Minus) - a diff-oriented code viewer.
//!
//! Opens a folder (arg 1, or the current directory), finds the enclosing git
//! repository, and shows changed files with a side-by-side line diff.
//!
//! Layout is code-editor shaped: a resizable left sidebar with two collapsible
//! sections ("Changes" = [`list_view`], "Explorer" = [`tree_view`]) and a center
//! diff pane ([`diff_view`]) whose HEAD/working split is itself draggable. The
//! diff body and both side panels are custom gpui [`Element`]s that own a pixel
//! scroll offset and paint only their visible rows.

// Release builds are a GUI app with no console window; debug builds keep the
// console so `eprintln!` / panics are visible during development.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diff;
mod diff_view;
mod git;
mod highlight;
mod icons;
mod list_view;
mod scroll;
mod tree_view;
mod watch;

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::Duration;

use diff::{side_by_side, DiffRow};
use diff_view::{diff_view, ShapeCache};
use git::{FileChange, Repo, TreeEntry};
use gpui::{
    canvas, deferred, div, prelude::*, px, rgb, size, App, Bounds, ClipboardItem, Context,
    DragMoveEvent, Empty, FocusHandle, KeyDownEvent, MouseButton, SharedString, Window,
    WindowBounds, WindowOptions,
};
use gpui_platform::application;
use highlight::{Highlighter, Line};
use list_view::list_view;
use scroll::{ScrollDrag, ScrollState};
use tree_view::tree_view;
use watch::Sentinel;

pub(crate) const BG: u32 = 0x1e1e1e;
pub(crate) const PANEL: u32 = 0x252526;
pub(crate) const BORDER: u32 = 0x333333;
pub(crate) const TEXT: u32 = 0xd4d4d4;
pub(crate) const DIM: u32 = 0x808080;
pub(crate) const SELECT: u32 = 0x094771;
pub(crate) const ADD_BG: u32 = 0x18321f;
pub(crate) const DEL_BG: u32 = 0x3a1d1d;
/// Tint for changed files / dirs in the Explorer tree.
pub(crate) const CHANGED: u32 = 0x4ec9b0;

/// Diff row height / line-height, in px.
pub(crate) const ROW_H: f32 = 18.0;
/// "Changes" list row height, in px.
pub(crate) const LIST_ROW_H: f32 = 24.0;
/// Explorer tree row height, in px.
pub(crate) const TREE_ROW_H: f32 = 22.0;
/// Explorer indent per depth level, in px.
pub(crate) const TREE_INDENT: f32 = 14.0;
/// File-type icon size, in px.
pub(crate) const ICON_SIZE: f32 = 14.0;
/// Scrollbar track thickness, in px.
pub(crate) const BAR: f32 = 12.0;
/// Line-number column width, in px.
pub(crate) const GUTTER_W: f32 = 52.0;
/// Line-number column right padding, in px.
pub(crate) const GUTTER_PAD: f32 = 8.0;
/// Text left padding inside a diff column, in px.
pub(crate) const TEXT_PAD_L: f32 = 8.0;
/// Centre divider width, in px.
pub(crate) const DIVIDER_W: f32 = 1.0;
/// Monospace font for diff text.
pub(crate) const BODY_FONT: &str = "Consolas";
pub(crate) const BODY_FONT_SIZE: f32 = 12.5;
/// Hard cap on the number of diff rows laid out.
pub(crate) const MAX_ROWS: usize = 200_000;

pub(crate) const SIDEBAR_MIN: f32 = 180.0;
/// The diff pane always keeps at least this many px.
pub(crate) const SIDEBAR_MAX_MARGIN: f32 = 320.0;
pub(crate) const SECTION_HEADER_H: f32 = 26.0;
pub(crate) const SECTION_SPLIT_H: f32 = 6.0;
pub(crate) const RESIZE_HANDLE_W: f32 = 6.0;
pub(crate) const DIFF_SPLIT_MIN: f32 = 0.15;
pub(crate) const DIFF_SPLIT_MAX: f32 = 0.85;

/// A caret position in *buffer* space: a line of one column's source file plus a
/// char-boundary byte offset into that line. Zed's `Point`, scaled down — the
/// diff's visual rows are a display layer on top of this (see [`Pm::col_display`]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct BufferPos {
    /// Zero-based line index into the column's source (`old` for col 0, `new` for col 1).
    pub file_row: usize,
    pub byte: usize,
}

/// A caret + selection within one diff column (`col`: 0 = HEAD, 1 = working).
/// `anchor`/`head` are buffer positions; `goal_x` is the preferred x for vertical
/// movement (Zed's `SelectionGoal`).
#[derive(Clone, Copy)]
pub(crate) struct DiffCursor {
    pub col: usize,
    pub anchor: BufferPos,
    pub head: BufferPos,
    pub goal_x: Option<f32>,
}

impl DiffCursor {
    pub fn has_selection(&self) -> bool {
        self.anchor != self.head
    }
    pub fn ordered(&self) -> (BufferPos, BufferPos) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

/// The two source files behind the open diff, one per column (`[old, new]`), each
/// split into line byte-ranges (newline excluded). This is the caret's "buffer":
/// selection and copy slice straight out of `src`, so lines that exist only on the
/// other column can't leak in.
#[derive(Default)]
pub(crate) struct DiffText {
    src: [String; 2],
    lines: [Vec<std::ops::Range<usize>>; 2],
}

impl DiffText {
    fn build(old: String, new: String) -> Self {
        let lines = [line_ranges(&old), line_ranges(&new)];
        Self { src: [old, new], lines }
    }

    pub fn line_count(&self, col: usize) -> usize {
        self.lines[col].len()
    }

    pub fn line(&self, col: usize, file_row: usize) -> &str {
        self.lines[col]
            .get(file_row)
            .map_or("", |r| &self.src[col][r.clone()])
    }

    /// Absolute byte offset in `src[col]` of `byte` within line `file_row`,
    /// clamped to the line's end.
    fn offset(&self, col: usize, file_row: usize, byte: usize) -> usize {
        match self.lines[col].get(file_row) {
            Some(r) => (r.start + byte).min(r.end),
            None => self.src[col].len(),
        }
    }

    /// Source text between two ordered buffer positions in the same column.
    fn slice(&self, col: usize, a: BufferPos, b: BufferPos) -> &str {
        let s = self.offset(col, a.file_row, a.byte);
        let e = self.offset(col, b.file_row, b.byte).max(s);
        &self.src[col][s..e]
    }

    /// Snap a buffer position onto real text: clamp the row into range and the
    /// byte onto a char boundary within that line. Zed's `clip_point`.
    fn clip(&self, col: usize, p: BufferPos) -> BufferPos {
        let last = self.line_count(col).saturating_sub(1);
        let file_row = p.file_row.min(last);
        let s = self.line(col, file_row);
        let mut byte = p.byte.min(s.len());
        while byte > 0 && !s.is_char_boundary(byte) {
            byte -= 1;
        }
        BufferPos { file_row, byte }
    }
}

/// Byte ranges of each line in `s` (trailing `\r`/`\n` excluded). Matches the line
/// count `diff::side_by_side` produces for the same text.
fn line_ranges(s: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = s.as_bytes();
    let mut v = Vec::new();
    let mut start = 0;
    for i in 0..bytes.len() {
        if bytes[i] == b'\n' {
            let end = if i > start && bytes[i - 1] == b'\r' { i - 1 } else { i };
            v.push(start..end);
            start = i + 1;
        }
    }
    if start < bytes.len() {
        let end = if bytes[bytes.len() - 1] == b'\r' { bytes.len() - 1 } else { bytes.len() };
        v.push(start..end.max(start));
    }
    v
}

/// Scroll state for the diff body: one shared vertical offset, one horizontal
/// offset per column.
#[derive(Default)]
pub(crate) struct DiffScroll {
    pub y: ScrollState,
    pub x: [ScrollState; 2],
    pub drag: Option<ScrollDrag>,
}

/// Drag payload for the panel resize handles.
#[derive(Clone)]
enum ResizeHandle {
    Sidebar,
    SectionSplit,
}

/// The (invisible) drag-preview view required by `on_drag`.
#[derive(Clone)]
struct DragPreview;
impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone, Copy)]
enum Section {
    Changes,
    Explorer,
}
impl Section {
    fn toggle(self, pm: &mut Pm) {
        match self {
            Section::Changes => pm.changes_collapsed = !pm.changes_collapsed,
            Section::Explorer => pm.explorer_collapsed = !pm.explorer_collapsed,
        }
    }
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

pub(crate) struct Pm {
    repo: Repo,
    hl: Highlighter,
    changes: Vec<FileChange>,
    change_names: Vec<String>,
    /// Changed files plus every ancestor directory (O(1) tree tint tests).
    changed: HashSet<PathBuf>,
    /// Repo-relative path currently shown in the diff.
    open: Option<PathBuf>,
    rows: Vec<DiffRow>,
    /// Highlighted lines of the HEAD and working-tree versions of the open file.
    old_lines: Vec<Line>,
    new_lines: Vec<Line>,
    /// Raw source of both sides — the caret's buffer (see [`DiffText`]).
    text: DiffText,
    /// Per column, `file_row -> display row` (index into `rows`). The inverse of
    /// `rows[i].left_no` / `right_no`; every real file line maps to exactly one row.
    col_display: [Vec<usize>; 2],
    /// Shaped diff lines for the open file (cleared on file switch).
    shaped: ShapeCache,
    diff: DiffScroll,

    list_scroll: ScrollState,
    list_drag: Option<ScrollDrag>,
    hover_file: Option<usize>,

    // layout prefs — persist across file switches, re-clamped on window resize
    sidebar_w: f32,
    changes_h: f32,
    diff_split: f32,
    changes_collapsed: bool,
    explorer_collapsed: bool,

    root_bounds: Bounds<gpui::Pixels>,
    diff_split_drag: Option<f32>,

    // Explorer tree
    tree: Vec<TreeEntry>,
    expanded: HashSet<PathBuf>,
    visible: Vec<usize>,
    tree_scroll: ScrollState,
    tree_drag: Option<ScrollDrag>,
    tree_hover: Option<usize>,
    tree_selected: Option<PathBuf>,

    /// Filesystem watcher; polled by a foreground task (see `start_watch`).
    sentinel: Option<Sentinel>,

    // read-only text-editor layer over the diff
    diff_focus: FocusHandle,
    caret: Option<DiffCursor>,
    text_drag: bool,
    /// Middle-click autoscroll origin, in window space.
    autoscroll: Option<(f32, f32)>,
    mouse_pos: (f32, f32),
    /// Diff viewport height in px, captured each prepaint (page-scroll + follow).
    diff_viewport_h: f32,
}

impl Pm {
    fn new(repo: Repo, cx: &mut Context<Self>) -> Self {
        let changes = repo.changes();
        let change_names = compute_change_names(&changes);
        let changed = repo.changed_set();
        let tree = repo.walk_tree();
        let sentinel = Sentinel::start(repo.root().to_path_buf())
            .map_err(|e| eprintln!("pm: filesystem watch unavailable ({e})"))
            .ok();
        let mut pm = Self {
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
            shaped: ShapeCache::default(),
            diff: DiffScroll::default(),
            list_scroll: ScrollState::default(),
            list_drag: None,
            hover_file: None,
            sidebar_w: 280.0,
            changes_h: 220.0,
            diff_split: 0.5,
            changes_collapsed: false,
            explorer_collapsed: false,
            root_bounds: Bounds::default(),
            diff_split_drag: None,
            tree,
            expanded: HashSet::new(),
            visible: Vec::new(),
            tree_scroll: ScrollState::default(),
            tree_drag: None,
            tree_hover: None,
            tree_selected: None,
            sentinel,
            diff_focus: cx.focus_handle(),
            caret: None,
            text_drag: false,
            autoscroll: None,
            mouse_pos: (0.0, 0.0),
            diff_viewport_h: 0.0,
        };
        pm.rebuild_visible();
        if let Some(first) = pm.changes.first().map(|c| c.rel.clone()) {
            pm.tree_selected = Some(first.clone());
            pm.open_path(first);
        }
        pm
    }

    /// Load `rel` into the diff. Leaves layout prefs (`diff_split`) untouched.
    fn open_path(&mut self, rel: PathBuf) {
        let old = self.repo.head_content(&rel);
        let new = self.repo.working_content(&rel);
        self.old_lines = self.hl.highlight(&rel, &old);
        self.new_lines = self.hl.highlight(&rel, &new);
        self.rows = side_by_side(&old, &new);
        self.rebuild_col_display();
        self.text = DiffText::build(old, new);
        self.open = Some(rel);
        self.shaped.clear();
        self.diff.y = ScrollState::default();
        self.diff.x = [ScrollState::default(); 2];
        self.diff.drag = None;
        self.caret = None;
        self.text_drag = false;
        self.autoscroll = None;
    }

    /// Rebuild `col_display` from `rows` (call whenever `rows` changes).
    fn rebuild_col_display(&mut self) {
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
    fn rebuild_visible(&mut self) {
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

    fn clamp_layout(&mut self, root: Bounds<gpui::Pixels>) {
        let rw = f32::from(root.size.width);
        let rh = f32::from(root.size.height);
        let max_sb = (rw - SIDEBAR_MAX_MARGIN).max(SIDEBAR_MIN);
        self.sidebar_w = self.sidebar_w.clamp(SIDEBAR_MIN, max_sb);
        let avail = (rh - 2.0 * SECTION_HEADER_H - SECTION_SPLIT_H).max(0.0);
        self.changes_h = self.changes_h.clamp(0.0, avail);
        self.diff_split = self.diff_split.clamp(DIFF_SPLIT_MIN, DIFF_SPLIT_MAX);
    }

    fn refresh(&mut self) {
        self.changes = self.repo.changes();
        self.change_names = compute_change_names(&self.changes);
        self.changed = self.repo.changed_set();
        self.tree = self.repo.walk_tree();
        self.rebuild_visible();
        self.hover_file = None;
        self.tree_hover = None;
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
                self.shaped.clear();
                self.diff = DiffScroll::default();
            }
        }
    }

    /// Spawn the foreground loop that drains the Sentinel and applies changes.
    fn start_watch(&mut self, cx: &mut Context<Self>) {
        if self.sentinel.is_none() {
            return;
        }
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;
            let batch = this.update(cx, |pm, _| {
                pm.sentinel
                    .as_ref()
                    .and_then(|s| s.poll(Duration::from_millis(250)))
            });
            match batch {
                Ok(Some(paths)) => {
                    let _ = this.update(cx, |pm, cx| pm.on_fs_change(&paths, cx));
                }
                Ok(None) => {}
                Err(_) => break, // entity gone — window closed
            }
        })
        .detach();
    }

    /// Re-derive git state and, if the open file (or `.git`) changed, reload the
    /// diff while keeping the scroll position.
    fn on_fs_change(&mut self, changed: &[PathBuf], cx: &mut Context<Self>) {
        let root = self.repo.root().to_path_buf();
        self.changes = self.repo.changes();
        self.change_names = compute_change_names(&self.changes);
        self.changed = self.repo.changed_set();
        self.tree = self.repo.walk_tree();
        self.rebuild_visible();
        self.hover_file = None;
        self.tree_hover = None;

        if let Some(rel) = self.open.clone() {
            let abs = root.join(&rel);
            let git_touched = changed.iter().any(|p| {
                p.strip_prefix(&root)
                    .map(|r| r.starts_with(".git"))
                    .unwrap_or(false)
            });
            if git_touched || changed.iter().any(|p| *p == abs) {
                self.reload_open_keep_scroll(rel);
            }
        }
        cx.notify();
    }

    fn reload_open_keep_scroll(&mut self, rel: PathBuf) {
        let (y, x0, x1) = (
            self.diff.y.offset,
            self.diff.x[0].offset,
            self.diff.x[1].offset,
        );
        let caret = self.caret;
        self.open_path(rel);
        self.diff.y.offset = y;
        self.diff.x[0].offset = x0;
        self.diff.x[1].offset = x1;
        self.caret = caret;
    }

    // ---- read-only text-editor layer -----------------------------------------
    //
    // The caret lives in buffer space (`BufferPos`: a source line + byte). Display
    // rows are just a layer on top: `col_display` maps a file line to the one
    // display row that shows it, and `rows[i].left_no`/`right_no` map back.
    // Movement and selection never touch display rows, so a block that exists only
    // on the other column simply isn't reachable — no phantom blank lines.

    fn line_len(&self, col: usize, file_row: usize) -> usize {
        self.text.line(col, file_row).len()
    }

    /// Display row where `(col, file_row)`'s line is painted.
    fn disp_row(&self, col: usize, file_row: usize) -> Option<usize> {
        self.col_display[col].get(file_row).copied()
    }

    /// File line shown at `disp_row` in `col`; if that column has no line there
    /// (a blank spacer opposite a one-sided block), snap to the nearest one.
    fn snap_file_row(&self, col: usize, disp_row: usize) -> usize {
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

    /// Byte offset nearest `x_local` px within `(col, file_row)`'s shaped line.
    fn byte_at_x(&self, col: usize, file_row: usize, x_local: f32) -> usize {
        match self.disp_row(col, file_row) {
            Some(d) => self.shaped.byte_at_x(col, d, x_local),
            None => 0,
        }
    }

    /// Place / extend the caret from a click in column `col` at display `disp_row`,
    /// `x_local` px into the shaped line.
    fn click_text(
        &mut self,
        col: usize,
        disp_row: usize,
        x_local: f32,
        shift: bool,
        clicks: usize,
        cx: &mut Context<Self>,
    ) {
        let row = self.snap_file_row(col, disp_row);
        let byte = self.byte_at_x(col, row, x_local);
        let at = BufferPos { file_row: row, byte };
        if clicks >= 3 {
            self.caret = Some(DiffCursor {
                col,
                anchor: BufferPos { file_row: row, byte: 0 },
                head: BufferPos { file_row: row, byte: self.line_len(col, row) },
                goal_x: None,
            });
            self.text_drag = false;
        } else if clicks == 2 {
            let (s, e) = self.word_bounds(col, row, byte);
            self.caret = Some(DiffCursor {
                col,
                anchor: BufferPos { file_row: row, byte: s },
                head: BufferPos { file_row: row, byte: e },
                goal_x: None,
            });
            self.text_drag = false;
        } else {
            let anchor = if shift {
                self.caret.filter(|c| c.col == col).map_or(at, |c| c.anchor)
            } else {
                at
            };
            self.caret = Some(DiffCursor { col, anchor, head: at, goal_x: None });
            self.text_drag = true;
        }
        cx.notify();
    }

    /// Extend the selection head to a drag position (same column as the caret).
    fn drag_text(&mut self, disp_row: usize, x_local: f32, cx: &mut Context<Self>) {
        let Some(mut cur) = self.caret else { return };
        let row = self.snap_file_row(cur.col, disp_row);
        cur.head = BufferPos { file_row: row, byte: self.byte_at_x(cur.col, row, x_local) };
        cur.goal_x = None;
        self.caret = Some(cur);
        self.scroll_caret_into_view();
        cx.notify();
    }

    fn word_bounds(&self, col: usize, file_row: usize, byte: usize) -> (usize, usize) {
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

    fn pos_left(&self, col: usize, p: BufferPos) -> BufferPos {
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

    fn pos_right(&self, col: usize, p: BufferPos) -> BufferPos {
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

    fn selected_text(&self) -> Option<String> {
        let cur = self.caret?;
        if !cur.has_selection() {
            return None;
        }
        let (a, b) = cur.ordered();
        Some(self.text.slice(cur.col, a, b).to_string())
    }

    fn scroll_caret_into_view(&mut self) {
        let Some(cur) = self.caret else { return };
        let vh = self.diff_viewport_h;
        if vh <= 0.0 {
            return;
        }
        let Some(disp) = self.disp_row(cur.col, cur.head.file_row) else { return };
        let caret_top = disp as f32 * ROW_H;
        let off = f32::from(self.diff.y.offset.y);
        let new = if caret_top < off {
            caret_top
        } else if caret_top + ROW_H > off + vh {
            caret_top + ROW_H - vh
        } else {
            off
        };
        self.diff.y.offset.y = px(new.max(0.0));
        self.diff.y.clamp();
    }

    fn on_diff_key(&mut self, e: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = e.keystroke.key.as_str();
        let shift = e.keystroke.modifiers.shift;
        let ctrl = e.keystroke.modifiers.secondary();

        let default_col = if self.text.line_count(1) > 0 { 1 } else { 0 };
        let mut cur = self.caret.unwrap_or(DiffCursor {
            col: default_col,
            anchor: BufferPos { file_row: 0, byte: 0 },
            head: BufferPos { file_row: 0, byte: 0 },
            goal_x: None,
        });
        let lines = self.text.line_count(cur.col);
        if lines == 0 {
            return;
        }
        let last = lines - 1;

        match key {
            "c" if ctrl => {
                if let Some(text) = self.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                cx.stop_propagation();
                return;
            }
            "a" if ctrl => {
                cur.anchor = BufferPos { file_row: 0, byte: 0 };
                cur.head = BufferPos { file_row: last, byte: self.line_len(cur.col, last) };
                cur.goal_x = None;
                self.caret = Some(cur);
                cx.notify();
                cx.stop_propagation();
                return;
            }
            "escape" => {
                cur.anchor = cur.head;
                self.caret = Some(cur);
                cx.notify();
                return;
            }
            "left" => {
                cur.head = self.pos_left(cur.col, cur.head);
                cur.goal_x = None;
            }
            "right" => {
                cur.head = self.pos_right(cur.col, cur.head);
                cur.goal_x = None;
            }
            "home" => {
                cur.head = if ctrl {
                    BufferPos { file_row: 0, byte: 0 }
                } else {
                    BufferPos { file_row: cur.head.file_row, byte: 0 }
                };
                cur.goal_x = None;
            }
            "end" => {
                cur.head = if ctrl {
                    BufferPos { file_row: last, byte: self.line_len(cur.col, last) }
                } else {
                    BufferPos {
                        file_row: cur.head.file_row,
                        byte: self.line_len(cur.col, cur.head.file_row),
                    }
                };
                cur.goal_x = None;
            }
            "up" | "down" | "pageup" | "pagedown" => {
                let g = match cur.goal_x {
                    Some(g) => g,
                    None => {
                        let d = self.disp_row(cur.col, cur.head.file_row);
                        let x = d
                            .and_then(|d| self.shaped.line_x(cur.col, d, cur.head.byte))
                            .unwrap_or(0.0);
                        cur.goal_x = Some(x);
                        x
                    }
                };
                let page = ((self.diff_viewport_h / ROW_H).floor() as isize - 1).max(1);
                let step: isize = match key {
                    "up" => -1,
                    "down" => 1,
                    "pageup" => -page,
                    _ => page,
                };
                let nr = (cur.head.file_row as isize + step).clamp(0, last as isize) as usize;
                cur.head = BufferPos { file_row: nr, byte: self.byte_at_x(cur.col, nr, g) };
            }
            _ => return,
        }

        cur.head = self.text.clip(cur.col, cur.head);
        if !shift {
            cur.anchor = cur.head;
        }
        self.caret = Some(cur);
        self.scroll_caret_into_view();
        cx.notify();
        cx.stop_propagation();
    }
}

impl Render for Pm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        div()
            .id("pm-root")
            .flex()
            .flex_row()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family("Segoe UI")
            .text_size(px(13.))
            .child(
                canvas(
                    {
                        let entity = entity.clone();
                        move |b, _w, cx| {
                            entity.update(cx, |pm, _| {
                                pm.root_bounds = b;
                                pm.clamp_layout(b);
                            })
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .on_drag_move(cx.listener(
                |pm, ev: &DragMoveEvent<ResizeHandle>, _window, cx| {
                    let root = pm.root_bounds;
                    let x = f32::from(ev.event.position.x) - f32::from(root.left());
                    let y = f32::from(ev.event.position.y) - f32::from(root.top());
                    match ev.drag(cx) {
                        ResizeHandle::Sidebar => pm.sidebar_w = x,
                        ResizeHandle::SectionSplit => pm.changes_h = y - SECTION_HEADER_H,
                    }
                    pm.clamp_layout(root);
                    cx.notify();
                },
            ))
            .child(self.left_column(cx))
            .child(self.diff_pane(cx))
    }
}

impl Pm {
    fn section_header(
        &self,
        cx: &mut Context<Self>,
        title: &'static str,
        count: Option<usize>,
        collapsed: bool,
        which: Section,
    ) -> impl IntoElement {
        let chevron = if collapsed { "▸" } else { "▾" };
        let label = match count {
            Some(n) => format!("{chevron}  {title}  ({n})"),
            None => format!("{chevron}  {title}"),
        };
        let mut header = div()
            .id(title)
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(SECTION_HEADER_H))
            .px_2()
            .cursor_pointer()
            .text_color(rgb(DIM))
            .hover(|s| s.bg(rgb(BORDER)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |pm, _, _, cx| {
                    which.toggle(pm);
                    cx.notify();
                }),
            )
            .child(SharedString::from(label));

        if matches!(which, Section::Changes) {
            header = header.child(
                div()
                    .id("refresh")
                    .px_1()
                    .rounded_sm()
                    .hover(|s| s.bg(rgb(BORDER)))
                    .child("⟳")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.refresh();
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    ),
            );
        }
        header
    }

    fn left_column(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let e = cx.entity();
        let changes_open = !self.changes_collapsed;
        let explorer_open = !self.explorer_collapsed;

        let mut col = div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(self.sidebar_w))
            .h_full()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(self.section_header(
                cx,
                "Changes",
                Some(self.changes.len()),
                self.changes_collapsed,
                Section::Changes,
            ));

        if changes_open {
            let body = div()
                .relative()
                .overflow_hidden()
                .child(list_view(e.clone()));
            col = col.child(if explorer_open {
                body.h(px(self.changes_h)).flex_none()
            } else {
                body.flex_1()
            });
        }

        if changes_open && explorer_open {
            col = col.child(
                div()
                    .id("section-split")
                    .h(px(SECTION_SPLIT_H))
                    .w_full()
                    .flex_none()
                    .cursor_row_resize()
                    .on_drag(ResizeHandle::SectionSplit, |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.new(|_| DragPreview)
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            );
        }

        col = col.child(self.section_header(
            cx,
            "Explorer",
            None,
            self.explorer_collapsed,
            Section::Explorer,
        ));

        if explorer_open {
            col = col.child(
                div()
                    .flex_1()
                    .relative()
                    .overflow_hidden()
                    .child(tree_view(e.clone())),
            );
        }

        col.child(deferred(
            div()
                .id("sidebar-resize")
                .occlude()
                .absolute()
                .top_0()
                .right(px(-RESIZE_HANDLE_W / 2.0))
                .w(px(RESIZE_HANDLE_W))
                .h_full()
                .cursor_col_resize()
                .on_drag(ResizeHandle::Sidebar, |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| DragPreview)
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
        ))
    }

    fn diff_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut title = self
            .open
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "no file selected".to_string());
        if self.rows.len() > MAX_ROWS {
            title = format!("{title}  (showing first {MAX_ROWS} rows)");
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .child(
                div()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .text_color(rgb(DIM))
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .id("pm-diff")
                    .flex_1()
                    .relative()
                    .overflow_hidden()
                    .track_focus(&self.diff_focus)
                    .on_key_down(cx.listener(Self::on_diff_key))
                    .child(diff_view(cx.entity())),
            )
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .expect("could not determine a folder to open");

    let repo = match Repo::discover(&path) {
        Ok(repo) => repo,
        Err(err) => {
            eprintln!("pm: {} is not inside a git repository ({err})", path.display());
            std::process::exit(1);
        }
    };
    eprintln!("pm: opened {}", repo.root().display());

    let root = repo.root().display().to_string();
    let title = format!(
        "{} - pm v{}",
        root.trim_end_matches(['/', '\\']),
        env!("CARGO_PKG_VERSION")
    );
    let icon = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()
        .map(|img| std::sync::Arc::new(img.into_rgba8()));

    application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                icon: icon.clone(),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title(&title);
                cx.new(|cx| {
                    let mut pm = Pm::new(repo, cx);
                    pm.start_watch(cx);
                    pm
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
