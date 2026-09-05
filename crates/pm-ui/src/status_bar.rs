//! The bottom status bar — branch, open path, caret position, change totals.
//! Modelled on Zed's `workspace::status_bar` (a plain `h_flex`, no fixed height
//! token, muted text).

use gpui::{
    deferred, div, prelude::*, px, rgb, svg, Context, Decorations, MouseButton, SharedString, Styled,
    Window,
};

use pm_core::DiffTarget;

use crate::app::Pm;
use crate::config::ConfigStore;
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
            .h(rm(STATUS_BAR_H))
            .px_2()
            .gap_3()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(BORDER))
            .text_color(rgb(DIM))
            .text_size(rm(11.0))
            .child(self.status_left(cx))
            .child(self.status_right(cx))
            // Click-away closer for the "Acting as" popover — mirrors the
            // tickets-pane menus. Sits above everything (deferred), so a second
            // click on the chip is caught here and `stop_propagation` keeps the
            // chip's own toggle from re-opening it in the same event.
            .when(self.user_menu_open, |d| {
                d.child(deferred(
                    div().absolute().inset_0().on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.commit_user(cx);
                            pm.user_menu_open = false;
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    ),
                ))
            });
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

        if !self.state.is_git() {
            row = row.child(
                div()
                    .text_color(rgb(0xe2c08d))
                    .child(SharedString::from("\u{26a0} not a git repository")),
            );
        } else if let Some(branch) = &self.state.branch {
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

    fn status_right(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div().flex().flex_row().items_center().gap_3().flex_none();

        // "Acting as" user section (PM-56) — shows the identity written as the
        // author of tickets / comments, with a popover to change it.
        row = row.child(self.user_chip(cx));

        // Watchjump toggle (PM-30) — only meaningful with a change list.
        if self.state.is_git() {
            let on = ConfigStore::get(cx).watchjump;
            let fg = rgb(if on { TEXT } else { DIM });
            let mut chip = div()
                .id("watchjump")
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .text_color(fg)
                .child(
                    svg()
                        .size(rm(12.0))
                        .flex_none()
                        .text_color(fg)
                        .data(crate::icons::svg_bytes("watchjump.svg")),
                )
                .child(SharedString::from("Watchjump"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _, _, cx| {
                        ConfigStore::update(cx, |c| c.watchjump = !c.watchjump);
                    }),
                );
            chip = if on {
                chip.bg(rgb(SELECT))
            } else {
                chip.hover(|s| s.text_color(rgb(TEXT)))
            };
            row = row.child(chip);
        }

        if let Some(rel) = crate::update::UpdateStatus::available(cx) {
            let tag = rel.tag.clone();
            row = row.child(
                div()
                    .id("update-available")
                    .px_1()
                    .rounded_sm()
                    .bg(rgb(SELECT))
                    .text_color(rgb(TEXT))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.show_about = true;
                            cx.notify();
                        }),
                    )
                    .child(SharedString::from(format!("\u{2191} {tag}"))),
            );
        }

        if self.zoom_pct() != 100 {
            row = row.child(SharedString::from(format!("{}%", self.zoom_pct())));
        }

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

    /// The "Acting as <name>" chip and its popover (PM-56). The chip always
    /// shows `state.author`, which `resolve_author` keeps non-empty (git
    /// `user.name`, else `"unknown"`).
    fn user_chip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.user_menu_open;
        let fg = rgb(if open { TEXT } else { DIM });
        let mut chip = div()
            .id("user")
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_sm()
            .cursor_pointer()
            .text_color(fg)
            .child(
                svg()
                    .size(rm(12.0))
                    .flex_none()
                    .text_color(fg)
                    .data(crate::icons::svg_bytes("user.svg")),
            )
            .child(SharedString::from(self.state.author.clone()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pm, _, _, cx| {
                    pm.user_menu_open = !pm.user_menu_open;
                    pm.filter_menu_open = false;
                    pm.status_menu_open = false;
                    cx.notify();
                    cx.stop_propagation();
                }),
            );
        chip = if open {
            chip.bg(rgb(SELECT))
        } else {
            chip.hover(|s| s.text_color(rgb(TEXT)))
        };

        div().relative().child(chip).when(open, |d| {
            d.child(deferred(
                div()
                    .absolute()
                    .bottom_full()
                    .right_0()
                    .mb(px(4.0))
                    .child(self.user_menu(cx)),
            ))
        })
    }

    /// Body of the "Acting as" popover: a name field, a "Use git username"
    /// action (when a git name is available), and "Reset to git default".
    fn user_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let git_name = self.state.repo.user_name();

        let mut panel = div()
            .occlude()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(220.0))
            .p_2()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded_md()
            .shadow_lg()
            .text_color(rgb(TEXT))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .text_color(rgb(DIM))
                    .text_size(rm(11.0))
                    .child(SharedString::from("Acting as")),
            )
            .child(self.author_box.clone());

        if let Some(name) = git_name {
            let label = format!("Use git username ({name})");
            panel = panel.child(
                div()
                    .id("user-from-git")
                    .px_1()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(rgb(TEXT))
                    .hover(|s| s.bg(rgb(SELECT)))
                    .child(SharedString::from(label))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pm, _, _, cx| {
                            let name = name.clone();
                            pm.author_box.update(cx, |ti, cx| ti.set_text(name, cx));
                            pm.commit_user(cx);
                            pm.user_menu_open = false;
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    ),
            );
        }

        panel.child(
            div()
                .id("user-reset")
                .px_1()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .text_color(rgb(DIM))
                .hover(|s| s.bg(rgb(SELECT)).text_color(rgb(TEXT)))
                .child(SharedString::from("Reset to git default"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|pm, _, _, cx| {
                        pm.reset_user(cx);
                        pm.user_menu_open = false;
                        cx.notify();
                        cx.stop_propagation();
                    }),
                ),
        )
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
