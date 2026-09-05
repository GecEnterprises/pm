//! The custom window title bar: menu strip, centered title, window controls,
//! and drag-to-move — modelled on Zed's `platform_title_bar` / `title_bar`.

use gpui::{
    deferred, div, prelude::*, px, svg, ClickEvent, Context, Decorations, MouseButton,
    SharedString, WindowControlArea,
};

use crate::app::{Pm, View};
use crate::icons;
use crate::menu::{self, Entry};
use crate::theme::*;

impl Pm {
    pub(crate) fn title_bar(&self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let decorations = window.window_decorations();
        let title = self.window_title();

        div()
            .id("title-bar")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .flex_none()
            .h(cx.theme().rm(cx.theme().metrics.title_bar_h))
            .bg(cx.theme().colors.panel)
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .text_color(cx.theme().colors.text)
            .window_control_area(WindowControlArea::Drag)
            .map(|bar| match decorations {
                Decorations::Client { tiling } => bar
                    .when(!tiling.top && !tiling.left, |b| {
                        b.rounded_tl(px(cx.theme().metrics.client_decoration_rounding))
                    })
                    .when(!tiling.top && !tiling.right, |b| {
                        b.rounded_tr(px(cx.theme().metrics.client_decoration_rounding))
                    }),
                Decorations::Server => bar,
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pm, _, _, _| pm.window_drag_armed = true),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|pm, _, _, _| pm.window_drag_armed = false),
            )
            .on_mouse_down_out(cx.listener(|pm, _, _, _| pm.window_drag_armed = false))
            .on_mouse_move(cx.listener(|pm, _, window, _| {
                if pm.window_drag_armed {
                    pm.window_drag_armed = false;
                    window.start_window_move();
                }
            }))
            .on_click(cx.listener(|_, e: &ClickEvent, window, _| {
                if e.click_count() == 2 {
                    window.zoom_window();
                }
            }))
            // centered title — first child, no hitbox, painted under the clusters.
            // Capped + ellipsised so a long "ticket — repo — pm" doesn't run
            // under the menu strip / window controls (PM-49).
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .max_w(cx.theme().rm(380.0))
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(cx.theme().colors.dim)
                            .child(SharedString::from(title)),
                    ),
            )
            .child(self.menu_strip(cx))
            .child(self.view_switcher(cx))
            .child(div().flex_1())
            .child(self.window_controls(window, cx))
    }

    /// The `Summary | File-to-File | Tickets` segmented switcher, next to the
    /// menu strip.
    fn view_switcher(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let segs = [
            ("Summary", "project.svg", View::Summary),
            ("File-to-File", "diff.svg", View::Files),
            ("Tickets", "hash.svg", View::Tickets),
        ];
        // No project open → the switcher is inert (PM-5).
        let disabled = self.empty;
        let mut strip = div()
            .id("view-switcher")
            .flex()
            .flex_row()
            .items_center()
            .mx_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors.border)
            .bg(cx.theme().colors.panel)
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        for (label, icon, view) in segs {
            let active = !disabled && self.view == view;
            let fg = if active { cx.theme().colors.text } else { cx.theme().colors.dim };
            strip = strip.child(
                div()
                    .id(label)
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py(px(2.0))
                    .text_color(fg)
                    .when(active, |s| s.bg(cx.theme().colors.select))
                    .when(!disabled, |s| s.cursor_pointer())
                    .when(!active && !disabled, |s| s.hover(|s| s.bg(cx.theme().colors.border)))
                    .child(
                        svg()
                            .size(cx.theme().rm(13.0))
                            .flex_none()
                            .text_color(fg)
                            .data(icons::svg_bytes(icon)),
                    )
                    .child(SharedString::from(label))
                    .when(!disabled, |s| {
                        s.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |pm, _, _, cx| {
                                pm.set_view(view, cx);
                                cx.stop_propagation();
                            }),
                        )
                    }),
            );
        }
        strip
    }

    fn menu_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = menu::menu_groups(self, cx);
        let mut strip = div()
            .id("menu-strip")
            .flex()
            .flex_row()
            .items_center()
            .h_full()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        for (i, group) in groups.into_iter().enumerate() {
            let open = self.open_menu == Some(i);
            let mut button = div()
                .id(("menu", i))
                .relative()
                .flex()
                .items_center()
                .h_full()
                .px_2()
                .cursor_pointer()
                .hover(|s| s.bg(cx.theme().colors.border))
                .when(open, |b| b.bg(cx.theme().colors.border))
                .child(SharedString::from(group.name))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |pm, _, _, cx| {
                        pm.open_menu = if pm.open_menu == Some(i) { None } else { Some(i) };
                        cx.notify();
                        cx.stop_propagation();
                    }),
                )
                .on_hover(cx.listener(move |pm, hovered: &bool, _, cx| {
                    if *hovered && pm.open_menu.is_some() && pm.open_menu != Some(i) {
                        pm.open_menu = Some(i);
                        cx.notify();
                    }
                }));

            if open {
                button = button.child(deferred(
                    div()
                        .absolute()
                        .top_full()
                        .left_0()
                        .mt(px(2.0))
                        .child(self.dropdown(i, group.entries, cx)),
                ));
            }
            strip = strip.child(button);
        }
        strip
    }

    fn dropdown(
        &self,
        menu_ix: usize,
        entries: Vec<Entry>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut panel = div()
            .occlude()
            .flex()
            .flex_col()
            .min_w(cx.theme().rm(210.0))
            .py_1()
            .bg(cx.theme().colors.panel)
            .border_1()
            .border_color(cx.theme().colors.border)
            .rounded_md()
            .shadow_lg()
            .text_color(cx.theme().colors.text);

        for (j, entry) in entries.into_iter().enumerate() {
            match entry {
                Entry::Separator => {
                    panel = panel.child(div().h(px(1.0)).my_1().bg(cx.theme().colors.border));
                }
                Entry::Header(label) => {
                    panel = panel.child(
                        div()
                            .px_3()
                            .py_1()
                            .text_size(cx.theme().rm(11.0))
                            .text_color(cx.theme().colors.dim)
                            .child(label),
                    );
                }
                Entry::Run { label, run } => {
                    panel = panel.child(
                        div()
                            .id(("menu-run", menu_ix * 64 + j))
                            .flex()
                            .flex_row()
                            .items_center()
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .hover(|s| s.bg(cx.theme().colors.select))
                            .child(label)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |pm, _, window, cx| {
                                    pm.open_menu = None;
                                    run(pm, window, cx);
                                    cx.notify();
                                    cx.stop_propagation();
                                }),
                            ),
                    );
                }
                Entry::Item {
                    label,
                    shortcut,
                    action,
                    checked,
                } => {
                    let row = div()
                        .id(("menu-item", menu_ix * 64 + j))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .px_3()
                        .py_1()
                        .cursor_pointer()
                        .hover(|s| s.bg(cx.theme().colors.select))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |pm, _, window, cx| {
                                pm.open_menu = None;
                                window.dispatch_action(action.boxed_clone(), cx);
                                cx.notify();
                                cx.stop_propagation();
                            }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .w(cx.theme().rm(12.0))
                                        .text_color(cx.theme().colors.changed)
                                        .child(if checked { "\u{2713}" } else { "" }),
                                )
                                .child(SharedString::from(label)),
                        )
                        .when_some(shortcut, |r, sc| {
                            r.child(div().text_color(cx.theme().colors.dim).child(SharedString::from(sc)))
                        });
                    panel = panel.child(row);
                }
            }
        }
        panel
    }

    fn window_controls(&self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = cx;
        let maximized = window.is_maximized();
        let row = div()
            .id("window-controls")
            .flex()
            .flex_row()
            .items_center()
            .h_full()
            .flex_none();

        #[cfg(target_os = "windows")]
        {
            // The Win32 backend turns these into native caption buttons via
            // WM_NCHITTEST (Close/Min/Max + Snap Layouts), so they carry a
            // `window_control_area` and an occluding hitbox but no `on_click` —
            // the OS performs the action. `.occlude()` is what makes the hitbox
            // register; without it clicks fall through to the drag region.
            let button = |id: &'static str, glyph: &'static str, area: WindowControlArea, close: bool| {
                div()
                    .id(id)
                    .occlude()
                    .w(cx.theme().rm(46.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family("Segoe Fluent Icons")
                    .text_size(cx.theme().rm(10.0))
                    .window_control_area(area)
                    .hover(|s| {
                        s.bg(if close { cx.theme().colors.close_hover } else { cx.theme().colors.border })
                            .text_color(cx.theme().colors.text)
                    })
                    .child(glyph)
            };
            row.child(button("min", "\u{e921}", WindowControlArea::Min, false))
                .child(button(
                    "max",
                    if maximized { "\u{e923}" } else { "\u{e922}" },
                    WindowControlArea::Max,
                    false,
                ))
                .child(button("close", "\u{e8bb}", WindowControlArea::Close, true))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = maximized;
            let mk = |id: &'static str, glyph: &'static str, close: bool| {
                div()
                    .id(id)
                    .occlude()
                    .w(cx.theme().rm(40.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(if close { cx.theme().colors.close_hover } else { cx.theme().colors.border }))
                    .child(glyph)
            };
            row.child(
                mk("win-min", "\u{2013}", false)
                    .on_click(cx.listener(|_, _, window, _| window.minimize_window())),
            )
            .child(
                mk("win-max", "\u{25a1}", false)
                    .on_click(cx.listener(|_, _, window, _| window.zoom_window())),
            )
            .child(
                mk("win-close", "\u{2715}", true)
                    .on_click(cx.listener(|_, _, window, _| window.remove_window())),
            )
        }
    }
}
