//! The Tickets pane: list, read, create, comment, and set status.
//!
//! No diff-gutter anchoring yet (that's PM-2), no priority editing yet (PM-4).

use std::path::PathBuf;

use gpui::{
    deferred, div, prelude::*, px, rgb, svg, Context, Hsla, MouseButton, SharedString, Window,
};

use pm_core::ticket_list::{active_statuses, TicketSort};
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

fn chip(text: impl Into<SharedString>, color: Hsla, bg: Hsla) -> impl IntoElement {
    div()
        .px_1()
        .rounded_sm()
        .bg(bg)
        .text_color(color)
        .text_size(px(11.0))
        .child(text.into())
}

/// One human-readable line for a history entry (PM-58) — who did what.
fn history_line(h: &HistoryEntry) -> String {
    let who = if h.author.is_empty() {
        "someone"
    } else {
        h.author.as_str()
    };
    match &h.event {
        HistoryEvent::RelationChanged {
            source,
            target,
            relation,
            removed,
        } => format!(
            "{who} {} link: #{source} {} #{target}",
            if *removed { "removed" } else { "added" },
            relation.label().to_lowercase()
        ),
        HistoryEvent::TitleChanged { old, new } => {
            format!("{who} renamed \u{201c}{old}\u{201d} \u{2192} \u{201c}{new}\u{201d}")
        }
        HistoryEvent::BodyChanged { .. } => format!("{who} edited the description"),
        HistoryEvent::StatusChanged { old, new } => {
            format!(
                "{who} changed status: {} \u{2192} {}",
                old.label(),
                new.label()
            )
        }
        HistoryEvent::PriorityChanged { old, new } => {
            format!(
                "{who} changed priority: {} \u{2192} {}",
                old.label(),
                new.label()
            )
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
    /// Ctrl+F in the Tickets view (PM-80) — opens the search box (if
    /// collapsed) and focuses it, same as clicking the search icon. A no-op
    /// outside the Tickets view, so it doesn't steal the shortcut elsewhere.
    pub(crate) fn find_tickets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view != View::Tickets {
            return;
        }
        self.ticket_search_open = true;
        self.ticket_search.update(cx, |ti, cx| ti.focus(window, cx));
        cx.notify();
    }

    pub(crate) fn tickets_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            .relative()
            .bg(cx.theme().colors.bg)
            // Same resizable sidebar as File-to-File (PM-42): the canvas keeps
            // `root_bounds` current, the handler routes the drag to `sidebar_w`.
            .child(self.root_bounds_canvas(cx))
            .on_drag_move(cx.listener(|pm, ev, _w, cx| Pm::route_sidebar_drag(pm, ev, cx)))
            // Click-away closer for the filter / status popovers. It sits above
            // everything (deferred), so it also catches a second click on the
            // trigger button — `stop_propagation` then keeps that button's own
            // toggle from re-opening the menu in the same event.
            .when(
                self.filter_menu_open || self.status_menu_open || self.sort_menu_open,
                |d| {
                    d.child(deferred(div().absolute().inset_0().on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.filter_menu_open = false;
                            pm.status_menu_open = false;
                            pm.sort_menu_open = false;
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    )))
                },
            )
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
            .bg(cx.theme().colors.panel)
            .border_1()
            .border_color(cx.theme().colors.border)
            .rounded_md()
            .shadow_lg()
            .text_color(cx.theme().colors.text)
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
                    .hover(|s| s.bg(cx.theme().colors.select))
                    .when(checks, |d| {
                        d.child(
                            div()
                                .w(px(12.0))
                                .text_color(cx.theme().colors.changed)
                                .child(if on { "\u{2713}" } else { "" }),
                        )
                    })
                    .child(
                        div()
                            .text_color(rgb(color))
                            .child(SharedString::from("\u{25cf}"))
                            .text_size(px(9.0)),
                    )
                    .child(div().min_w_0().overflow_hidden().child(label))
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
            .text_color(cx.theme().colors.text)
            .when(open, |s| s.bg(cx.theme().colors.border))
            .hover(|s| s.bg(cx.theme().colors.border))
            .child(
                svg()
                    .size(cx.theme().rm(13.0))
                    .flex_none()
                    .text_color(cx.theme().colors.text)
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
        let mut rows: Vec<MenuRow> = Status::ALL
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

        for label in self
            .project_labels()
            .into_iter()
            .chain(self.ticket_label_filter.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
        {
            let on = self.ticket_label_filter.contains(&label);
            let text = SharedString::from(format!("Label: {label}"));
            rows.push((
                text,
                0x6ca4dc,
                on,
                Box::new(move |pm, cx| {
                    if !pm.ticket_label_filter.remove(&label) {
                        pm.ticket_label_filter.insert(label.clone());
                    }
                    cx.notify();
                }),
            ));
        }

        div()
            .relative()
            .child(
                div()
                    .id("ticket-filter")
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(cx.theme().colors.text)
                    .when(open, |s| s.bg(cx.theme().colors.border))
                    .hover(|s| s.bg(cx.theme().colors.border))
                    .child("Filter \u{25be}")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.filter_menu_open = !pm.filter_menu_open;
                            pm.status_menu_open = false;
                            pm.sort_menu_open = false;
                            pm.user_menu_open = false;
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    ),
            )
            .when(open, |d| {
                d.child(deferred(
                    div().absolute().top_full().right_0().mt(px(2.0)).child(
                        div()
                            .id("ticket-filter-options")
                            .max_h(px(320.0))
                            .w(px(self.sidebar_w.min(320.0)))
                            .overflow_y_scroll()
                            .child(self.menu_panel(cx, true, rows)),
                    ),
                ))
            })
    }

    /// Tickets visible after the status filter + search query (PM-75) — the
    /// single source of truth for "what the list shows", shared by the
    /// renderer and the auto-select logic below.
    fn shown_tickets(&self, cx: &Context<Self>) -> Vec<&Ticket> {
        pm_core::ticket_list::shown_tickets(
            &self.state.pm,
            &self.ticket_filter,
            &self.ticket_label_filter,
            self.ticket_search.read(cx).content(),
            ConfigStore::get(cx).ticket_sort,
        )
        .into_iter()
        .filter(|t| !self.dependency_filter || self.state.pm.dependencies_satisfied(t.id))
        .collect()
    }

    fn project_labels(&self) -> Vec<String> {
        self.state
            .pm
            .tickets
            .iter()
            .flat_map(|t| t.labels.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn reset_ticket_filters(&mut self, cx: &mut Context<Self>) {
        self.ticket_filter = active_statuses();
        self.ticket_label_filter.clear();
        self.dependency_filter = false;
        self.ticket_search.update(cx, |ti, cx| ti.reset(cx));
        self.ticket_list_shown_count = None;
        cx.notify();
    }

    fn ticket_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sort = ConfigStore::get(cx).ticket_sort;
        let rows = TicketSort::ALL
            .into_iter()
            .map(|s| {
                let act: MenuAct = Box::new(move |pm, cx| {
                    ConfigStore::update(cx, |c| c.ticket_sort = s);
                    pm.sort_menu_open = false;
                    cx.notify();
                });
                (SharedString::from(s.label()), 0x6ca4dc, sort == s, act)
            })
            .collect();
        let statuses = Status::ALL
            .into_iter()
            .filter(|s| self.ticket_filter.contains(s))
            .map(|s| s.label())
            .collect::<Vec<_>>()
            .join(", ");
        let status_text = if self.ticket_filter == active_statuses() {
            "Active work (closed hidden)".to_string()
        } else if self.ticket_filter.len() == Status::ALL.len() {
            "All statuses".to_string()
        } else if statuses.is_empty() {
            "No statuses selected".to_string()
        } else {
            format!("Status: {statuses}")
        };
        let mut controls = div()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_1()
            .text_size(px(11.0))
            .text_color(cx.theme().colors.dim)
            .child(
                div()
                    .relative()
                    .child(
                        div()
                            .id("ticket-sort")
                            .cursor_pointer()
                            .child(SharedString::from(format!(
                                "Sort: {} \u{25be}",
                                sort.label()
                            )))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|pm, _, _, cx| {
                                    pm.sort_menu_open = !pm.sort_menu_open;
                                    pm.filter_menu_open = false;
                                    pm.status_menu_open = false;
                                    pm.user_menu_open = false;
                                    cx.notify();
                                    cx.stop_propagation();
                                }),
                            ),
                    )
                    .when(self.sort_menu_open, |d| {
                        d.child(deferred(
                            div()
                                .absolute()
                                .top_full()
                                .left_0()
                                .child(self.menu_panel(cx, true, rows)),
                        ))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(SharedString::from(status_text))
                    .when(self.ticket_filter.len() != Status::ALL.len(), |d| {
                        d.child(
                            div()
                                .id("ticket-all-statuses")
                                .cursor_pointer()
                                .text_color(cx.theme().colors.changed)
                                .child("Show all statuses")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|pm, _, _, cx| {
                                        pm.ticket_filter = Status::ALL.into_iter().collect();
                                        cx.notify();
                                    }),
                                ),
                        )
                    }),
            );
        if !self.ticket_label_filter.is_empty() {
            let mut labels = div().flex().flex_wrap().gap_1().child("All labels:");
            for (i, label) in self
                .ticket_label_filter
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .enumerate()
            {
                labels = labels.child(
                    div()
                        .id(("clear-label-filter", i))
                        .max_w_full()
                        .overflow_hidden()
                        .cursor_pointer()
                        .text_color(cx.theme().colors.changed)
                        .child(SharedString::from(format!("{label} ×")))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |pm, _, _, cx| {
                                pm.ticket_label_filter.remove(&label);
                                cx.notify();
                            }),
                        ),
                );
            }
            controls = controls.child(labels);
        }
        if !self.ticket_search.read(cx).content().is_empty() {
            controls = controls.child(
                div()
                    .id("clear-ticket-query")
                    .cursor_pointer()
                    .child("Clear search ×")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.ticket_search.update(cx, |ti, cx| ti.reset(cx));
                            pm.ticket_list_shown_count = None;
                            cx.notify();
                        }),
                    ),
            );
        }
        controls = controls.child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    div()
                        .id("toggle-ticket-hierarchy")
                        .cursor_pointer()
                        .child(if self.ticket_hierarchy {
                            "View: Hierarchy ▾"
                        } else {
                            "View: Flat list ▾"
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|pm, _, _, cx| {
                                pm.ticket_hierarchy = !pm.ticket_hierarchy;
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .id("ticket-dependency-filter")
                        .cursor_pointer()
                        .text_color(if self.dependency_filter {
                            cx.theme().colors.changed
                        } else {
                            cx.theme().colors.dim
                        })
                        .child(if self.dependency_filter {
                            "✓ Dependencies satisfied"
                        } else {
                            "Only dependencies satisfied"
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|pm, _, _, cx| {
                                pm.dependency_filter = !pm.dependency_filter;
                                cx.notify();
                            }),
                        ),
                ),
        );
        controls.child(
            div()
                .id("reset-ticket-filters")
                .cursor_pointer()
                .child("Reset filters")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|pm, _, _, cx| pm.reset_ticket_filters(cx)),
                ),
        )
    }

    /// Called on every search-box keystroke (PM-75). Jumps the selection to
    /// the new first result when the list just narrowed; a widening edit
    /// (e.g. backspace) leaves whatever's selected alone.
    pub(crate) fn autoselect_on_narrow(&mut self, cx: &mut Context<Self>) {
        let shown_ids: Vec<u64> = self.shown_tickets(cx).iter().map(|t| t.id).collect();
        let narrowed = self
            .ticket_list_shown_count
            .is_some_and(|prev| shown_ids.len() < prev);
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
        let reveal_matches = !self.ticket_search.read(cx).content().trim().is_empty()
            || !self.ticket_label_filter.is_empty()
            || self.dependency_filter
            || self.ticket_filter != active_statuses();
        let rows = if self.ticket_hierarchy {
            pm_core::relations::hierarchy_rows(
                pm,
                &shown,
                ConfigStore::get(cx).ticket_sort,
                &self.collapsed_tickets,
                reveal_matches,
            )
        } else {
            shown
                .iter()
                .map(|t| pm_core::relations::HierarchyRow {
                    ticket: t,
                    depth: 0,
                    context: false,
                    has_children: false,
                })
                .collect()
        };
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
            .h(px(cx.theme().metrics.section_header_h))
            .px_2()
            .text_color(cx.theme().colors.dim)
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
                            .text_color(cx.theme().colors.text)
                            .hover(|s| s.bg(cx.theme().colors.border))
                            .child("+ New")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|pm, _, window, cx| {
                                    pm.composing = Some(Compose::NewTicket);
                                    pm.selected_ticket = None;
                                    pm.filter_menu_open = false;
                                    pm.status_menu_open = false;
                                    pm.sort_menu_open = false;
                                    pm.editing_ticket_labels = None;
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
        if rows.iter().any(|r| r.context) {
            list = list.child(
                div()
                    .p_2()
                    .text_size(px(11.0))
                    .text_color(cx.theme().colors.dim)
                    .child("Parent context is included; counts show matching tickets."),
            );
        }
        if shown.is_empty() {
            list = list.child(div().p_2().text_color(cx.theme().colors.dim).child(
                SharedString::from(
                    "No tickets match. Clear filters or search above to see more work.",
                ),
            ));
        }
        for row in rows {
            let t = row.ticket;
            let id = t.id;
            let selected = self.selected_ticket == Some(id);
            let closed = t.status.is_closed();
            let progress = pm.child_progress(id);
            let blockers = pm
                .blockers(id)
                .into_iter()
                .filter(|t| t.status != Status::Done)
                .map(|t| pm.display_id(t))
                .collect::<Vec<_>>();
            list = list.child(
                div()
                    .id(("ticket", id))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_2()
                    .pl(px(8.0 + row.depth.min(8) as f32 * 14.0))
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().colors.border)
                    .cursor_pointer()
                    .when(selected, |s| s.bg(cx.theme().colors.select))
                    .when(!selected, |s| s.hover(|s| s.bg(cx.theme().colors.panel)))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .text_size(px(11.0))
                            .text_color(cx.theme().colors.dim)
                            .when(row.has_children, |d| {
                                d.child(
                                    div()
                                        .id(("collapse-ticket", id))
                                        .cursor_pointer()
                                        .child(
                                            if !reveal_matches
                                                && self.collapsed_tickets.contains(&id)
                                            {
                                                "▸"
                                            } else {
                                                "▾"
                                            },
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |pm, _, _, cx| {
                                                if !reveal_matches
                                                    && !pm.collapsed_tickets.remove(&id)
                                                {
                                                    pm.collapsed_tickets.insert(id);
                                                }
                                                cx.stop_propagation();
                                                cx.notify();
                                            }),
                                        ),
                                )
                            })
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
                    .when(row.context, |d| {
                        d.child(
                            div()
                                .text_size(px(10.0))
                                .text_color(cx.theme().colors.dim)
                                .child("Parent context"),
                        )
                    })
                    .when(progress.total > 0, |d| {
                        d.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(cx.theme().colors.dim)
                                .child(SharedString::from(progress.label())),
                        )
                    })
                    .when(!blockers.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(rgb(Status::Blocked.color()))
                                .child(SharedString::from(format!(
                                    "Blocked by {}",
                                    blockers.join(", ")
                                ))),
                        )
                    })
                    .child(
                        div()
                            .text_color(if closed {
                                cx.theme().colors.dim
                            } else {
                                cx.theme().colors.text
                            })
                            .when(closed, |d| d.line_through())
                            .child(SharedString::from(t.title.clone())),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pm, _, _, cx| {
                            pm.navigate_ticket(id, cx);
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
            .bg(cx.theme().colors.panel)
            .border_r_1()
            .border_color(cx.theme().colors.border)
            .child(header)
            .child(self.ticket_controls(cx))
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
            .child(self.sidebar_resize_handle(cx))
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
                    .bg(cx.theme().colors.del_bg)
                    .text_color(cx.theme().colors.text)
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
                        .text_color(cx.theme().colors.dim)
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
                    .text_color(cx.theme().colors.text)
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
                            .bg(cx.theme().colors.select)
                            .text_color(cx.theme().colors.text)
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
                            .border_color(cx.theme().colors.border)
                            .text_color(cx.theme().colors.dim)
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
                    let _ = pm.state.set_ticket_status(tid, s, None);
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
                    .bg(cx.theme().colors.border)
                    .text_color(rgb(current.color()))
                    .text_size(px(11.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(cx.theme().colors.select))
                    .child(SharedString::from(format!("{}  \u{25be}", current.label())))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|pm, _, _, cx| {
                            pm.status_menu_open = !pm.status_menu_open;
                            pm.filter_menu_open = false;
                            pm.sort_menu_open = false;
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

    pub(crate) fn submit_ticket_label(&mut self, cx: &mut Context<Self>) {
        let Some(tid) = self
            .editing_ticket_labels
            .filter(|id| self.selected_ticket == Some(*id))
        else {
            return;
        };
        let label = self
            .ticket_label_input
            .read(cx)
            .content()
            .trim()
            .to_string();
        if !label.is_empty() && self.state.change_ticket_label(tid, label, true).is_ok() {
            self.ticket_label_input.update(cx, |ti, cx| ti.reset(cx));
        }
        cx.notify();
    }

    fn label_editor(&self, t: &Ticket, cx: &mut Context<Self>) -> impl IntoElement {
        let tid = t.id;
        let editing = self.editing_ticket_labels == Some(tid);
        let mut labels = div().flex().flex_wrap().items_center().gap_2();
        for (i, label) in t.labels.iter().enumerate() {
            let label = label.clone();
            labels = labels.child(
                div()
                    .id(("ticket-label", i))
                    .max_w_full()
                    .overflow_hidden()
                    .text_color(cx.theme().colors.changed)
                    .child(SharedString::from(if editing {
                        format!("{label} ×")
                    } else {
                        label.clone()
                    }))
                    .when(editing, |d| {
                        d.cursor_pointer().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |pm, _, _, cx| {
                                let _ = pm.state.change_ticket_label(tid, label.clone(), false);
                                cx.notify();
                            }),
                        )
                    }),
            );
        }
        labels = labels.child(
            div()
                .id("edit-ticket-labels")
                .cursor_pointer()
                .text_color(cx.theme().colors.dim)
                .child(if editing {
                    "Done editing labels"
                } else {
                    "Edit labels"
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pm, _, window, cx| {
                        pm.editing_ticket_labels = if editing { None } else { Some(tid) };
                        if !editing {
                            pm.ticket_label_input.update(cx, |ti, cx| {
                                ti.reset(cx);
                                ti.focus(window, cx);
                            });
                        }
                        cx.notify();
                    }),
                ),
        );
        let mut editor = div().flex().flex_col().gap_2().child(labels);
        if editing {
            editor = editor.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.ticket_label_input.clone()),
                    )
                    .child(
                        div()
                            .id("add-ticket-label")
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .bg(cx.theme().colors.select)
                            .child("Add")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|pm, _, _, cx| pm.submit_ticket_label(cx)),
                            ),
                    ),
            );
            let query = self
                .ticket_label_input
                .read(cx)
                .content()
                .trim()
                .to_lowercase();
            let mut suggestions = div()
                .flex()
                .flex_wrap()
                .gap_2()
                .text_size(px(11.0))
                .text_color(cx.theme().colors.dim)
                .child("Existing labels:");
            for (i, label) in self
                .project_labels()
                .into_iter()
                .filter(|s| {
                    s.to_lowercase().contains(&query)
                        && !t
                            .labels
                            .iter()
                            .any(|l| l.trim().to_lowercase() == s.trim().to_lowercase())
                })
                .take(12)
                .enumerate()
            {
                suggestions = suggestions.child(
                    div()
                        .id(("suggest-label", i))
                        .max_w_full()
                        .overflow_hidden()
                        .cursor_pointer()
                        .text_color(cx.theme().colors.changed)
                        .child(SharedString::from(label.clone()))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |pm, _, _, cx| {
                                if pm
                                    .state
                                    .change_ticket_label(tid, label.clone(), true)
                                    .is_ok()
                                {
                                    pm.ticket_label_input.update(cx, |ti, cx| ti.reset(cx));
                                }
                                cx.notify();
                            }),
                        ),
                );
            }
            editor = editor.child(suggestions);
        }
        editor
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
                    .text_color(cx.theme().colors.dim)
                    .child(SharedString::from(pm.display_id(t))),
            )
            .child(self.status_button(tid, t.status, cx))
            .child(chip(
                t.priority.label(),
                rgb(t.priority.color()).into(),
                cx.theme().colors.border,
            ));
        if let Some(a) = &t.assignee {
            meta = meta.child(
                div()
                    .text_color(cx.theme().colors.dim)
                    .child(SharedString::from(format!("@{a}"))),
            );
        }
        if !t.author.trim().is_empty() {
            meta = meta.child(
                div()
                    .text_color(cx.theme().colors.dim)
                    .text_size(px(11.0))
                    .child(SharedString::from(format!("by {}", t.author))),
            );
        }
        meta = meta.child(
            div()
                .text_color(cx.theme().colors.dim)
                .text_size(px(11.0))
                .child(SharedString::from(format!(
                    "updated {}",
                    rel_time(t.updated)
                ))),
        );

        let mut card = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(17.0))
                    .text_color(cx.theme().colors.text)
                    .child(SharedString::from(t.title.clone())),
            )
            .child(meta)
            .child(self.label_editor(t, cx))
            .child(self.ticket_relations(t, cx));

        if !self.shown_tickets(cx).iter().any(|shown| shown.id == tid) {
            card = card.child(
                div()
                    .text_color(cx.theme().colors.dim)
                    .child("This ticket is outside the current list filters."),
            );
        }

        if !t.body.trim().is_empty() {
            card = card.child(crate::markdown::view(t.body.clone(), cx));
        }

        // Code anchors — read-only for now; click opens the file in File-to-File.
        for (i, a) in t.anchors.iter().enumerate() {
            let file = a.file.clone();
            card = card.child(
                div()
                    .id(("anchor", i))
                    .text_color(cx.theme().colors.changed)
                    .cursor_pointer()
                    .hover(|s| s.bg(cx.theme().colors.panel))
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
                    .text_color(cx.theme().colors.dim)
                    .text_size(px(11.0))
                    .child(SharedString::from(format!(
                        "History  ({})",
                        t.history.len()
                    ))),
            );
            for h in &t.history {
                card = card.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_size(px(11.0))
                        .text_color(cx.theme().colors.dim)
                        .child(SharedString::from(rel_time(h.at)))
                        .child(SharedString::from(history_line(h))),
                );
            }
        }

        // Comments.
        card = card.child(
            div()
                .mt_2()
                .text_color(cx.theme().colors.dim)
                .text_size(px(11.0))
                .child(SharedString::from(format!(
                    "Comments  ({})",
                    t.comments.len()
                ))),
        );
        for c in &t.comments {
            card = card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().colors.border)
                    .child(
                        div()
                            .text_color(cx.theme().colors.dim)
                            .text_size(px(11.0))
                            .child(SharedString::from(format!(
                                "{}  \u{00b7}  {}",
                                if c.author.is_empty() {
                                    "someone"
                                } else {
                                    &c.author
                                },
                                rel_time(c.created)
                            ))),
                    )
                    .child(crate::markdown::view(c.body.clone(), cx)),
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
                        .bg(cx.theme().colors.select)
                        .text_color(cx.theme().colors.text)
                        .cursor_pointer()
                        .child("Comment")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |pm, _, _, cx| {
                                let body = pm.comment_box.read(cx).content().to_string();
                                if !body.is_empty() {
                                    if pm.state.add_comment(tid, body, None).is_ok() {
                                        pm.comment_box
                                            .update(cx, |ti, cx| ti.set_text(String::new(), cx));
                                    }
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
        let title = self.new_ticket_title.read(cx).content().to_string();
        if title.is_empty() {
            return;
        }
        let body = self.new_ticket_body.read(cx).content().to_string();
        if let Ok(id) = self.state.create_ticket(title, body, None) {
            self.new_ticket_title
                .update(cx, |ti, cx| ti.set_text(String::new(), cx));
            self.new_ticket_body
                .update(cx, |ti, cx| ti.set_text(String::new(), cx));
            self.selected_ticket = Some(id);
            self.composing = None;
        }
        cx.notify();
    }
}
