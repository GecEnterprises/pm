//! The `Pm` view: a gpui entity that owns an [`AppState`] plus all render and
//! interaction state (scroll offsets, drags, hovers, the shaped-line cache).

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    canvas, deferred, div, prelude::*, px, rgb, Bounds, ClipboardItem, Context, DragMoveEvent,
    Empty, FocusHandle, KeyDownEvent, MouseButton, SharedString, Window,
};

use pm_core::text::{BufferPos, DiffCursor};
use pm_core::watch::Sentinel;
use pm_core::{AppState, Repo};

use crate::diff_view::{diff_view, ShapeCache};
use crate::list_view::list_view;
use crate::scroll::{ScrollDrag, ScrollState};
use crate::theme::*;
use crate::tree_view::tree_view;

/// Scroll state for the diff body: one shared vertical offset, one horizontal
/// offset per column.
#[derive(Default)]
pub struct DiffScroll {
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

pub struct Pm {
    pub state: AppState,
    /// Shaped diff lines for the open file (cleared on file switch).
    pub shaped: ShapeCache,
    pub diff: DiffScroll,

    pub list_scroll: ScrollState,
    pub list_drag: Option<ScrollDrag>,
    pub hover_file: Option<usize>,

    // layout prefs — persist across file switches, re-clamped on window resize
    pub sidebar_w: f32,
    pub changes_h: f32,
    pub diff_split: f32,
    pub changes_collapsed: bool,
    pub explorer_collapsed: bool,

    pub root_bounds: Bounds<gpui::Pixels>,
    pub diff_split_drag: Option<f32>,

    pub tree_scroll: ScrollState,
    pub tree_drag: Option<ScrollDrag>,
    pub tree_hover: Option<usize>,

    /// Filesystem watcher; polled by a foreground task (see `start_watch`).
    pub sentinel: Option<Sentinel>,

    pub diff_focus: FocusHandle,
    pub text_drag: bool,
    /// Middle-click autoscroll origin, in window space.
    pub autoscroll: Option<(f32, f32)>,
    pub mouse_pos: (f32, f32),
    /// Diff viewport height in px, captured each prepaint (page-scroll + follow).
    pub diff_viewport_h: f32,}

impl Pm {
    pub fn new(repo: Repo, cx: &mut Context<Self>) -> Self {
        let sentinel = Sentinel::start(repo.root().to_path_buf())
            .map_err(|e| eprintln!("pm: filesystem watch unavailable ({e})"))
            .ok();
        Self {
            state: AppState::new(repo),
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
            tree_scroll: ScrollState::default(),
            tree_drag: None,
            tree_hover: None,
            sentinel,
            diff_focus: cx.focus_handle(),
            text_drag: false,
            autoscroll: None,
            mouse_pos: (0.0, 0.0),
            diff_viewport_h: 0.0,
        }
    }

    /// Load `rel` into the diff and reset the view (scroll, shaped cache, drags).
    pub fn open_path(&mut self, rel: PathBuf) {
        self.state.open_path(rel);
        self.shaped.clear();
        self.diff.y = ScrollState::default();
        self.diff.x = [ScrollState::default(); 2];
        self.diff.drag = None;
        self.text_drag = false;
        self.autoscroll = None;
    }

    pub fn refresh(&mut self) {
        self.state.refresh();
        self.shaped.clear();
        self.diff = DiffScroll::default();
        self.hover_file = None;
        self.tree_hover = None;
    }

    fn reload_open_keep_scroll(&mut self, rel: PathBuf) {
        let (y, x0, x1) = (
            self.diff.y.offset,
            self.diff.x[0].offset,
            self.diff.x[1].offset,
        );
        let caret = self.state.caret;
        self.open_path(rel);
        self.diff.y.offset = y;
        self.diff.x[0].offset = x0;
        self.diff.x[1].offset = x1;
        self.state.caret = caret;
    }

    /// Spawn the foreground loop that drains the Sentinel and applies changes.
    pub fn start_watch(&mut self, cx: &mut Context<Self>) {
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

    fn on_fs_change(&mut self, changed: &[PathBuf], cx: &mut Context<Self>) {
        let reload = self.state.apply_fs_change(changed);
        self.hover_file = None;
        self.tree_hover = None;
        if let Some(rel) = reload {
            self.reload_open_keep_scroll(rel);
        }
        cx.notify();
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

    /// Byte offset nearest `x_local` px within `(col, file_row)`'s shaped line.
    fn byte_at_x(&self, col: usize, file_row: usize, x_local: f32) -> usize {
        match self.state.disp_row(col, file_row) {
            Some(d) => self.shaped.byte_at_x(col, d, x_local),
            None => 0,
        }
    }

    /// Place / extend the caret from a click in column `col` at display `disp_row`,
    /// `x_local` px into the shaped line.
    pub fn click_text(
        &mut self,
        col: usize,
        disp_row: usize,
        x_local: f32,
        shift: bool,
        clicks: usize,
        cx: &mut Context<Self>,
    ) {
        let row = self.state.snap_file_row(col, disp_row);
        let byte = self.byte_at_x(col, row, x_local);
        let at = BufferPos { file_row: row, byte };
        if clicks >= 3 {
            self.state.caret = Some(DiffCursor {
                col,
                anchor: BufferPos { file_row: row, byte: 0 },
                head: BufferPos { file_row: row, byte: self.state.line_len(col, row) },
                goal_x: None,
            });
            self.text_drag = false;
        } else if clicks == 2 {
            let (s, e) = self.state.word_bounds(col, row, byte);
            self.state.caret = Some(DiffCursor {
                col,
                anchor: BufferPos { file_row: row, byte: s },
                head: BufferPos { file_row: row, byte: e },
                goal_x: None,
            });
            self.text_drag = false;
        } else {
            let anchor = if shift {
                self.state.caret.filter(|c| c.col == col).map_or(at, |c| c.anchor)
            } else {
                at
            };
            self.state.caret = Some(DiffCursor { col, anchor, head: at, goal_x: None });
            self.text_drag = true;
        }
        cx.notify();
    }

    /// Extend the selection head to a drag position (same column as the caret).
    pub fn drag_text(&mut self, disp_row: usize, x_local: f32, cx: &mut Context<Self>) {
        let Some(mut cur) = self.state.caret else { return };
        let row = self.state.snap_file_row(cur.col, disp_row);
        cur.head = BufferPos { file_row: row, byte: self.byte_at_x(cur.col, row, x_local) };
        cur.goal_x = None;
        self.state.caret = Some(cur);
        self.scroll_caret_into_view();
        cx.notify();
    }


    fn scroll_caret_into_view(&mut self) {
        let Some(cur) = self.state.caret else { return };
        let vh = self.diff_viewport_h;
        if vh <= 0.0 {
            return;
        }
        let Some(disp) = self.state.disp_row(cur.col, cur.head.file_row) else { return };
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

        let default_col = if self.state.text.line_count(1) > 0 { 1 } else { 0 };
        let mut cur = self.state.caret.unwrap_or(DiffCursor {
            col: default_col,
            anchor: BufferPos { file_row: 0, byte: 0 },
            head: BufferPos { file_row: 0, byte: 0 },
            goal_x: None,
        });
        let lines = self.state.text.line_count(cur.col);
        if lines == 0 {
            return;
        }
        let last = lines - 1;

        match key {
            "c" if ctrl => {
                if let Some(text) = self.state.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                cx.stop_propagation();
                return;
            }
            "a" if ctrl => {
                cur.anchor = BufferPos { file_row: 0, byte: 0 };
                cur.head = BufferPos { file_row: last, byte: self.state.line_len(cur.col, last) };
                cur.goal_x = None;
                self.state.caret = Some(cur);
                cx.notify();
                cx.stop_propagation();
                return;
            }
            "escape" => {
                cur.anchor = cur.head;
                self.state.caret = Some(cur);
                cx.notify();
                return;
            }
            "left" => {
                cur.head = self.state.pos_left(cur.col, cur.head);
                cur.goal_x = None;
            }
            "right" => {
                cur.head = self.state.pos_right(cur.col, cur.head);
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
                    BufferPos { file_row: last, byte: self.state.line_len(cur.col, last) }
                } else {
                    BufferPos {
                        file_row: cur.head.file_row,
                        byte: self.state.line_len(cur.col, cur.head.file_row),
                    }
                };
                cur.goal_x = None;
            }
            "up" | "down" | "pageup" | "pagedown" => {
                let g = match cur.goal_x {
                    Some(g) => g,
                    None => {
                        let d = self.state.disp_row(cur.col, cur.head.file_row);
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

        cur.head = self.state.text.clip(cur.col, cur.head);
        if !shift {
            cur.anchor = cur.head;
        }
        self.state.caret = Some(cur);
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
                Some(self.state.changes.len()),
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
        let mut title = self.state.open
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "no file selected".to_string());
        if self.state.rows.len() > pm_core::MAX_ROWS {
            title = format!("{title}  (showing first {} rows)", pm_core::MAX_ROWS);
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
