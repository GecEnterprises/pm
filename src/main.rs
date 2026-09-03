//! pm (Plus Minus) - a diff-oriented code viewer.
//!
//! Opens a folder (arg 1, or the current directory), finds the enclosing git
//! repository, and shows changed files with a side-by-side line diff.

mod diff;
mod git;

use std::path::PathBuf;

use diff::{side_by_side, DiffRow, RowKind};
use git::Repo;
use gpui::{
    div, prelude::*, px, rgb, size, App, Bounds, Context, MouseButton, Rgba, SharedString, Window,
    WindowBounds, WindowOptions,
};
use gpui_platform::application;

const BG: u32 = 0x1e1e1e;
const PANEL: u32 = 0x252526;
const BORDER: u32 = 0x333333;
const TEXT: u32 = 0xd4d4d4;
const DIM: u32 = 0x808080;
const SELECT: u32 = 0x094771;
const ADD_BG: u32 = 0x18321f;
const DEL_BG: u32 = 0x3a1d1d;

struct Pm {
    repo: Repo,
    files: Vec<PathBuf>,
    selected: Option<usize>,
    rows: Vec<DiffRow>,
}

impl Pm {
    fn new(repo: Repo) -> Self {
        let files = repo.changed_files().unwrap_or_default();
        let mut pm = Self {
            repo,
            files,
            selected: None,
            rows: Vec::new(),
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
        self.rows = side_by_side(&old, &new);
    }

    fn refresh(&mut self) {
        self.files = self.repo.changed_files().unwrap_or_default();
        match self.selected {
            Some(i) if i < self.files.len() => self.select(i),
            _ => {
                self.selected = None;
                self.rows.clear();
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
            .child(self.diff_pane())
    }
}

impl Pm {
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let header = div()
            .flex()
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

        let rows = self.files.iter().enumerate().map(|(i, path)| {
            let selected = self.selected == Some(i);
            div()
                .id(("file", i))
                .flex()
                .px_3()
                .py_1()
                .cursor_pointer()
                .when(selected, |s| s.bg(rgb(SELECT)))
                .hover(|s| s.bg(rgb(BORDER)))
                .child(SharedString::from(path.to_string_lossy().into_owned()))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pm, _, _, cx| {
                        pm.select(i);
                        cx.notify();
                    }),
                )
        });

        div()
            .flex()
            .flex_col()
            .w(px(280.))
            .h_full()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(header)
            .child(div().flex().flex_col().overflow_y_scroll().children(rows))
    }

    fn diff_pane(&self) -> impl IntoElement {
        let title = self
            .selected
            .and_then(|i| self.files.get(i))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "no file selected".to_string());

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .text_color(rgb(DIM))
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_scroll()
                    .font_family("Consolas")
                    .text_size(px(12.5))
                    .children(self.rows.iter().map(row_view)),
            )
    }
}

fn row_view(row: &DiffRow) -> impl IntoElement {
    let (left_bg, right_bg) = match row.kind {
        RowKind::Equal => (rgb(BG), rgb(BG)),
        RowKind::Add => (rgb(BG), rgb(ADD_BG)),
        RowKind::Remove => (rgb(DEL_BG), rgb(BG)),
        RowKind::Modify => (rgb(DEL_BG), rgb(ADD_BG)),
    };

    div()
        .flex()
        .w_full()
        .child(cell(row.left_no, row.left.as_deref(), left_bg))
        .child(div().w(px(1.)).bg(rgb(BORDER)))
        .child(cell(row.right_no, row.right.as_deref(), right_bg))
}

fn cell(number: Option<usize>, text: Option<&str>, bg: Rgba) -> impl IntoElement {
    let gutter = number
        .map(|n| SharedString::from(n.to_string()))
        .unwrap_or_default();
    let content = SharedString::from(text.unwrap_or("").to_string());

    div()
        .flex()
        .flex_1()
        .min_w_0()
        .bg(bg)
        .child(
            div()
                .w(px(48.))
                .flex_none()
                .px_2()
                .text_color(rgb(DIM))
                .child(gutter),
        )
        .child(div().flex_1().px_2().whitespace_nowrap().child(content))
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
