//! A deliberately tiny editable text field.
//!
//! The rest of pm is read-only (even the diff caret), so there is no editor
//! infrastructure to lean on. This is a stopgap: a focusable box that collects
//! keystrokes into a `String`, with a byte cursor and the bare minimum of
//! navigation. No selection, no IME, no clipboard, no undo. Replace it the day
//! pm grows a real text editor.

use gpui::{
    div, prelude::*, px, rgb, Context, Div, FocusHandle, KeyDownEvent, SharedString, Stateful,
    Window,
};

use crate::app::Pm;
use crate::theme::{BG, BORDER, DIM, SELECT, TEXT};

pub struct TextInput {
    pub content: String,
    /// Byte offset of the caret within `content` (always on a char boundary).
    cursor: usize,
    pub multiline: bool,
    pub focus: FocusHandle,
}

impl TextInput {
    pub fn single(cx: &mut Context<Pm>) -> Self {
        Self { content: String::new(), cursor: 0, multiline: false, focus: cx.focus_handle() }
    }

    pub fn multi(cx: &mut Context<Pm>) -> Self {
        Self { content: String::new(), cursor: 0, multiline: true, focus: cx.focus_handle() }
    }

    /// Take the trimmed content, leaving the field empty.
    pub fn take(&mut self) -> String {
        let out = std::mem::take(&mut self.content);
        self.cursor = 0;
        out.trim().to_string()
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor = 0;
    }

    fn insert(&mut self, s: &str) {
        self.content.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.content[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.content.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    fn delete(&mut self) {
        if self.cursor >= self.content.len() {
            return;
        }
        let next = self.content[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor + i)
            .unwrap_or(self.content.len());
        self.content.replace_range(self.cursor..next, "");
    }

    fn left(&mut self) {
        self.cursor = self.content[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
    }

    fn right(&mut self) {
        if let Some((i, c)) = self.content[self.cursor..].char_indices().next() {
            self.cursor += i + c.len_utf8();
        }
    }

    /// Feed a key event. Returns `true` when the field was "submitted" (Enter on
    /// a single-line field) so the caller can act on it.
    pub fn key(&mut self, e: &KeyDownEvent, _window: &mut Window, _cx: &mut Context<Pm>) -> bool {
        let ks = &e.keystroke;
        if ks.modifiers.control || ks.modifiers.platform || ks.modifiers.function {
            return false;
        }
        match ks.key.as_str() {
            "backspace" => self.backspace(),
            "delete" => self.delete(),
            "left" => self.left(),
            "right" => self.right(),
            "home" => self.cursor = 0,
            "end" => self.cursor = self.content.len(),
            "enter" if self.multiline => self.insert("\n"),
            "enter" => return true,
            "space" => self.insert(" "),
            "tab" => {}
            _ => {
                if let Some(ch) = ks.key_char.as_deref().filter(|c| !c.is_empty()) {
                    if !ch.chars().any(|c| c.is_control()) {
                        self.insert(ch);
                    }
                }
            }
        }
        false
    }

    /// Content with a visible caret glyph spliced in at the cursor (for display
    /// only, when focused).
    fn with_caret(&self) -> String {
        let mut s = self.content.clone();
        s.insert(self.cursor, '\u{2502}');
        s
    }
}

/// Render a text field. The caller wires the key handler:
/// `.on_key_down(cx.listener(|pm, e, w, cx| { pm.<field>.key(e, w, cx); cx.notify(); }))`.
pub fn text_input(
    id: impl Into<gpui::ElementId>,
    ti: &TextInput,
    placeholder: &str,
    focused: bool,
) -> Stateful<Div> {
    let (text, dim) = if ti.content.is_empty() && !focused {
        (placeholder.to_string(), true)
    } else if focused {
        (ti.with_caret(), false)
    } else {
        (ti.content.clone(), false)
    };

    let mut lines: Vec<Div> = text
        .split('\n')
        .map(|l| div().child(SharedString::from(l.to_string())))
        .collect();
    if lines.is_empty() {
        lines.push(div().child(""));
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
        .text_color(rgb(if dim { DIM } else { TEXT }))
        .when(ti.multiline, |d| d.min_h(px(64.0)).whitespace_normal())
        .children(lines)
}
