//! The `Pm` view: a gpui entity that owns an [`AppState`] plus all render and
//! interaction state (scroll offsets, drags, hovers, the shaped-line cache).

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    canvas, deferred, div, prelude::*, px, rgb, rgba, Bounds, ClipboardItem, Context, DragMoveEvent,
    Empty, Entity, FocusHandle, KeyDownEvent, MouseButton, SharedString, Window,
};

use pm_core::text::{BufferPos, DiffCursor};
use pm_core::watch::Sentinel;
use pm_core::{AppState, Repo, Status};

use pm_core::state::Content;

use crate::decorations::client_side_decorations;
use crate::diff_view::{diff_view, ShapeCache};
use crate::history_view::history_view;
use crate::image_view::ImageView;
use crate::list_view::list_view;
use crate::config::ConfigStore;
use crate::menu::{
    About, Copy, Refresh, SelectAll, ToggleChanges, ToggleExplorer, ToggleHistory, ViewFiles,
    ViewSummary, ViewTickets, ZoomIn, ZoomOut, ZoomReset,
};
use crate::scroll::{ScrollDrag, ScrollState};
use crate::text_input::{TextInput, TextInputEvent};
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
pub(crate) enum ResizeHandle {
    Sidebar,
    SectionSplit,
    /// The centre divider of the image diff (text diff has its own in-element one).
    DiffSplit,
}

/// Step the zoom factor by `delta`, snapped to 10% and clamped to 50–300%.
fn zoom_step(scale: f32, delta: f32) -> f32 {
    (((scale + delta) * 10.0).round() / 10.0).clamp(0.5, 3.0)
}

/// The (invisible) drag-preview view required by `on_drag`.
#[derive(Clone)]
pub(crate) struct DragPreview;
impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// The top-level pane, chosen by the title-bar switcher.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Summary,
    Files,
    Tickets,
}

/// An in-progress authoring action in the Tickets pane.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Compose {
    NewTicket,
}

#[derive(Clone, Copy)]
enum Section {
    Changes,
    History,
    Explorer,
}
impl Section {
    fn toggle(self, pm: &mut Pm) {
        match self {
            Section::Changes => pm.changes_collapsed = !pm.changes_collapsed,
            Section::History => pm.history_collapsed = !pm.history_collapsed,
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

    pub history_scroll: ScrollState,
    pub history_drag: Option<ScrollDrag>,
    pub history_hover: Option<usize>,

    // layout prefs — persist across file switches, re-clamped on window resize
    pub sidebar_w: f32,
    pub changes_h: f32,
    pub history_h: f32,
    pub diff_split: f32,
    pub changes_collapsed: bool,
    pub history_collapsed: bool,
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
    pub diff_viewport_h: f32,
    /// Last window title we pushed, to avoid redundant `set_window_title` calls.
    title: String,

    /// Focus target for the root so menu actions / keybindings have a dispatch
    /// path even before the diff pane is focused.
    pub root_focus: FocusHandle,
    /// Which top-level title-bar menu is open (index into `menu::menu_groups`).
    pub open_menu: Option<usize>,
    /// Armed by a mouse-down on the title bar, consumed by the next move to
    /// start a window drag (Zed's `should_move`).
    pub window_drag_armed: bool,
    /// Whether the About overlay is showing.
    pub show_about: bool,
    /// Shared zoom / pan for the image diff viewer.
    pub image_view: ImageView,
    /// Active pan gesture in the image viewer: last pointer position.
    pub image_drag: Option<(f32, f32)>,
    /// Bounds of the image diff pane, sampled each frame (for pan clamping).
    pub image_pane: Bounds<gpui::Pixels>,

    /// Which top-level pane is showing (title-bar switcher).
    pub view: View,
    /// Selected ticket id in the Tickets pane.
    pub selected_ticket: Option<u64>,
    /// Active authoring action in the Tickets pane, if any.
    pub composing: Option<Compose>,
    pub new_ticket_title: Entity<TextInput>,
    pub new_ticket_body: Entity<TextInput>,
    pub comment_box: Entity<TextInput>,
    /// "as:" identity field in the Tickets pane — overrides `state.author` for
    /// the next ticket / comment (PM-15).
    pub author_box: Entity<TextInput>,
    pub ticket_hover: Option<usize>,
    /// Whole-window zoom, synced from the config each render (1.0 = 100%).
    pub scale: f32,
    /// Statuses shown in the ticket list (the header filter dropdown).
    pub ticket_filter: std::collections::HashSet<Status>,
    /// The list-header status-filter popover is open.
    pub filter_menu_open: bool,
    /// The selected ticket's status-picker popover is open.
    pub status_menu_open: bool,
}

impl Pm {
    pub fn new(repo: Repo, cx: &mut Context<Self>) -> Self {
        let sentinel = Sentinel::start(repo.root().to_path_buf())
            .map_err(|e| eprintln!("pm: filesystem watch unavailable ({e})"))
            .ok();

        let new_ticket_title = cx.new(|cx| TextInput::single(cx).placeholder("Title"));
        cx.subscribe(&new_ticket_title, |pm, _, ev, cx| match ev {
            TextInputEvent::Submit => pm.submit_new_ticket(cx),
        })
        .detach();
        let new_ticket_body =
            cx.new(|cx| TextInput::multi(cx).placeholder("Description (optional)"));
        let comment_box = cx.new(|cx| TextInput::multi(cx).placeholder("Add a comment\u{2026}"));

        let scale = ConfigStore::get(cx).ui_scale();

        let state = AppState::new(repo);
        let author_box =
            cx.new(|cx| TextInput::single(cx).placeholder("author").text(state.author.clone(), cx));

        Self {
            state,
            shaped: ShapeCache::default(),
            diff: DiffScroll::default(),
            list_scroll: ScrollState::default(),
            list_drag: None,
            hover_file: None,
            history_scroll: ScrollState::default(),
            history_drag: None,
            history_hover: None,
            sidebar_w: 280.0 * scale,
            changes_h: 220.0 * scale,
            history_h: 200.0 * scale,
            diff_split: 0.5,
            changes_collapsed: false,
            history_collapsed: true,
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
            title: String::new(),
            root_focus: cx.focus_handle(),
            open_menu: None,
            window_drag_armed: false,
            show_about: false,
            image_view: ImageView::default(),
            image_drag: None,
            image_pane: Bounds::default(),
            view: View::Files,
            selected_ticket: None,
            composing: None,
            new_ticket_title,
            new_ticket_body,
            comment_box,
            author_box,
            ticket_hover: None,
            scale,
            // Active work by default; closed tickets hidden until asked for.
            ticket_filter: [Status::Open, Status::InProgress, Status::Blocked]
                .into_iter()
                .collect(),
            filter_menu_open: false,
            status_menu_open: false,
        }
    }

    /// VS Code-style window title: `<context> — <repo> — pm`. The leading
    /// segment tracks the active view — the open file (File-to-File), or the
    /// ticket being viewed / created (Tickets) — and is dropped when there's
    /// nothing to name.
    pub(crate) fn window_title(&self) -> String {
        let app = "pm";
        let repo = self
            .state
            .repo
            .root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "pm".to_string());

        let lead: Option<String> = match self.view {
            View::Summary => Some("Summary".to_string()),
            View::Files => self
                .state
                .open
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|f| f.to_string_lossy().into_owned()),
            View::Tickets => Some(self.ticket_title_segment()),
        };

        match lead {
            Some(lead) => format!("{lead} \u{2014} {repo} \u{2014} {app}"),
            None => format!("{repo} \u{2014} {app}"),
        }
    }

    /// The title-bar segment for the Tickets view.
    fn ticket_title_segment(&self) -> String {
        if self.composing == Some(Compose::NewTicket) {
            return "New ticket".to_string();
        }
        match self.selected_ticket.and_then(|id| self.state.pm.ticket(id)) {
            Some(t) => {
                let title: String = t.title.chars().take(50).collect();
                format!("{} {title}", self.state.pm.display_id(t)).trim().to_string()
            }
            None => "Tickets".to_string(),
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
        self.image_view = ImageView::default();
        self.image_drag = None;
    }

    pub fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.view != view {
            self.view = view;
            cx.notify();
        }
    }

    /// Pull the zoom factor from the config, rescaling the layout state that's
    /// held in real pixels (`sidebar_w`, `changes_h`, `history_h`). Everything
    /// else scales through `window.set_rem_size`. Called each render.
    fn sync_scale(&mut self, cx: &mut Context<Self>) {
        let scale = ConfigStore::get(cx).ui_scale();
        if (scale - self.scale).abs() > 0.001 {
            let f = scale / self.scale;
            self.sidebar_w *= f;
            self.changes_h *= f;
            self.history_h *= f;
            self.scale = scale;
            self.shaped.clear(); // diff text re-shapes at the new font size
        }
        // Adopt an identity another window (or an external edit) saved (PM-15).
        let cfg_author = ConfigStore::get(cx).author;
        if !cfg_author.trim().is_empty() && cfg_author != self.state.author {
            self.state.author = cfg_author.clone();
            self.author_box.update(cx, |ti, cx| ti.set_text(cfg_author, cx));
        }
    }

    /// Current zoom, as a whole percent (for the status bar).
    pub fn zoom_pct(&self) -> u32 {
        (self.scale * 100.0).round() as u32
    }

    /// Diff row height at the current zoom (the custom diff element scales its
    /// own metrics; this is for the scroll math that lives here).
    fn diff_row_h(&self) -> f32 {
        ROW_H * self.scale
    }

    pub fn refresh(&mut self) {
        self.state.refresh();
        self.shaped.clear();
        self.diff = DiffScroll::default();
        self.hover_file = None;
        self.tree_hover = None;
        self.history_hover = None;
    }

    /// Point the diff at a history row: row 0 is the working tree, rows 1.. are
    /// `state.commits[row - 1]` (compared against their first parent).
    pub fn select_commit(&mut self, row: usize) {
        let target = match row.checked_sub(1) {
            None => pm_core::DiffTarget::WorkingTree,
            Some(i) => match self.state.commits.get(i) {
                Some(c) => pm_core::DiffTarget::Commit(c.id),
                None => return,
            },
        };
        if target == self.state.target {
            return;
        }
        self.state.set_target(target);
        self.shaped.clear();
        self.diff = DiffScroll::default();
        self.hover_file = None;
        self.image_view = ImageView::default();
        self.image_drag = None;
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
        let sc = self.scale;
        let max_sb = (rw - SIDEBAR_MAX_MARGIN * sc).max(SIDEBAR_MIN * sc);
        self.sidebar_w = self.sidebar_w.clamp(SIDEBAR_MIN * sc, max_sb);
        // Room the three section headers + the one split handle always take.
        let avail = (rh - (3.0 * SECTION_HEADER_H + SECTION_SPLIT_H) * sc).max(0.0);
        self.history_h = self.history_h.clamp(0.0, avail);
        let history_used = if self.history_collapsed { 0.0 } else { self.history_h };
        self.changes_h = self.changes_h.clamp(0.0, (avail - history_used).max(0.0));
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
        let rh = self.diff_row_h();
        let caret_top = disp as f32 * rh;
        let off = f32::from(self.diff.y.offset.y);
        let new = if caret_top < off {
            caret_top
        } else if caret_top + rh > off + vh {
            caret_top + rh - vh
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
                self.copy_selection(cx);
                cx.stop_propagation();
                return;
            }
            "a" if ctrl => {
                self.select_all(cx);
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
                let page = ((self.diff_viewport_h / self.diff_row_h()).floor() as isize - 1).max(1);
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

    /// Copy the current diff selection to the clipboard.
    pub(crate) fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.state.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Select the whole of the side the caret is on (or the working side).
    pub(crate) fn select_all(&mut self, cx: &mut Context<Self>) {
        let col = self
            .state
            .caret
            .map(|c| c.col)
            .unwrap_or(if self.state.text.line_count(1) > 0 { 1 } else { 0 });
        let lines = self.state.text.line_count(col);
        if lines == 0 {
            return;
        }
        let last = lines - 1;
        self.state.caret = Some(DiffCursor {
            col,
            anchor: BufferPos { file_row: 0, byte: 0 },
            head: BufferPos { file_row: last, byte: self.state.line_len(col, last) },
            goal_x: None,
        });
        cx.notify();
    }

}

impl Render for Pm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One-shot: surfaces a native dialog if the config store started
        // read-only or failed to load. `take_alert` self-clears.
        crate::config::present_config_alert(window, cx);
        self.sync_scale(cx);
        window.set_rem_size(px(BASE_REM * self.scale));

        let title = self.window_title();
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }

        let body = match self.view {
            View::Files => self.files_body(cx).into_any_element(),
            View::Summary => self.summary_body().into_any_element(),
            View::Tickets => self.tickets_body(cx).into_any_element(),
        };

        let root = div()
            .id("pm-root")
            .track_focus(&self.root_focus)
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family(UI_FONT)
            .text_size(rm(13.))
            .on_action(cx.listener(|pm, _: &Refresh, _, cx| {
                pm.refresh();
                cx.notify();
            }))
            .on_action(cx.listener(|pm, _: &About, _, cx| {
                pm.show_about = true;
                cx.notify();
            }))
            .on_action(cx.listener(|pm, _: &ToggleChanges, _, cx| {
                pm.changes_collapsed = !pm.changes_collapsed;
                cx.notify();
            }))
            .on_action(cx.listener(|pm, _: &ToggleExplorer, _, cx| {
                pm.explorer_collapsed = !pm.explorer_collapsed;
                cx.notify();
            }))
            .on_action(cx.listener(|pm, _: &ToggleHistory, _, cx| {
                pm.history_collapsed = !pm.history_collapsed;
                cx.notify();
            }))
            .on_action(cx.listener(|pm, _: &Copy, _, cx| pm.copy_selection(cx)))
            .on_action(cx.listener(|pm, _: &SelectAll, _, cx| pm.select_all(cx)))
            .on_action(cx.listener(|pm, _: &ViewSummary, _, cx| pm.set_view(View::Summary, cx)))
            .on_action(cx.listener(|pm, _: &ViewFiles, _, cx| pm.set_view(View::Files, cx)))
            .on_action(cx.listener(|pm, _: &ViewTickets, _, cx| pm.set_view(View::Tickets, cx)))
            .on_action(cx.listener(|_, _: &ZoomIn, _, cx| {
                ConfigStore::update(cx, |c| c.ui_scale = zoom_step(c.ui_scale, 0.1));
            }))
            .on_action(cx.listener(|_, _: &ZoomOut, _, cx| {
                ConfigStore::update(cx, |c| c.ui_scale = zoom_step(c.ui_scale, -0.1));
            }))
            .on_action(cx.listener(|_, _: &ZoomReset, _, cx| {
                ConfigStore::update(cx, |c| c.ui_scale = 1.0);
            }))
            .on_key_down(cx.listener(|pm, e: &KeyDownEvent, _, cx| {
                if pm.open_menu.is_some() && e.keystroke.key == "escape" {
                    pm.open_menu = None;
                    cx.notify();
                }
            }))
            .when(self.open_menu.is_some(), |r| {
                r.child(deferred(
                    div().absolute().inset_0().on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.open_menu = None;
                            cx.notify();
                            // Stop the click reaching the menu button underneath,
                            // whose toggle would otherwise reopen the menu.
                            cx.stop_propagation();
                        }),
                    ),
                ))
            })
            .child(self.title_bar(window, cx))
            .child(body)
            .when(self.show_about, |r| r.child(self.about_overlay(cx)))
            .child(self.status_bar(window, cx));

        client_side_decorations(root, window, cx)
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
            .h(rm(SECTION_HEADER_H))
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
        let history_open = !self.history_collapsed;
        let explorer_open = !self.explorer_collapsed;

        // The last open section fills the remaining space; earlier ones are fixed.
        let last = if explorer_open {
            2
        } else if history_open {
            1
        } else {
            0
        };

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
            col = col.child(if last == 0 {
                body.flex_1()
            } else {
                body.h(px(self.changes_h)).flex_none()
            });
        }

        // One resize handle, under Changes, whenever a section follows it.
        if changes_open && (history_open || explorer_open) {
            col = col.child(
                div()
                    .id("section-split")
                    .h(rm(SECTION_SPLIT_H))
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
            "Commit History",
            Some(self.state.commits.len()),
            self.history_collapsed,
            Section::History,
        ));

        if history_open {
            let body = div()
                .relative()
                .overflow_hidden()
                .child(history_view(e.clone()));
            col = col.child(if last == 1 {
                body.flex_1()
            } else {
                body.h(px(self.history_h)).flex_none()
            });
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

        col.child(self.sidebar_resize_handle())
    }

    /// The drag strip on the right edge of the sidebar. Shared by File-to-File
    /// (`left_column`) and Tickets so both panes resize the same `sidebar_w`.
    /// The parent needs `.relative()` and an `on_drag_move` handling
    /// `ResizeHandle::Sidebar`.
    pub(crate) fn sidebar_resize_handle(&self) -> impl IntoElement {
        deferred(
            div()
                .id("sidebar-resize")
                .occlude()
                .absolute()
                .top_0()
                .right(rm(-RESIZE_HANDLE_W / 2.0))
                .w(rm(RESIZE_HANDLE_W))
                .h_full()
                .cursor_col_resize()
                .on_drag(ResizeHandle::Sidebar, |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| DragPreview)
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
        )
    }

    /// The invisible `root_bounds` canvas a resizable-sidebar pane needs (feeds
    /// the drag math + `clamp_layout`). Pair with `Self::route_sidebar_drag` in
    /// the pane's `on_drag_move`.
    pub(crate) fn root_bounds_canvas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        canvas(
            move |b, _w, cx| {
                entity.update(cx, |pm, _| {
                    pm.root_bounds = b;
                    pm.clamp_layout(b);
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    /// Route a `Sidebar` drag to `sidebar_w`. (Other `ResizeHandle`s are ignored;
    /// `files_body` handles all three itself.)
    pub(crate) fn route_sidebar_drag(
        pm: &mut Pm,
        ev: &DragMoveEvent<ResizeHandle>,
        cx: &mut Context<Pm>,
    ) {
        if let ResizeHandle::Sidebar = ev.drag(cx) {
            let root = pm.root_bounds;
            pm.sidebar_w = f32::from(ev.event.position.x) - f32::from(root.left());
            pm.clamp_layout(root);
            cx.notify();
        }
    }

    /// The File-to-File view: sidebar + diff. The `root_bounds` canvas lives here
    /// (not on the window root) so the resize-handle math and `clamp_layout` see
    /// the area between the title and status bars. Only mounted for `View::Files`.
    fn files_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        div()
            .id("pm-body")
            .relative()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
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
                        ResizeHandle::SectionSplit => {
                            pm.changes_h = y - SECTION_HEADER_H * pm.scale
                        }
                        ResizeHandle::DiffSplit => {
                            let pane_w = f32::from(root.size.width) - pm.sidebar_w;
                            if pane_w > 1.0 {
                                pm.diff_split = ((x - pm.sidebar_w) / pane_w).clamp(0.05, 0.95);
                            }
                        }
                    }
                    pm.clamp_layout(root);
                    cx.notify();
                },
            ))
            .child(self.left_column(cx))
            .child(self.diff_pane(cx))
    }

    /// The Summary view — a placeholder until summary diffing lands.
    fn summary_body(&self) -> impl IntoElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .bg(rgb(BG))
            .text_color(rgb(DIM))
            .child(SharedString::from("Summary"))
            .child(
                div()
                    .text_size(rm(11.0))
                    .child(SharedString::from("a whole-diff overview is coming soon")),
            )
    }

    fn diff_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The open path + row-truncation note now live in the status bar.
        // The container is the same for every viewer — it just swaps its child
        // based on the detected file kind.
        let pane = div()
            .id("pm-diff")
            .flex_1()
            .relative()
            .overflow_hidden()
            .track_focus(&self.diff_focus)
            .on_key_down(cx.listener(Self::on_diff_key));

        match self.state.content {
            Content::Text => pane.child(diff_view(cx.entity())).into_any_element(),
            Content::Image { .. } => pane.child(self.image_diff_pane(cx)).into_any_element(),
            Content::Binary => pane.child(self.unviewable_pane()).into_any_element(),
        }
    }

    fn unviewable_pane(&self) -> impl IntoElement {
        let name = self
            .state
            .open
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .bg(rgb(BG))
            .text_color(rgb(DIM))
            .child(SharedString::from("Binary file — not shown"))
            .child(
                div()
                    .text_size(rm(11.0))
                    .child(SharedString::from(name)),
            )
    }

    fn about_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        deferred(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x0000_00aa))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|pm, _, _, cx| {
                        pm.show_about = false;
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .occlude()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .w(px(360.0))
                        .p_4()
                        .bg(rgb(PANEL))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .rounded_lg()
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .text_size(rm(16.0))
                                .text_color(rgb(TEXT))
                                .child("pm \u{2014} Plus Minus"),
                        )
                        .child(
                            div().text_color(rgb(DIM)).child(SharedString::from(format!(
                                "version {}",
                                env!("CARGO_PKG_VERSION")
                            ))),
                        )
                        .child(div().text_color(rgb(DIM)).child(SharedString::from(
                            self.state.repo.root().display().to_string(),
                        ))),
                ),
        )
    }
}
