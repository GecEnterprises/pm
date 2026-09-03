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
    canvas, deferred, div, prelude::*, px, rgb, size, App, Bounds, Context, DragMoveEvent, Empty,
    MouseButton, SharedString, Window, WindowBounds, WindowOptions,
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
}

impl Pm {
    fn new(repo: Repo) -> Self {
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
        self.open = Some(rel);
        self.shaped.clear();
        self.diff.y = ScrollState::default();
        self.diff.x = [ScrollState::default(); 2];
        self.diff.drag = None;
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
        self.open_path(rel);
        self.diff.y.offset = y;
        self.diff.x[0].offset = x0;
        self.diff.x[1].offset = x1;
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
                    .flex_1()
                    .relative()
                    .overflow_hidden()
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
                    let mut pm = Pm::new(repo);
                    pm.start_watch(cx);
                    pm
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
