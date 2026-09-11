//! Ticket relation navigation and editing, shared by the detail and hierarchy views.
use gpui::{div, prelude::*, px, relative, rgb, Context, MouseButton, SharedString};
use pm_core::{relations::LinkKind, Status, Ticket};

use crate::{app::Pm, theme::*};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RelationMode {
    Parent,
    Child,
    BlockedBy,
    Blocks,
    Related,
    Closes,
    Duplicate,
}

impl RelationMode {
    const ALL: [Self; 7] = [
        Self::Parent,
        Self::Child,
        Self::BlockedBy,
        Self::Blocks,
        Self::Related,
        Self::Closes,
        Self::Duplicate,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Parent => "Parent",
            Self::Child => "Child",
            Self::BlockedBy => "Blocked by",
            Self::Blocks => "Blocks",
            Self::Related => "Related to",
            Self::Closes => "Closes",
            Self::Duplicate => "Duplicate of",
        }
    }
}

impl Pm {
    pub(crate) fn navigate_ticket(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.state.pm.ticket(id).is_none() {
            return;
        }
        self.selected_ticket = Some(id);
        self.composing = None;
        self.relation_editor = None;
        self.editing_ticket_labels = None;
        self.status_menu_open = false;
        self.sort_menu_open = false;
        self.comment_box.update(cx, |ti, cx| ti.reset(cx));
        let mut current = id;
        let mut seen = std::collections::HashSet::new();
        while let Some(parent) = self.state.pm.parent_id(current) {
            if !seen.insert(parent) {
                break;
            }
            self.collapsed_tickets.remove(&parent);
            current = parent;
        }
        cx.notify();
    }

    fn choose_relation(
        &mut self,
        source: u64,
        target: u64,
        mode: RelationMode,
        cx: &mut Context<Self>,
    ) {
        let result = match mode {
            RelationMode::Parent => self.state.set_ticket_parent(source, Some(target)),
            RelationMode::Child => {
                self.state
                    .change_ticket_link(source, target, LinkKind::ParentOf, false)
            }
            RelationMode::BlockedBy => {
                self.state
                    .change_ticket_link(target, source, LinkKind::Blocks, false)
            }
            RelationMode::Blocks => {
                self.state
                    .change_ticket_link(source, target, LinkKind::Blocks, false)
            }
            RelationMode::Related => {
                self.state
                    .change_ticket_link(source, target, LinkKind::Relates, false)
            }
            RelationMode::Closes => {
                self.state
                    .change_ticket_link(source, target, LinkKind::Closes, false)
            }
            RelationMode::Duplicate => {
                self.state
                    .change_ticket_link(source, target, LinkKind::DuplicateOf, false)
            }
        };
        if result.is_ok() {
            self.relation_editor = None;
            self.collapsed_tickets.remove(&source);
            self.collapsed_tickets.remove(&target);
        }
        cx.notify();
    }

    pub(crate) fn ticket_relations(
        &self,
        ticket: &Ticket,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tid = ticket.id;
        let data = &self.state.pm;
        let progress = data.child_progress(tid);
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(cx.theme().colors.border)
            .rounded_md()
            .child(
                div()
                    .text_color(cx.theme().colors.dim)
                    .child("Links and hierarchy"),
            );

        if progress.total > 0 {
            panel = panel
                .child(
                    div()
                        .text_color(cx.theme().colors.text)
                        .child(SharedString::from(progress.label())),
                )
                .child(
                    div()
                        .h(px(4.0))
                        .w_full()
                        .flex()
                        .bg(cx.theme().colors.border)
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress.done as f32 / progress.total as f32))
                                .bg(rgb(Status::Done.color())),
                        )
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress.cancelled as f32 / progress.total as f32))
                                .bg(cx.theme().colors.dim),
                        ),
                );
        }
        let blockers = data.blockers(tid);
        if !blockers.is_empty() {
            let outstanding = blockers.iter().filter(|t| t.status != Status::Done).count();
            let dependency_color = if outstanding == 0 {
                rgb(Status::Done.color())
            } else {
                rgb(Status::Blocked.color())
            };
            panel = panel.child(div().text_color(dependency_color).child(SharedString::from(
                if outstanding == 0 {
                    "Dependencies satisfied".into()
                } else {
                    format!("{outstanding} unresolved blocker(s)")
                },
            )));
        }

        // Canonical edges plus inverses. The row carries the canonical endpoints
        // so Remove works identically from either side.
        let mut edges = Vec::new();
        for source in &data.tickets {
            for link in &source.links {
                if source.id == tid {
                    edges.push((
                        source.id,
                        link.target,
                        link.kind,
                        link.target,
                        link.kind.label(),
                    ));
                } else if link.target == tid {
                    edges.push((
                        source.id,
                        link.target,
                        link.kind,
                        source.id,
                        link.kind.inverse_label(),
                    ));
                }
            }
        }
        edges.sort_by_key(|(_, _, _, other, label)| (*label, *other));
        if edges.is_empty() {
            panel = panel.child(
                div()
                    .text_color(cx.theme().colors.dim)
                    .child("No linked tickets yet."),
            );
        }
        for (i, (source, target, kind, other, label)) in edges.into_iter().enumerate() {
            let linked = data.ticket(other);
            let title = linked
                .map(|t| format!("{} · {}", data.display_id(t), t.title))
                .unwrap_or_else(|| format!("Missing ticket #{other}"));
            let mut row = div()
                .flex()
                .items_start()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .w(px(88.0))
                        .flex_none()
                        .text_size(px(11.0))
                        .text_color(cx.theme().colors.dim)
                        .child(label),
                )
                .child(
                    div()
                        .id(("relation-target", i))
                        .flex_1()
                        .min_w_0()
                        .text_color(cx.theme().colors.changed)
                        .child(SharedString::from(title))
                        .when(linked.is_some(), |d| {
                            d.cursor_pointer().on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |pm, _, _, cx| pm.navigate_ticket(other, cx)),
                            )
                        }),
                );
            if let Some(t) = linked {
                row = row.child(
                    div()
                        .flex_none()
                        .text_size(px(11.0))
                        .text_color(rgb(t.status.color()))
                        .child(t.status.label()),
                );
            }
            row = row.child(
                div()
                    .id(("remove-relation", i))
                    .flex_none()
                    .cursor_pointer()
                    .text_size(px(11.0))
                    .text_color(cx.theme().colors.dim)
                    .child("Remove")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pm, _, _, cx| {
                            let _ = pm.state.change_ticket_link(source, target, kind, true);
                            cx.notify();
                        }),
                    ),
            );
            panel = panel.child(row);
        }

        let mut actions = div().flex().flex_wrap().gap_2();
        for (i, (text, mode)) in [
            ("Set parent", RelationMode::Parent),
            ("Add child", RelationMode::Child),
            ("Add link", RelationMode::Related),
        ]
        .into_iter()
        .enumerate()
        {
            actions = actions.child(
                div()
                    .id(("add-relation", i))
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(cx.theme().colors.border)
                    .child(text)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pm, _, window, cx| {
                            pm.relation_editor = Some((tid, mode));
                            pm.relation_query.update(cx, |ti, cx| {
                                ti.reset(cx);
                                ti.focus(window, cx);
                            });
                            cx.notify();
                        }),
                    ),
            );
        }
        panel = panel.child(actions);
        if let Some((source, mode)) = self.relation_editor.filter(|(id, _)| *id == tid) {
            let mut modes = div().flex().flex_wrap().gap_1();
            for (i, option) in RelationMode::ALL.into_iter().enumerate() {
                modes = modes.child(
                    div()
                        .id(("relation-mode", i))
                        .cursor_pointer()
                        .px_1()
                        .when(mode == option, |d| d.bg(cx.theme().colors.select))
                        .child(option.label())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |pm, _, _, cx| {
                                pm.relation_editor = Some((source, option));
                                cx.notify();
                            }),
                        ),
                );
            }
            let query = self.relation_query.read(cx).content().trim().to_lowercase();
            let candidates: Vec<_> = data
                .tickets
                .iter()
                .filter(|t| {
                    t.id != source
                        && (query.is_empty()
                            || data.display_id(t).to_lowercase().contains(&query)
                            || t.title.to_lowercase().contains(&query))
                })
                .collect();
            let mut choices = div()
                .id("relation-candidates")
                .max_h(px(210.0))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1();
            if candidates.is_empty() {
                choices = choices.child("No tickets match.");
            }
            for t in candidates {
                let target = t.id;
                choices = choices.child(
                    div()
                        .id(("relation-candidate", target))
                        .cursor_pointer()
                        .p_1()
                        .hover(|s| s.bg(cx.theme().colors.select))
                        .child(SharedString::from(format!(
                            "{} · {} · {}",
                            data.display_id(t),
                            t.title,
                            t.status.label()
                        )))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |pm, _, _, cx| {
                                pm.choose_relation(source, target, mode, cx)
                            }),
                        ),
                );
            }
            panel = panel
                .child(modes)
                .child(self.relation_query.clone())
                .child(choices)
                .child(
                    div()
                        .id("cancel-relation")
                        .cursor_pointer()
                        .text_color(cx.theme().colors.dim)
                        .child("Cancel linking")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|pm, _, _, cx| {
                                pm.relation_editor = None;
                                cx.notify();
                            }),
                        ),
                );
        }
        panel
    }
}
