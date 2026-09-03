//! pm (Plus Minus) - a diff-oriented code viewer.
//!
//! Opens a folder (arg 1, or the current directory), finds the enclosing git
//! repository, and shows changed files with a side-by-side line diff.
//!
//! The diff body and the file list are each a single custom gpui [`Element`]
//! (see [`diff_view`] / [`list_view`]) that owns a pixel scroll offset and paints
//! only the visible rows — an editor-style scroll surface rather than a
//! virtualized list of flex rows.

mod diff;
mod diff_view;
mod git;
mod highlight;
mod list_view;
mod scroll;

use std::path::PathBuf;

use diff::{side_by_side, DiffRow};
use diff_view::{diff_view, ShapeCache};
use git::Repo;
use gpui::{
    div, prelude::*, px, rgb, size, App, Bounds, Context, MouseButton, SharedString, Window,
    WindowBounds, WindowOptions,
};
use gpui_platform::application;
use highlight::{Highlighter, Line};
use list_view::list_view;
use scroll::{ScrollDrag, ScrollState};

pub(crate) const BG: u32 = 0x1e1e1e;
pub(crate) const PANEL: u32 = 0x252526;
pub(crate) const BORDER: u32 = 0x333333;
pub(crate) const TEXT: u32 = 0xd4d4d4;
pub(crate) const DIM: u32 = 0x808080;
pub(crate) const SELECT: u32 = 0x094771;
pub(crate) const ADD_BG: u32 = 0x18321f;
pub(crate) const DEL_BG: u32 = 0x3a1d1d;

/// Diff row height / line-height, in px.
pub(crate) const ROW_H: f32 = 18.0;
/// Sidebar row height, in px.
pub(crate) const LIST_ROW_H: f32 = 24.0;
/// Scrollbar track thickness, in px.
pub(crate) const BAR: f32 = 12.0;
/// Line-number column width, in px.
pub(crate) const GUTTER_W: f32 = 52.0;
/// Line-number column right padding, in px.
pub(crate) const GUTTER_PAD: f32 = 8.0;
/// Text left padding inside a column, in px.
pub(crate) const TEXT_PAD_L: f32 = 8.0;
/// Centre divider width, in px.
pub(crate) const DIVIDER_W: f32 = 1.0;
/// Monospace font for diff text.
pub(crate) const BODY_FONT: &str = "Consolas";
pub(crate) const BODY_FONT_SIZE: f32 = 12.5;
/// Hard cap on the number of diff rows laid out.
pub(crate) const MAX_ROWS: usize = 200_000;

/// Scroll state for the diff body: one shared vertical offset, one horizontal
/// offset per column.
#[derive(Default)]
pub(crate) struct DiffScroll {
    pub y: ScrollState,
    pub x: [ScrollState; 2],
    pub drag: Option<ScrollDrag>,
}

pub(crate) struct Pm {
    repo: Repo,
    hl: Highlighter,
    files: Vec<PathBuf>,
    selected: Option<usize>,
    rows: Vec<DiffRow>,
    /// Highlighted lines of the HEAD and working-tree versions of the current
    /// file, indexed by `DiffRow::left_no` / `right_no` (1-based).
    old_lines: Vec<Line>,
    new_lines: Vec<Line>,
    /// Shaped diff lines for the current file (cleared on file switch).
    shaped: ShapeCache,
    diff: DiffScroll,
    list_scroll: ScrollState,
    list_drag: Option<ScrollDrag>,
    hover_file: Option<usize>,
}

impl Pm {
    fn new(repo: Repo) -> Self {
        let files = repo.changed_files().unwrap_or_default();
        let mut pm = Self {
            repo,
            hl: Highlighter::new(),
            files,
            selected: None,
            rows: Vec::new(),
            old_lines: Vec::new(),
            new_lines: Vec::new(),
            shaped: ShapeCache::default(),
            diff: DiffScroll::default(),
            list_scroll: ScrollState::default(),
            list_drag: None,
            hover_file: None,
        };
        if !pm.files.is_empty() {
            pm.select(0);
        }
        pm
    }

    fn select(&mut self, index: usize) {
        let Some(rel) = self.files.get(index).cloned() else {
            return;
        };
        self.selected = Some(index);
        let old = self.repo.head_content(&rel);
        let new = self.repo.working_content(&rel);
        self.old_lines = self.hl.highlight(&rel, &old);
        self.new_lines = self.hl.highlight(&rel, &new);
        self.rows = side_by_side(&old, &new);
        self.shaped.clear();
        self.diff = DiffScroll::default();
    }

    fn refresh(&mut self) {
        self.files = self.repo.changed_files().unwrap_or_default();
        self.hover_file = None;
        match self.selected {
            Some(i) if i < self.files.len() => self.select(i),
            _ => {
                self.selected = None;
                self.rows.clear();
                self.old_lines.clear();
                self.new_lines.clear();
                self.shaped.clear();
                self.diff = DiffScroll::default();
            }
        }
    }
}

impl Render for Pm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family("Segoe UI")
            .text_size(px(13.))
            .child(self.sidebar(cx))
            .child(self.diff_pane(cx))
    }
}

impl Pm {
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let header = div()
            .flex()
            .flex_none()
            .justify_between()
            .items_center()
            .px_3()
            .py_2()
            .text_color(rgb(DIM))
            .child(SharedString::from(format!("{} changed", self.files.len())))
            .child(
                div()
                    .id("refresh")
                    .cursor_pointer()
                    .px_2()
                    .rounded_sm()
                    .hover(|s| s.bg(rgb(BORDER)))
                    .child("⟳")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.refresh();
                            cx.notify();
                        }),
                    ),
            );

        div()
            .flex()
            .flex_col()
            .w(px(280.))
            .h_full()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .relative()
                    .overflow_hidden()
                    .child(list_view(cx.entity())),
            )
    }

    fn diff_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut title = self
            .selected
            .and_then(|i| self.files.get(i))
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

    application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Pm::new(repo)),
        )
        .unwrap();
        cx.activate(true);
    });
}
