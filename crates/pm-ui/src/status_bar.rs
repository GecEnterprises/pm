//! The bottom status bar — branch, open path, caret position, change totals.
//! Modelled on Zed's `workspace::status_bar` (a plain `h_flex`, no fixed height
//! token, muted text).

use gpui::{
    div, prelude::*, px, rgb, Context, Decorations, MouseButton, SharedString, Styled, Window,
};

use pm_core::DiffTarget;

use crate::app::Pm;
use crate::theme::*;

impl Pm {
    pub(crate) fn status_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let decorations = window.window_decorations();
        let bar = div()
            .id("status-bar")
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .flex_none()
            .h(px(STATUS_BAR_H))
            .px_2()
            .gap_3()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(BORDER))
            .text_color(rgb(DIM))
            .text_size(px(11.0))
            .child(self.status_left(cx))
            .child(self.status_right());
        round_bottom(bar, decorations)
    }

    fn status_left(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div().flex().flex_row().items_center().gap_3().min_w_0();

        if let DiffTarget::Commit(oid) = self.state.target {
            let summary = self
                .state
                .commits
                .iter()
                .find(|c| c.id == oid)
                .map(|c| {
                    let s: String = c.summary.chars().take(44).collect();
                    format!("{}  {s}", c.short_id)
                })
                .unwrap_or_else(|| oid.to_string().chars().take(7).collect());
            row = row.child(
                div()
                    .id("diffing")
                    .px_1()
                    .rounded_sm()
                    .bg(rgb(SELECT))
                    .text_color(rgb(TEXT))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.select_commit(0);
                            cx.notify();
                        }),
                    )
                    .child(SharedString::from(format!("Diffing {summary}"))),
            );
        }

        if let Some(branch) = &self.state.branch {
            row = row.child(SharedString::from(format!("\u{2387} {branch}")));
        }

        let path = self
            .state
            .open
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "no file selected".to_string());
        row = row.child(SharedString::from(path));

        if self.state.rows.len() > pm_core::MAX_ROWS {
            row = row.child(
                div()
                    .text_color(rgb(BORDER))
                    .child(SharedString::from(format!(
                        "(showing first {} rows)",
                        pm_core::MAX_ROWS
                    ))),
            );
        }
        row
    }

    fn status_right(&self) -> impl IntoElement {
        let mut row = div().flex().flex_row().items_center().gap_3().flex_none();

        if let Some(cur) = self.state.caret {
            let line = cur.head.file_row + 1;
            let col = self.state.text.char_col(cur.col, cur.head);
            row = row.child(SharedString::from(format!("Ln {line}, Col {col}")));
        }

        let n = self.state.changes.len();
        if n > 0 {
            let (adds, dels): (usize, usize) = self
                .state
                .changes
                .iter()
                .fold((0, 0), |(a, d), c| (a + c.added, d + c.removed));
            let files = if n == 1 { "file" } else { "files" };
            row = row.child(SharedString::from(format!(
                "{n} {files}  +{adds} \u{2212}{dels}"
            )));
        }
        row
    }
}

/// Round the bottom corners of a bottom-edge bar under client-side decorations.
pub(crate) fn round_bottom<T: Styled + IntoElement>(el: T, decorations: Decorations) -> T {
    match decorations {
        Decorations::Client { tiling } => el
            .when(!tiling.bottom && !tiling.left, |b| {
                b.rounded_bl(px(CLIENT_DECORATION_ROUNDING))
            })
            .when(!tiling.bottom && !tiling.right, |b| {
                b.rounded_br(px(CLIENT_DECORATION_ROUNDING))
            }),
        Decorations::Server => el,
    }
}
