//! The custom window title bar: menu strip, centered title, window controls,
//! and drag-to-move — modelled on Zed's `platform_title_bar` / `title_bar`.

use gpui::{
    deferred, div, prelude::*, px, rgb, ClickEvent, Context, Decorations, MouseButton, SharedString,
    WindowControlArea,
};

use crate::app::Pm;
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
            .h(px(TITLE_BAR_H))
            .bg(rgb(PANEL))
            .border_b_1()
            .border_color(rgb(BORDER))
            .text_color(rgb(TEXT))
            .window_control_area(WindowControlArea::Drag)
            .map(|bar| match decorations {
                Decorations::Client { tiling } => bar
                    .when(!tiling.top && !tiling.left, |b| {
                        b.rounded_tl(px(CLIENT_DECORATION_ROUNDING))
                    })
                    .when(!tiling.top && !tiling.right, |b| {
                        b.rounded_tr(px(CLIENT_DECORATION_ROUNDING))
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
            // centered title — first child, no hitbox, painted under the clusters
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(DIM))
                    .child(SharedString::from(title)),
            )
            .child(self.menu_strip(cx))
            .child(div().flex_1())
            .child(self.window_controls(window, cx))
    }

    fn menu_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = menu::menu_groups(self);
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
                .hover(|s| s.bg(rgb(BORDER)))
                .when(open, |b| b.bg(rgb(BORDER)))
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
            .min_w(px(210.0))
            .py_1()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded_md()
            .shadow_lg()
            .text_color(rgb(TEXT));

        for (j, entry) in entries.into_iter().enumerate() {
            match entry {
                Entry::Separator => {
                    panel = panel.child(div().h(px(1.0)).my_1().bg(rgb(BORDER)));
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
                        .hover(|s| s.bg(rgb(SELECT)))
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
                                        .w(px(12.0))
                                        .text_color(rgb(CHANGED))
                                        .child(if checked { "\u{2713}" } else { "" }),
                                )
                                .child(SharedString::from(label)),
                        )
                        .when_some(shortcut, |r, sc| {
                            r.child(div().text_color(rgb(DIM)).child(SharedString::from(sc)))
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
            .flex_none()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        #[cfg(target_os = "windows")]
        {
            let button = |glyph: &'static str, area: WindowControlArea, close: bool| {
                div()
                    .w(px(46.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family("Segoe Fluent Icons")
                    .text_size(px(10.0))
                    .window_control_area(area)
                    .hover(|s| s.bg(rgb(if close { CLOSE_HOVER } else { BORDER })))
                    .child(glyph)
            };
            row.child(button("\u{e921}", WindowControlArea::Min, false))
                .child(button(
                    if maximized { "\u{e923}" } else { "\u{e922}" },
                    WindowControlArea::Max,
                    false,
                ))
                .child(button("\u{e8bb}", WindowControlArea::Close, true))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = maximized;
            let mk = |id: &'static str, glyph: &'static str, close: bool| {
                div()
                    .id(id)
                    .w(px(40.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(if close { CLOSE_HOVER } else { BORDER })))
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
