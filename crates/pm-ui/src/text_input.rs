//! A real editable text field, modelled on gpui's `examples/input.rs` (which is
//! how Zed builds its inputs).
//!
//! The line is shaped once per frame with `shape_line` and painted as a single
//! `ShapedLine`, so glyphs never shift as the selection moves. Text arrives
//! through the platform IME pipeline (`window.handle_input` +
//! [`EntityInputHandler`]), so dead keys / compose / AltGr all work. Editing
//! commands are actions (see [`crate::text_input`] `actions!`), bound in
//! `src/main.rs` under the `TextInput` key context.
//!
//! Not handled: soft wrap (long lines scroll horizontally on a single-line
//! field, clip on a multi-line one), undo/redo.

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgb, rgba, size, App, Bounds, ClipboardItem,
    CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    Focusable, GlobalElementId, InspectorElementId, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString, Style,
    TextRun, UTF16Selection, UnderlineStyle, Window,
};

use crate::theme::{BG, BORDER, DIM, SELECT, TEXT};

actions!(
    pm_text_input,
    [
        Backspace,
        Delete,
        DeleteWordLeft,
        Left,
        Right,
        Up,
        Down,
        WordLeft,
        WordRight,
        Home,
        End,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectHome,
        SelectEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        /// Enter on a single-line field.
        Confirm,
        /// Enter on a multi-line field.
        Newline,
    ]
);

/// Emitted by a single-line field when the user presses Enter.
pub enum TextInputEvent {
    Submit,
}

pub struct TextInput {
    content: String,
    placeholder: SharedString,
    /// Byte offsets into `content`; empty range == plain caret.
    selected_range: Range<usize>,
    selection_reversed: bool,
    /// IME composition range, underlined while active.
    marked_range: Option<Range<usize>>,
    pub multiline: bool,
    is_selecting: bool,
    /// Horizontal scroll (single-line only), keeps the caret in view.
    scroll_x: Pixels,
    focus_handle: FocusHandle,
    // stashed each paint, for mouse hit-testing and IME geometry
    last_lines: Vec<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    last_line_height: Pixels,
}

impl TextInput {
    pub fn single(cx: &mut App) -> Self {
        Self::build(cx, false)
    }

    pub fn multi(cx: &mut App) -> Self {
        Self::build(cx, true)
    }

    fn build(cx: &mut App, multiline: bool) -> Self {
        Self {
            content: String::new(),
            placeholder: SharedString::default(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            multiline,
            is_selecting: false,
            scroll_x: px(0.0),
            focus_handle: cx.focus_handle(),
            last_lines: Vec::new(),
            last_bounds: None,
            last_line_height: px(16.0),
        }
    }

    pub fn placeholder(mut self, s: impl Into<SharedString>) -> Self {
        self.placeholder = s.into();
        self
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }

    /// Take the trimmed content, leaving the field empty.
    pub fn take(&mut self, cx: &mut Context<Self>) -> String {
        let out = self.content.trim().to_string();
        self.set_content(String::new(), cx);
        out
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.set_content(String::new(), cx);
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus_handle, cx);
    }

    fn set_content(&mut self, s: String, cx: &mut Context<Self>) {
        self.content = s;
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.scroll_x = px(0.0);
        cx.notify();
    }

    // ---- offsets ---------------------------------------------------------

    fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn prev_boundary(&self, off: usize) -> usize {
        self.content[..off].char_indices().next_back().map(|(i, _)| i).unwrap_or(0)
    }

    fn next_boundary(&self, off: usize) -> usize {
        self.content[off..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| off + i)
            .unwrap_or(self.content.len())
    }

    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn prev_word(&self, mut p: usize) -> usize {
        let s = &self.content;
        while p > 0 && !Self::is_word(s[..p].chars().next_back().unwrap()) {
            p = self.prev_boundary(p);
        }
        while p > 0 && Self::is_word(s[..p].chars().next_back().unwrap()) {
            p = self.prev_boundary(p);
        }
        p
    }

    fn next_word(&self, mut p: usize) -> usize {
        let s = &self.content;
        let len = s.len();
        while p < len && !Self::is_word(s[p..].chars().next().unwrap()) {
            p = self.next_boundary(p);
        }
        while p < len && Self::is_word(s[p..].chars().next().unwrap()) {
            p = self.next_boundary(p);
        }
        p
    }

    fn line_start(&self, p: usize) -> usize {
        self.content[..p].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self, p: usize) -> usize {
        self.content[p..].find('\n').map(|i| p + i).unwrap_or(self.content.len())
    }

    /// `(line index, byte offset within that line)` for an absolute offset.
    fn line_col(&self, off: usize) -> (usize, usize) {
        let mut line = 0;
        let mut start = 0;
        for (i, _) in self.content[..off].match_indices('\n') {
            line += 1;
            start = i + 1;
        }
        (line, off - start)
    }

    /// Absolute offset of the start of visual line `idx`.
    fn line_offset(&self, idx: usize) -> usize {
        if idx == 0 {
            return 0;
        }
        self.content
            .match_indices('\n')
            .nth(idx - 1)
            .map(|(i, _)| i + 1)
            .unwrap_or(self.content.len())
    }

    // ---- movement / selection ------------------------------------------

    fn move_to(&mut self, off: usize, cx: &mut Context<Self>) {
        self.selected_range = off..off;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, off: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = off;
        } else {
            self.selected_range.end = off;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn vertical(&self, dir: isize) -> usize {
        let cur = self.cursor();
        let (line, col) = self.line_col(cur);
        let target = line as isize + dir;
        if target < 0 {
            return 0;
        }
        let target = target as usize;
        if target >= self.last_lines.len().max(1) {
            return self.content.len();
        }
        // Same x as the current column, resolved on the target line.
        let x = self
            .last_lines
            .get(line)
            .map(|l| l.x_for_index(col.min(l.text.len())))
            .unwrap_or(px(0.0));
        let ls = self.line_offset(target);
        let idx = self
            .last_lines
            .get(target)
            .map(|l| l.closest_index_for_x(x))
            .unwrap_or(0);
        ls + idx
    }

    fn index_for_mouse(&self, pos: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else { return 0 };
        if self.last_lines.is_empty() {
            return 0;
        }
        let rel_y = (pos.y - bounds.top()).max(px(0.0));
        let line = (f32::from(rel_y) / f32::from(self.last_line_height)) as usize;
        let line = line.min(self.last_lines.len() - 1);
        let x = pos.x - bounds.left() + self.scroll_x;
        let idx = self.last_lines[line].closest_index_for_x(x);
        self.line_offset(line) + idx
    }

    // ---- action handlers ----------------------------------------------

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let p = self.prev_boundary(self.cursor());
            self.move_to(p, cx);
        } else {
            let s = self.selected_range.start;
            self.move_to(s, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let p = self.next_boundary(self.cursor());
            self.move_to(p, cx);
        } else {
            let e = self.selected_range.end;
            self.move_to(e, cx);
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            let p = self.vertical(-1);
            self.move_to(p, cx);
        }
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            let p = self.vertical(1);
            self.move_to(p, cx);
        }
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            let p = self.vertical(-1);
            self.select_to(p, cx);
        }
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            let p = self.vertical(1);
            self.select_to(p, cx);
        }
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.prev_word(self.cursor());
        self.move_to(p, cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.next_word(self.cursor());
        self.move_to(p, cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.line_start(self.cursor());
        self.move_to(p, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.line_end(self.cursor());
        self.move_to(p, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.prev_boundary(self.cursor());
        self.select_to(p, cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.next_boundary(self.cursor());
        self.select_to(p, cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.prev_word(self.cursor());
        self.select_to(p, cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.next_word(self.cursor());
        self.select_to(p, cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.line_start(self.cursor());
        self.select_to(p, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.line_end(self.cursor());
        self.select_to(p, cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let p = self.prev_boundary(self.cursor());
            self.select_to(p, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let p = self.next_boundary(self.cursor());
            self.select_to(p, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_left(&mut self, _: &DeleteWordLeft, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let p = self.prev_word(self.cursor());
            self.select_to(p, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            let text = if self.multiline {
                text.replace('\r', "")
            } else {
                text.replace(['\n', '\r'], " ")
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Submit);
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn on_mouse_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        let idx = self.index_for_mouse(e.position);
        if e.modifiers.shift {
            self.select_to(idx, cx);
        } else {
            self.move_to(idx, cx);
        }
    }

    fn on_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let idx = self.index_for_mouse(e.position);
            self.select_to(idx, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    // ---- utf16 helpers (verbatim from examples/input.rs) --------------

    fn offset_from_utf16(&self, off: usize) -> usize {
        let mut u8o = 0;
        let mut u16c = 0;
        for ch in self.content.chars() {
            if u16c >= off {
                break;
            }
            u16c += ch.len_utf16();
            u8o += ch.len_utf8();
        }
        u8o
    }

    fn offset_to_utf16(&self, off: usize) -> usize {
        let mut u16o = 0;
        let mut u8c = 0;
        for ch in self.content.chars() {
            if u8c >= off {
                break;
            }
            u8c += ch.len_utf8();
            u16o += ch.len_utf16();
        }
        u16o
    }

    fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(r.start)..self.offset_to_utf16(r.end)
    }

    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(r.start)..self.offset_from_utf16(r.end)
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl gpui::EventEmitter<TextInputEvent> for TextInput {}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            self.content[..range.start].to_owned() + new_text + &self.content[range.end..];
        let caret = range.start + new_text.len();
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            self.content[..range.start].to_owned() + new_text + &self.content[range.end..];
        self.marked_range = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .map(|r| range.start + r.start..range.start + r.end)
            .unwrap_or_else(|| {
                let c = range.start + new_text.len();
                c..c
            });
        self.selection_reversed = false;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let (line, col) = self.line_col(range.start);
        let sl = self.last_lines.get(line)?;
        let x = bounds.left() + sl.x_for_index(col) - self.scroll_x;
        let y = bounds.top() + self.last_line_height * (line as f32);
        Some(Bounds::from_corners(
            point(x, y),
            point(x + px(1.0), y + self.last_line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let idx = self.index_for_mouse(p);
        Some(self.offset_to_utf16(idx))
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let ctx = if self.multiline {
            "TextInput TextInputMultiLine"
        } else {
            "TextInput TextInputSingleLine"
        };
        div()
            .key_context(ctx)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .w_full()
            .px_2()
            .py_1()
            .bg(rgb(BG))
            .border_1()
            .border_color(rgb(if focused { SELECT } else { BORDER }))
            .rounded_sm()
            .overflow_hidden()
            .when(self.multiline, |d| d.min_h(px(64.0)))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::newline))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(TextElement { input: cx.entity() })
    }
}

/// The painted line(s) + caret + selection.
struct TextElement {
    input: Entity<TextInput>,
}

struct TextPrepaint {
    lines: Vec<ShapedLine>,
    line_height: Pixels,
    caret: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
    scroll_x: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = TextPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let line_count = self.input.read(cx).content.split('\n').count().max(1);
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = (window.line_height() * line_count as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> TextPrepaint {
        let input = self.input.read(cx);
        let style = window.text_style();
        let line_height = window.line_height();
        let font_size = style.font_size.to_pixels(window.rem_size());

        let empty = input.content.is_empty();
        let display: String = if empty {
            input.placeholder.to_string()
        } else {
            input.content.clone()
        };
        let color = if empty { rgb(DIM).into() } else { style.color };
        let sel = input.selected_range.clone();
        let cursor = input.cursor();
        let marked = input.marked_range.clone();
        let multiline = input.multiline;

        // Shape each visual line, tracking its absolute byte start.
        let mut lines = Vec::new();
        let mut starts = Vec::new();
        let mut off = 0usize;
        for line in display.split('\n') {
            let run_len = line.len();
            let runs: Vec<TextRun> = match &marked {
                Some(m) if !empty && m.start >= off && m.end <= off + run_len => {
                    let a = m.start - off;
                    let b = m.end - off;
                    [
                        TextRun { len: a, font: style.font(), color, background_color: None, underline: None, strikethrough: None },
                        TextRun {
                            len: b - a,
                            font: style.font(),
                            color,
                            background_color: None,
                            underline: Some(UnderlineStyle { color: Some(color), thickness: px(1.0), wavy: false }),
                            strikethrough: None,
                        },
                        TextRun { len: run_len - b, font: style.font(), color, background_color: None, underline: None, strikethrough: None },
                    ]
                    .into_iter()
                    .filter(|r| r.len > 0)
                    .collect()
                }
                _ => vec![TextRun {
                    len: run_len,
                    font: style.font(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
            };
            lines.push(window.text_system().shape_line(line.to_string().into(), font_size, &runs, None));
            starts.push(off);
            off += run_len + 1;
        }

        // Horizontal scroll for a single-line field: keep the caret in view.
        let mut scroll_x = input.scroll_x;
        if !multiline {
            let caret_x = lines
                .first()
                .map(|l| l.x_for_index(cursor.min(l.text.len())))
                .unwrap_or(px(0.0));
            let vw = bounds.size.width - px(4.0);
            if caret_x - scroll_x > vw {
                scroll_x = caret_x - vw;
            }
            if caret_x - scroll_x < px(0.0) {
                scroll_x = caret_x;
            }
            let content_w = lines.first().map(|l| l.width()).unwrap_or(px(0.0));
            let max = (content_w - vw).max(px(0.0));
            scroll_x = scroll_x.clamp(px(0.0), max);
        } else {
            scroll_x = px(0.0);
        }

        // Caret + selection quads.
        let line_at = |abs: usize| -> (usize, usize) {
            let mut idx = 0;
            for (i, &s) in starts.iter().enumerate() {
                if abs >= s {
                    idx = i;
                }
            }
            (idx, abs - starts[idx])
        };
        let x0 = bounds.left() - scroll_x;

        let caret = if !empty || cursor == 0 {
            let (li, col) = line_at(cursor);
            let cx_px = lines.get(li).map(|l| l.x_for_index(col.min(l.text.len()))).unwrap_or(px(0.0));
            Some(fill(
                Bounds::new(
                    point(x0 + cx_px, bounds.top() + line_height * li as f32 + px(1.0)),
                    size(px(1.5), line_height - px(2.0)),
                ),
                rgb(TEXT),
            ))
        } else {
            None
        };

        let mut selections = Vec::new();
        if sel.start != sel.end {
            let (sl, sc) = line_at(sel.start);
            let (el, ec) = line_at(sel.end);
            for li in sl..=el {
                let Some(line) = lines.get(li) else { continue };
                let from = if li == sl { sc } else { 0 };
                let to = if li == el { ec } else { line.text.len() };
                let ax = line.x_for_index(from.min(line.text.len()));
                let bx = line.x_for_index(to.min(line.text.len()));
                let end_pad = if li != el { px(6.0) } else { px(0.0) };
                selections.push(fill(
                    Bounds::from_corners(
                        point(x0 + ax, bounds.top() + line_height * li as f32),
                        point(x0 + bx + end_pad, bounds.top() + line_height * (li as f32 + 1.0)),
                    ),
                    rgba(0x3a5f8a99),
                ));
            }
        }

        TextPrepaint { lines, line_height, caret, selections, scroll_x }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        p: &mut TextPrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus_handle.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.input.clone()), cx);

        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for q in p.selections.drain(..) {
                window.paint_quad(q);
            }
            let x0 = bounds.left() - p.scroll_x;
            for (i, line) in p.lines.iter().enumerate() {
                let _ = line.paint(
                    point(x0, bounds.top() + p.line_height * i as f32),
                    p.line_height,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            if focus.is_focused(window) {
                if let Some(q) = p.caret.take() {
                    window.paint_quad(q);
                }
            }
        });

        let lines = std::mem::take(&mut p.lines);
        let lh = p.line_height;
        let sx = p.scroll_x;
        self.input.update(cx, |input, _| {
            input.last_lines = lines;
            input.last_bounds = Some(bounds);
            input.last_line_height = lh;
            input.scroll_x = sx;
        });
    }
}
