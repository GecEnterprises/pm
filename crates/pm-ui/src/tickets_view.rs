//! The Tickets pane: a read + create view over `.pm/pm.json5`.
//!
//! Scope for now: list tickets, read a ticket, add comments, create new tickets.
//! No field editing, no deleting, no diff-gutter anchoring yet (all next-pass).

use std::path::PathBuf;

use gpui::{
    div, prelude::*, px, rgb, Context, KeyDownEvent, MouseButton, SharedString, Window,
};

use pm_core::Ticket;

use crate::app::{Compose, Pm, View};
use crate::history_view::rel_time;
use crate::text_input::text_input;
use crate::theme::*;

fn chip(text: impl Into<SharedString>, color: u32) -> impl IntoElement {
    div()
        .px_1()
        .rounded_sm()
        .bg(rgb(BORDER))
        .text_color(rgb(color))
        .text_size(px(11.0))
        .child(text.into())
}

/// Split a plain-text block into one div per line (gpui doesn't break on `\n`).
fn text_block(s: &str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .text_color(rgb(TEXT))
        .children(
            s.split('\n')
                .map(|l| div().child(SharedString::from(l.to_string()))),
        )
}

impl Pm {
    pub(crate) fn tickets_body(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            .bg(rgb(BG))
            .child(self.ticket_list(cx))
            .child(self.ticket_detail(window, cx))
    }

    fn ticket_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pm = &self.state.pm;
        let n = pm.tickets.len();

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .flex_none()
            .h(px(SECTION_HEADER_H))
            .px_2()
            .text_color(rgb(DIM))
            .child(SharedString::from(format!("Tickets  ({n})")))
            .child(
                div()
                    .id("new-ticket")
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(rgb(TEXT))
                    .hover(|s| s.bg(rgb(BORDER)))
                    .child("+ New")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, window, cx| {
                            pm.composing = Some(Compose::NewTicket);
                            pm.selected_ticket = None;
                            pm.new_ticket_title.clear();
                            pm.new_ticket_body.clear();
                            window.focus(&pm.new_ticket_title.focus, cx);
                            cx.notify();
                        }),
                    ),
            );

        let mut list = div().id("ticket-list").flex_1().overflow_y_scroll();
        for (i, t) in pm.tickets.iter().enumerate() {
            let id = t.id;
            let selected = self.selected_ticket == Some(id);
            list = list.child(
                div()
                    .id(("ticket", i))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .cursor_pointer()
                    .when(selected, |s| s.bg(rgb(SELECT)))
                    .when(!selected, |s| s.hover(|s| s.bg(rgb(PANEL))))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .text_size(px(11.0))
                            .text_color(rgb(DIM))
                            .child(
                                div()
                                    .text_color(rgb(t.status.color()))
                                    .child(SharedString::from("\u{25cf}")),
                            )
                            .child(SharedString::from(pm.display_id(t)))
                            .child(SharedString::from(t.priority.label())),
                    )
                    .child(
                        div()
                            .text_color(rgb(TEXT))
                            .child(SharedString::from(t.title.clone())),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pm, _, _, cx| {
                            pm.selected_ticket = Some(id);
                            pm.composing = None;
                            pm.comment_box.clear();
                            cx.notify();
                        }),
                    ),
            );
        }

        div()
            .flex_none()
            .w(px(320.0))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(header)
            .child(list)
    }

    fn ticket_detail(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div()
            .id("ticket-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();

        if let Some(err) = &self.state.pm_error {
            col = col.child(
                div()
                    .p_2()
                    .rounded_sm()
                    .bg(rgb(DEL_BG))
                    .text_color(rgb(TEXT))
                    .child(SharedString::from(format!("pm.json5: {err}"))),
            );
        }

        if self.composing == Some(Compose::NewTicket) {
            return col.child(self.new_ticket_form(window, cx)).into_any_element();
        }

        match self.selected_ticket.and_then(|id| self.state.pm.ticket(id)) {
            Some(t) => col.child(self.ticket_card(t, window, cx)).into_any_element(),
            None => col
                .child(
                    div()
                        .text_color(rgb(DIM))
                        .child(SharedString::from("Select a ticket, or press + New.")),
                )
                .into_any_element(),
        }
    }

    fn new_ticket_form(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title_focused = self.new_ticket_title.focus.is_focused(window);
        let body_focused = self.new_ticket_body.focus.is_focused(window);

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(15.0))
                    .text_color(rgb(TEXT))
                    .child(SharedString::from("New ticket")),
            )
            .child(
                text_input("nt-title", &self.new_ticket_title, "Title", title_focused)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, w, cx| {
                            w.focus(&pm.new_ticket_title.focus, cx);
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(|pm, e: &KeyDownEvent, w, cx| {
                        if pm.new_ticket_title.key(e, w, cx) {
                            pm.submit_new_ticket(cx);
                        }
                        cx.notify();
                    })),
            )
            .child(
                text_input("nt-body", &self.new_ticket_body, "Description (optional)", body_focused)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, w, cx| {
                            w.focus(&pm.new_ticket_body.focus, cx);
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(|pm, e: &KeyDownEvent, w, cx| {
                        pm.new_ticket_body.key(e, w, cx);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("nt-create")
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(rgb(SELECT))
                            .text_color(rgb(TEXT))
                            .cursor_pointer()
                            .child("Create")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|pm, _, _, cx| pm.submit_new_ticket(cx)),
                            ),
                    )
                    .child(
                        div()
                            .id("nt-cancel")
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .text_color(rgb(DIM))
                            .cursor_pointer()
                            .child("Cancel")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|pm, _, _, cx| {
                                    pm.composing = None;
                                    cx.notify();
                                }),
                            ),
                    ),
            )
    }

    fn ticket_card(
        &self,
        t: &Ticket,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pm = &self.state.pm;
        let tid = t.id;
        let comment_focused = self.comment_box.focus.is_focused(window);

        let mut meta = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_color(rgb(DIM))
                    .child(SharedString::from(pm.display_id(t))),
            )
            .child(chip(t.status.label(), t.status.color()))
            .child(chip(t.priority.label(), t.priority.color()));
        for label in &t.labels {
            meta = meta.child(chip(label.clone(), CHANGED));
        }
        if let Some(a) = &t.assignee {
            meta = meta.child(
                div()
                    .text_color(rgb(DIM))
                    .child(SharedString::from(format!("@{a}"))),
            );
        }
        meta = meta.child(
            div()
                .text_color(rgb(DIM))
                .text_size(px(11.0))
                .child(SharedString::from(format!("updated {}", rel_time(t.updated)))),
        );

        let mut card = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(17.0))
                    .text_color(rgb(TEXT))
                    .child(SharedString::from(t.title.clone())),
            )
            .child(meta);

        if !t.body.trim().is_empty() {
            card = card.child(text_block(&t.body));
        }

        // Code anchors — read-only for now; click opens the file in File-to-File.
        for (i, a) in t.anchors.iter().enumerate() {
            let file = a.file.clone();
            card = card.child(
                div()
                    .id(("anchor", i))
                    .text_color(rgb(CHANGED))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(PANEL)))
                    .child(SharedString::from(format!(
                        "\u{1f4ce} {}:{}\u{2013}{}",
                        a.file, a.start_line, a.end_line
                    )))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pm, _, _, cx| {
                            pm.view = View::Files;
                            pm.open_path(PathBuf::from(file.clone()));
                            cx.notify();
                        }),
                    ),
            );
        }

        // Comments.
        card = card.child(
            div()
                .mt_2()
                .text_color(rgb(DIM))
                .text_size(px(11.0))
                .child(SharedString::from(format!("Comments  ({})", t.comments.len()))),
        );
        for c in &t.comments {
            card = card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .py_1()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_color(rgb(DIM))
                            .text_size(px(11.0))
                            .child(SharedString::from(format!(
                                "{}  \u{00b7}  {}",
                                if c.author.is_empty() { "someone" } else { &c.author },
                                rel_time(c.created)
                            ))),
                    )
                    .child(text_block(&c.body)),
            );
        }

        card.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .mt_2()
                .child(
                    text_input("comment-box", &self.comment_box, "Add a comment\u{2026}", comment_focused)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|pm, _, w, cx| {
                                w.focus(&pm.comment_box.focus, cx);
                                cx.notify();
                            }),
                        )
                        .on_key_down(cx.listener(|pm, e: &KeyDownEvent, w, cx| {
                            pm.comment_box.key(e, w, cx);
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id("comment-submit")
                        .self_start()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(rgb(SELECT))
                        .text_color(rgb(TEXT))
                        .cursor_pointer()
                        .child("Comment")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |pm, _, _, cx| {
                                let body = pm.comment_box.take();
                                if !body.is_empty() {
                                    pm.state.add_comment(tid, body);
                                    cx.notify();
                                }
                            }),
                        ),
                ),
        )
    }

    fn submit_new_ticket(&mut self, cx: &mut Context<Self>) {
        let title = self.new_ticket_title.take();
        if title.is_empty() {
            return;
        }
        let body = self.new_ticket_body.take();
        let id = self.state.create_ticket(title, body);
        self.selected_ticket = Some(id);
        self.composing = None;
        cx.notify();
    }
}
