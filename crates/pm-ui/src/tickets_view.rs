//! The Tickets pane: list, read, create, comment, and set status.
//!
//! No diff-gutter anchoring yet (that's PM-2), no priority editing yet (PM-4).

use std::path::PathBuf;

use gpui::{deferred, div, prelude::*, px, rgb, svg, Context, MouseButton, SharedString};

use pm_core::{HistoryEntry, HistoryEvent, Status, Ticket};

use crate::app::{Compose, Pm, View};
use crate::config::ConfigStore;
use crate::history_view::rel_time;
use crate::icons;
use crate::theme::*;

/// What a dropdown row does when clicked.
type MenuAct = Box<dyn Fn(&mut Pm, &mut Context<Pm>)>;
/// One dropdown row: label, status-dot colour, checked, action.
type MenuRow = (SharedString, u32, bool, MenuAct);

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

/// One human-readable line for a history entry (PM-58) — who did what.
fn history_line(h: &HistoryEntry) -> String {
    let who = if h.author.is_empty() { "someone" } else { h.author.as_str() };
    match &h.event {
        HistoryEvent::TitleChanged { old, new } => {
            format!("{who} renamed \u{201c}{old}\u{201d} \u{2192} \u{201c}{new}\u{201d}")
        }
        HistoryEvent::BodyChanged { .. } => format!("{who} edited the description"),
        HistoryEvent::StatusChanged { old, new } => {
            format!("{who} changed status: {} \u{2192} {}", old.label(), new.label())
        }
        HistoryEvent::PriorityChanged { old, new } => {
            format!("{who} changed priority: {} \u{2192} {}", old.label(), new.label())
        }
        HistoryEvent::LabelsChanged { old, new } => format!(
            "{who} changed labels: [{}] \u{2192} [{}]",
            old.join(", "),
            new.join(", ")
        ),
        HistoryEvent::AssigneeChanged { old, new } => format!(
            "{who} reassigned: {} \u{2192} {}",
            old.as_deref().unwrap_or("nobody"),
            new.as_deref().unwrap_or("nobody")
        ),
        HistoryEvent::Commented { .. } => format!("{who} commented"),
    }
}

impl Pm {
    pub(crate) fn tickets_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            .relative()
            .bg(rgb(BG))
            // Same resizable sidebar as File-to-File (PM-42): the canvas keeps
            // `root_bounds` current, the handler routes the drag to `sidebar_w`.
            .child(self.root_bounds_canvas(cx))
            .on_drag_move(cx.listener(|pm, ev, _w, cx| Pm::route_sidebar_drag(pm, ev, cx)))
            // Click-away closer for the filter / status popovers. It sits above
            // everything (deferred), so it also catches a second click on the
            // trigger button — `stop_propagation` then keeps that button's own
            // toggle from re-opening the menu in the same event.
            .when(self.filter_menu_open || self.status_menu_open, |d| {
                d.child(deferred(
                    div().absolute().inset_0().on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.filter_menu_open = false;
                            pm.status_menu_open = false;
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    ),
                ))
            })
            .child(self.ticket_list(cx))
            .child(self.ticket_detail(cx))
    }

    /// A dropdown panel body: one row per `(label, dot colour, checked, action)`,
    /// with an optional leading check column. The caller positions it (a
    /// `deferred` absolute wrapper).
    fn menu_panel(
        &self,
        cx: &mut Context<Self>,
        checks: bool,
        rows: Vec<MenuRow>,
    ) -> impl IntoElement {
        let mut panel = div()
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(160.0))
            .py_1()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded_md()
            .shadow_lg()
            .text_color(rgb(TEXT))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        for (i, (label, color, on, act)) in rows.into_iter().enumerate() {
            panel = panel.child(
                div()
                    .id(("menu-row", i))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(SELECT)))
                    .when(checks, |d| {
                        d.child(
                            div()
                                .w(px(12.0))
                                .text_color(rgb(CHANGED))
                                .child(if on { "\u{2713}" } else { "" }),
                        )
                    })
                    .child(
                        div()
                            .text_color(rgb(color))
                            .child(SharedString::from("\u{25cf}"))
                            .text_size(px(9.0)),
                    )
                    .child(label)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pm, _, _, cx| {
                            act(pm, cx);
                            cx.stop_propagation();
                        }),
                    ),
            );
        }
        panel
    }

    /// The list-header search icon (PM-75) — toggles the search box below the
    /// header. Closing it clears the query so a hidden filter can't linger.
    fn search_toggle_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.ticket_search_open;
        div()
            .id("ticket-search-toggle")
            .px_1()
            .rounded_sm()
            .cursor_pointer()
            .text_color(rgb(TEXT))
            .when(open, |s| s.bg(rgb(BORDER)))
            .hover(|s| s.bg(rgb(BORDER)))
            .child(
                svg()
                    .size(rm(13.0))
                    .flex_none()
                    .text_color(rgb(TEXT))
                    .data(icons::svg_bytes("search.svg")),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pm, _, window, cx| {
                    pm.ticket_search_open = !pm.ticket_search_open;
                    if pm.ticket_search_open {
                        pm.ticket_search.update(cx, |ti, cx| ti.focus(window, cx));
                    } else {
                        pm.ticket_search.update(cx, |ti, cx| ti.reset(cx));
                        pm.ticket_list_shown_count = None;
                    }
                    cx.notify();
                }),
            )
    }

    /// The list-header `Filter (n/5) ▾` button + its status-checklist popover.
    fn filter_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.filter_menu_open;
        let rows: Vec<MenuRow> = Status::ALL
            .iter()
            .map(|&s| {
                let on = self.ticket_filter.contains(&s);
                let act: MenuAct = Box::new(move |pm, cx| {
                    if !pm.ticket_filter.remove(&s) {
                        pm.ticket_filter.insert(s);
                    }
                    cx.notify();
                });
                (SharedString::from(s.label()), s.color(), on, act)
            })
            .collect();

        div()
            .relative()
            .child(
                div()
                    .id("ticket-filter")
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(rgb(TEXT))
                    .when(open, |s| s.bg(rgb(BORDER)))
                    .hover(|s| s.bg(rgb(BORDER)))
                    .child(SharedString::from(format!(
                        "Filter ({}/5) \u{25be}",
                        self.ticket_filter.len()
                    )))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.filter_menu_open = !pm.filter_menu_open;
                            pm.status_menu_open = false;
                            pm.user_menu_open = false;
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    ),
            )
            .when(open, |d| {
                d.child(deferred(
                    div()
                        .absolute()
                        .top_full()
                        .right_0()
                        .mt(px(2.0))
                        .child(self.menu_panel(cx, true, rows)),
                ))
            })
    }

    /// Tickets visible after the status filter + search query (PM-75) — the
    /// single source of truth for "what the list shows", shared by the
    /// renderer and the auto-select logic below.
    fn shown_tickets(&self, cx: &Context<Self>) -> Vec<&Ticket> {
        let pm = &self.state.pm;
        let query = self.ticket_search.read(cx).content().trim().to_lowercase();
        pm.tickets
            .iter()
            .filter(|t| self.ticket_filter.contains(&t.status))
            .filter(|t| {
                query.is_empty()
                    || t.title.to_lowercase().contains(&query)
                    || pm.display_id(t).to_lowercase().contains(&query)
            })
            .collect()
    }

    /// Called on every search-box keystroke (PM-75). Jumps the selection to
    /// the new first result when the list just narrowed; a widening edit
    /// (e.g. backspace) leaves whatever's selected alone.
    pub(crate) fn autoselect_on_narrow(&mut self, cx: &mut Context<Self>) {
        let shown_ids: Vec<u64> = self.shown_tickets(cx).iter().map(|t| t.id).collect();
        let narrowed = self.ticket_list_shown_count.is_some_and(|prev| shown_ids.len() < prev);
        self.ticket_list_shown_count = Some(shown_ids.len());
        if narrowed {
            self.selected_ticket = shown_ids.first().copied();
            self.composing = None;
            cx.notify();
        }
    }

    /// Default to the first visible ticket when the Tickets pane is opened
    /// with nothing selected yet.
    pub(crate) fn autoselect_first_ticket(&mut self, cx: &mut Context<Self>) {
        if self.selected_ticket.is_none() {
            self.selected_ticket = self.shown_tickets(cx).first().map(|t| t.id);
        }
    }

    fn ticket_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pm = &self.state.pm;
        let shown = self.shown_tickets(cx);
        let count = if shown.len() == pm.tickets.len() {
            format!("Tickets  ({})", shown.len())
        } else {
            format!("Tickets  ({} of {})", shown.len(), pm.tickets.len())
        };

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .flex_none()
            .h(px(SECTION_HEADER_H))
            .px_2()
            .text_color(rgb(DIM))
            .child(SharedString::from(count))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(self.search_toggle_button(cx))
                    .child(self.filter_button(cx))
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
                                    pm.filter_menu_open = false;
                                    pm.status_menu_open = false;
                                    pm.new_ticket_body.update(cx, |ti, cx| ti.reset(cx));
                                    pm.new_ticket_title.update(cx, |ti, cx| {
                                        ti.reset(cx);
                                        ti.focus(window, cx);
                                    });
                                    pm.comment_box.update(cx, |ti, cx| ti.reset(cx));
                                    cx.notify();
                                }),
                            ),
                    ),
            );

        let mut list = div().id("ticket-list").flex_1().overflow_y_scroll();
        if shown.is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_color(rgb(DIM))
                    .child(SharedString::from("No tickets match.")),
            );
        }
        for (i, t) in shown.into_iter().enumerate() {
            let id = t.id;
            let selected = self.selected_ticket == Some(id);
            let closed = t.status.is_closed();
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
                            .child(SharedString::from(format!(
                                "{}  \u{00b7}  {}",
                                t.status.label(),
                                t.priority.label()
                            ))),
                    )
                    .child(
                        div()
                            .text_color(rgb(if closed { DIM } else { TEXT }))
                            .when(closed, |d| d.line_through())
                            .child(SharedString::from(t.title.clone())),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pm, _, _, cx| {
                            pm.selected_ticket = Some(id);
                            pm.composing = None;
                            pm.status_menu_open = false;
                            pm.comment_box.update(cx, |ti, cx| ti.reset(cx));
                            cx.notify();
                        }),
                    ),
            );
        }

        div()
            .relative()
            .flex_none()
            .w(px(self.sidebar_w))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(header)
            .when(self.ticket_search_open, |d| {
                d.child(
                    div()
                        .flex_none()
                        .px_2()
                        .pb_1()
                        .child(self.ticket_search.clone()),
                )
            })
            .child(list)
            .child(self.sidebar_resize_handle())
    }

    fn ticket_detail(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            return col.child(self.new_ticket_form(cx)).into_any_element();
        }

        match self.selected_ticket.and_then(|id| self.state.pm.ticket(id)) {
            Some(t) => col.child(self.ticket_card(t, cx)).into_any_element(),
            None => col
                .child(
                    div()
                        .text_color(rgb(DIM))
                        .child(SharedString::from("Select a ticket, or press + New.")),
                )
                .into_any_element(),
        }
    }

    fn new_ticket_form(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(self.new_ticket_title.clone())
            .child(self.new_ticket_body.clone())
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

    /// The clickable status chip + its status-picker popover.
    fn status_button(&self, tid: u64, current: Status, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.status_menu_open;
        let rows: Vec<MenuRow> = Status::ALL
            .iter()
            .map(|&s| {
                let act: MenuAct = Box::new(move |pm, cx| {
                    pm.state.set_ticket_status(tid, s, None);
                    pm.status_menu_open = false;
                    cx.notify();
                });
                (SharedString::from(s.label()), s.color(), s == current, act)
            })
            .collect();

        div()
            .relative()
            .child(
                div()
                    .id("status-picker")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .rounded_sm()
                    .bg(rgb(BORDER))
                    .text_color(rgb(current.color()))
                    .text_size(px(11.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(SELECT)))
                    .child(SharedString::from(format!("{}  \u{25be}", current.label())))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.status_menu_open = !pm.status_menu_open;
                            pm.filter_menu_open = false;
                            pm.user_menu_open = false;
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    ),
            )
            .when(open, |d| {
                d.child(deferred(
                    div()
                        .absolute()
                        .top_full()
                        .left_0()
                        .mt(px(2.0))
                        .child(self.menu_panel(cx, true, rows)),
                ))
            })
    }

    fn ticket_card(&self, t: &Ticket, cx: &mut Context<Self>) -> impl IntoElement {
        let pm = &self.state.pm;
        let tid = t.id;

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
            .child(self.status_button(tid, t.status, cx))
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
        if !t.author.trim().is_empty() {
            meta = meta.child(
                div()
                    .text_color(rgb(DIM))
                    .text_size(px(11.0))
                    .child(SharedString::from(format!("by {}", t.author))),
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

        // History — field edits and a "commented" marker per comment, oldest
        // first; the comment's own text is rendered below, not repeated here.
        if !t.history.is_empty() {
            card = card.child(
                div()
                    .mt_2()
                    .text_color(rgb(DIM))
                    .text_size(px(11.0))
                    .child(SharedString::from(format!("History  ({})", t.history.len()))),
            );
            for h in &t.history {
                card = card.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_size(px(11.0))
                        .text_color(rgb(DIM))
                        .child(SharedString::from(rel_time(h.at)))
                        .child(SharedString::from(history_line(h))),
                );
            }
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
                .child(self.comment_box.clone())
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
                                let body = pm.comment_box.update(cx, |ti, cx| ti.take(cx));
                                if !body.is_empty() {
                                    pm.state.add_comment(tid, body, None);
                                    cx.notify();
                                }
                            }),
                        ),
                ),
        )
    }

    /// Adopt whatever is in the "Acting as" box as this window's author and
    /// persist it to `~/.pm/config.json` (PM-15, PM-56). A blank box leaves the
    /// current identity untouched — use [`reset_user`](Self::reset_user) to fall
    /// back to the git default.
    pub(crate) fn commit_user(&mut self, cx: &mut Context<Self>) {
        let name = self.author_box.read(cx).content().trim().to_string();
        if name.is_empty() || name == self.state.author {
            return;
        }
        self.state.author = name.clone();
        ConfigStore::update(cx, move |c| c.author = name);
    }

    /// Clear the persisted author so attribution falls back to the git
    /// `user.name` (PM-56).
    pub(crate) fn reset_user(&mut self, cx: &mut Context<Self>) {
        ConfigStore::update(cx, |c| c.author = String::new());
        self.state.author = pm_core::resolve_author(None, &self.state.repo);
        let name = self.state.author.clone();
        self.author_box.update(cx, |ti, cx| ti.set_text(name, cx));
    }

    pub(crate) fn submit_new_ticket(&mut self, cx: &mut Context<Self>) {
        let title = self.new_ticket_title.update(cx, |ti, cx| ti.take(cx));
        if title.is_empty() {
            return;
        }
        let body = self.new_ticket_body.update(cx, |ti, cx| ti.take(cx));
        let id = self.state.create_ticket(title, body, None);
        self.selected_ticket = Some(id);
        self.composing = None;
        cx.notify();
    }
}
