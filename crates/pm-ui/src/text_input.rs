//! A small editable text field with a selection, clipboard, and word motions.
//!
//! The rest of pm is read-only, so there is no editor to lean on. This stays
//! deliberately compact — no IME, no click-to-place-caret, no undo — but it
//! handles the things that make a box feel broken without them: Ctrl+A, cut /
//! copy / paste, shift-selection, and Ctrl+Backspace.

use gpui::{
    div, prelude::*, px, rgb, ClipboardItem, Context, Div, FocusHandle, KeyDownEvent, SharedString,
    Stateful, Window,
};

use crate::app::Pm;
use crate::theme::{BG, BORDER, DIM, SELECT, TEXT};

/// Caret / selection glyph height, px.
const CARET_H: f32 = 15.0;

pub struct TextInput {
    pub content: String,
    /// Byte offset of the caret (char boundary).
    cursor: usize,
    /// Selection anchor; equal to `cursor` when there is no selection.
    anchor: usize,
    pub multiline: bool,
    pub focus: FocusHandle,
}

impl TextInput {
    pub fn single(cx: &mut Context<Pm>) -> Self {
        Self { content: String::new(), cursor: 0, anchor: 0, multiline: false, focus: cx.focus_handle() }
    }

    pub fn multi(cx: &mut Context<Pm>) -> Self {
        Self { content: String::new(), cursor: 0, anchor: 0, multiline: true, focus: cx.focus_handle() }
    }

    /// Take the trimmed content, leaving the field empty.
    pub fn take(&mut self) -> String {
        let out = std::mem::take(&mut self.content);
        self.cursor = 0;
        self.anchor = 0;
        out.trim().to_string()
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor = 0;
        self.anchor = 0;
    }

    // ---- selection helpers -------------------------------------------------

    fn sel(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }

    fn has_sel(&self) -> bool {
        self.cursor != self.anchor
    }

    fn selected(&self) -> Option<String> {
        let (a, b) = self.sel();
        (a != b).then(|| self.content[a..b].to_string())
    }

    /// Remove the selection if any; returns whether something was deleted.
    fn delete_sel(&mut self) -> bool {
        let (a, b) = self.sel();
        if a == b {
            return false;
        }
        self.content.replace_range(a..b, "");
        self.cursor = a;
        self.anchor = a;
        true
    }

    fn insert(&mut self, s: &str) {
        self.delete_sel();
        self.content.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.anchor = self.cursor;
    }

    /// Move the caret to `pos`, extending the selection when `extend`.
    fn go(&mut self, pos: usize, extend: bool) {
        self.cursor = pos.min(self.content.len());
        if !extend {
            self.anchor = self.cursor;
        }
    }

    // ---- position math ---------------------------------------------------

    fn prev_char(&self, p: usize) -> usize {
        self.content[..p].char_indices().next_back().map(|(i, _)| i).unwrap_or(0)
    }

    fn next_char(&self, p: usize) -> usize {
        self.content[p..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| p + i)
            .unwrap_or(self.content.len())
    }

    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn prev_word(&self, mut p: usize) -> usize {
        let s = &self.content;
        while p > 0 && !Self::is_word(s[..p].chars().next_back().unwrap()) {
            p = self.prev_char(p);
        }
        while p > 0 && Self::is_word(s[..p].chars().next_back().unwrap()) {
            p = self.prev_char(p);
        }
        p
    }

    fn next_word(&self, mut p: usize) -> usize {
        let s = &self.content;
        let len = s.len();
        while p < len && !Self::is_word(s[p..].chars().next().unwrap()) {
            p = self.next_char(p);
        }
        while p < len && Self::is_word(s[p..].chars().next().unwrap()) {
            p = self.next_char(p);
        }
        p
    }

    fn line_start(&self, p: usize) -> usize {
        self.content[..p].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self, p: usize) -> usize {
        self.content[p..].find('\n').map(|i| p + i).unwrap_or(self.content.len())
    }

    /// The same visual column one line up / down (`dir` = -1 / +1).
    fn vertical(&self, p: usize, dir: isize) -> usize {
        let col = p - self.line_start(p);
        if dir < 0 {
            let ls = self.line_start(p);
            if ls == 0 {
                return 0;
            }
            let prev_start = self.line_start(ls - 1);
            (prev_start + col).min(ls - 1)
        } else {
            let le = self.line_end(p);
            if le == self.content.len() {
                return le;
            }
            let next_start = le + 1;
            let next_end = self.line_end(next_start);
            (next_start + col).min(next_end)
        }
    }

    /// Feed a key event. Returns `true` on submit (Enter in a single-line field).
    pub fn key(&mut self, e: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Pm>) -> bool {
        let m = &e.keystroke.modifiers;
        let shift = m.shift;
        let key = e.keystroke.key.as_str();

        if m.secondary() && !m.alt {
            match key {
                "a" => {
                    self.anchor = 0;
                    self.cursor = self.content.len();
                }
                "c" => {
                    if let Some(s) = self.selected() {
                        cx.write_to_clipboard(ClipboardItem::new_string(s));
                    }
                }
                "x" => {
                    if let Some(s) = self.selected() {
                        cx.write_to_clipboard(ClipboardItem::new_string(s));
                        self.delete_sel();
                    }
                }
                "v" => {
                    if let Some(t) = cx.read_from_clipboard().and_then(|i| i.text()) {
                        let t = if self.multiline {
                            t.replace('\r', "")
                        } else {
                            t.replace(['\n', '\r'], " ")
                        };
                        self.insert(&t);
                    }
                }
                "backspace" => {
                    if !self.delete_sel() {
                        let p = self.prev_word(self.cursor);
                        self.content.replace_range(p..self.cursor, "");
                        self.cursor = p;
                        self.anchor = p;
                    }
                }
                "left" => self.go(self.prev_word(self.cursor), shift),
                "right" => self.go(self.next_word(self.cursor), shift),
                _ => {}
            }
            return false;
        }

        match key {
            "backspace" => {
                if !self.delete_sel() && self.cursor > 0 {
                    let p = self.prev_char(self.cursor);
                    self.content.replace_range(p..self.cursor, "");
                    self.cursor = p;
                    self.anchor = p;
                }
            }
            "delete" => {
                if !self.delete_sel() && self.cursor < self.content.len() {
                    let n = self.next_char(self.cursor);
                    self.content.replace_range(self.cursor..n, "");
                }
            }
            "left" => {
                let to = if self.has_sel() && !shift { self.sel().0 } else { self.prev_char(self.cursor) };
                self.go(to, shift);
            }
            "right" => {
                let to = if self.has_sel() && !shift { self.sel().1 } else { self.next_char(self.cursor) };
                self.go(to, shift);
            }
            "up" if self.multiline => {
                let to = self.vertical(self.cursor, -1);
                self.go(to, shift);
            }
            "down" if self.multiline => {
                let to = self.vertical(self.cursor, 1);
                self.go(to, shift);
            }
            "home" => self.go(self.line_start(self.cursor), shift),
            "end" => self.go(self.line_end(self.cursor), shift),
            "enter" if self.multiline => self.insert("\n"),
            "enter" => return true,
            "escape" => self.anchor = self.cursor,
            "space" => self.insert(" "),
            "tab" => {}
            _ => {
                if let Some(ch) = e.keystroke.key_char.as_deref().filter(|c| !c.is_empty()) {
                    if !ch.chars().any(|c| c.is_control()) {
                        self.insert(ch);
                    }
                }
            }
        }
        false
    }
}

/// One rendered line: text split around the selection, with a 1px caret quad
/// spliced in at the cursor (so the caret never shifts the text it sits in).
fn line_row(line: &str, sel: Option<(usize, usize)>, caret: Option<usize>) -> Div {
    let len = line.len();
    let mut cuts = vec![0usize, len];
    if let Some((a, b)) = sel {
        cuts.push(a);
        cuts.push(b);
    }
    if let Some(c) = caret {
        cuts.push(c);
    }
    cuts.retain(|&p| p <= len);
    cuts.sort_unstable();
    cuts.dedup();

    let caret_quad = || div().w(px(1.0)).h(px(CARET_H)).bg(rgb(TEXT));
    let mut row = div().flex().flex_row().items_center().min_h(px(CARET_H + 3.0));

    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if caret == Some(a) {
            row = row.child(caret_quad());
        }
        let selected = sel.is_some_and(|(s, e)| s != e && a >= s && b <= e);
        let mut span = div().child(SharedString::from(line[a..b].to_string()));
        if selected {
            span = span.bg(rgb(SELECT));
        }
        row = row.child(span);
    }
    if caret == Some(len) {
        row = row.child(caret_quad());
    }
    row
}

/// Render a text field. The caller wires the handlers:
/// `.on_key_down(cx.listener(|pm, e, w, cx| { pm.<field>.key(e, w, cx); cx.notify(); }))`
/// and a mouse-down that focuses `pm.<field>.focus`.
pub fn text_input(
    id: impl Into<gpui::ElementId>,
    ti: &TextInput,
    placeholder: &str,
    focused: bool,
) -> Stateful<Div> {
    let mut body = div().flex().flex_col();

    if ti.content.is_empty() && !focused {
        body = body.child(
            div()
                .text_color(rgb(DIM))
                .child(SharedString::from(placeholder.to_string())),
        );
    } else {
        let (s0, s1) = ti.sel();
        let mut off = 0usize;
        for line in ti.content.split('\n') {
            let (ls, le) = (off, off + line.len());
            let sel = (s0 < s1 && s0 <= le && s1 >= ls)
                .then(|| (s0.max(ls) - ls, s1.min(le) - ls));
            let caret = (focused && ti.cursor >= ls && ti.cursor <= le).then_some(ti.cursor - ls);
            body = body.child(line_row(line, sel, caret));
            off = le + 1;
        }
    }

    div()
        .id(id)
        .track_focus(&ti.focus)
        .w_full()
        .px_2()
        .py_1()
        .bg(rgb(BG))
        .border_1()
        .border_color(rgb(if focused { SELECT } else { BORDER }))
        .rounded_sm()
        .text_color(rgb(TEXT))
        .cursor_text()
        .when(ti.multiline, |d| d.min_h(px(64.0)))
        .child(body)
}
