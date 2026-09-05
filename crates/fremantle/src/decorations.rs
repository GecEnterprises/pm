//! Client-side window decorations for Linux (rounded corners, drop shadow, a 1px
//! border, and edge/corner resize grips). Ported from Zed's
//! `workspace::client_side_decorations`. On Windows/macOS the compositor draws
//! the frame (`Decorations::Server`) and this wrapper is a passthrough.

use gpui::{
    canvas, div, point, prelude::*, px, size, App, Bounds, BoxShadow, CursorStyle, Decorations, Div,
    Global, Hitbox, HitboxBehavior, Hsla, MouseButton, Pixels, ResizeEdge, Stateful, Styled, Tiling,
    Window,
};

/// Colors/metrics for [`client_side_decorations`] — the caller supplies these
/// from its own theme rather than fremantle owning any color values.
#[derive(Clone, Copy)]
pub struct DecorationStyle {
    pub rounding: f32,
    pub shadow: f32,
    pub bg: Hsla,
    pub border: Hsla,
}

/// Round the corners that aren't tiled flush against a screen edge.
fn round_corners<T: Styled + IntoElement>(el: T, tiling: Tiling, rounding: f32) -> T {
    el.when(!tiling.top && !tiling.left, |s| s.rounded_tl(px(rounding)))
        .when(!tiling.top && !tiling.right, |s| s.rounded_tr(px(rounding)))
        .when(!tiling.bottom && !tiling.left, |s| s.rounded_bl(px(rounding)))
        .when(!tiling.bottom && !tiling.right, |s| s.rounded_br(px(rounding)))
}

/// Wrap the window's root element with client-side decorations when the platform
/// asks the app to draw its own frame.
pub fn client_side_decorations(
    element: impl gpui::IntoElement,
    window: &mut Window,
    _cx: &mut App,
    style: DecorationStyle,
) -> Stateful<Div> {
    let DecorationStyle { rounding, shadow, bg, border } = style;
    const BORDER_SIZE: Pixels = px(1.0);
    let decorations = window.window_decorations();
    let is_resizable = window.is_resizable();
    let tiling = match decorations {
        Decorations::Server => Tiling::default(),
        Decorations::Client { tiling } => tiling,
    };

    match decorations {
        Decorations::Client { .. } => window.set_client_inset(px(shadow)),
        Decorations::Server => window.set_client_inset(px(0.0)),
    }

    struct GlobalResizeEdge(ResizeEdge);
    impl Global for GlobalResizeEdge {}

    div()
        .id("window-backdrop")
        .bg(gpui::transparent_black())
        .map(|div| match decorations {
            Decorations::Server => div,
            Decorations::Client { .. } => div
                .map(|d| round_corners(d, tiling, rounding))
                .when(!tiling.top, |div| div.pt(px(shadow)))
                .when(!tiling.bottom, |div| div.pb(px(shadow)))
                .when(!tiling.left, |div| div.pl(px(shadow)))
                .when(!tiling.right, |div| div.pr(px(shadow)))
                .when(is_resizable, |div| {
                    div.on_mouse_move(move |e, window, cx| {
                        let size = window.window_bounds().get_bounds().size;
                        let edge = resize_edge(e.position, px(shadow), size, tiling);
                        let prev = cx.try_global::<GlobalResizeEdge>().map(|g| g.0);
                        if edge != prev {
                            window.refresh();
                        }
                    })
                    .on_mouse_down(MouseButton::Left, move |e, window, _| {
                        let size = window.window_bounds().get_bounds().size;
                        if let Some(edge) = resize_edge(e.position, px(shadow), size, tiling) {
                            window.start_window_resize(edge);
                        }
                    })
                }),
        })
        .size_full()
        .child(
            div()
                .cursor(CursorStyle::Arrow)
                .map(|div| match decorations {
                    Decorations::Server => div,
                    Decorations::Client { .. } => div
                        .border_color(border)
                        .map(|d| round_corners(d, tiling, rounding))
                        .when(!tiling.top, |div| div.border_t(BORDER_SIZE))
                        .when(!tiling.bottom, |div| div.border_b(BORDER_SIZE))
                        .when(!tiling.left, |div| div.border_l(BORDER_SIZE))
                        .when(!tiling.right, |div| div.border_r(BORDER_SIZE))
                        .when(!tiling.is_tiled(), |div| {
                            div.shadow(vec![BoxShadow::new(
                                px(0.),
                                px(0.),
                                Hsla { h: 0., s: 0., l: 0., a: 0.4 },
                            )
                            .blur_radius(px(shadow / 2.))])
                        }),
                })
                .on_mouse_move(|_e, _, cx| cx.stop_propagation())
                .size_full()
                .bg(bg)
                .child(element),
        )
        .map(|div| match decorations {
            Decorations::Client { .. } if is_resizable => div.child(
                canvas(
                    |_bounds, window, _| {
                        window.insert_hitbox(
                            Bounds::new(point(px(0.0), px(0.0)), window.window_bounds().get_bounds().size),
                            HitboxBehavior::Normal,
                        )
                    },
                    move |_bounds, hitbox: Hitbox, window, cx| {
                        let size = window.window_bounds().get_bounds().size;
                        let Some(edge) = resize_edge(window.mouse_position(), px(shadow), size, tiling)
                        else {
                            return;
                        };
                        cx.set_global(GlobalResizeEdge(edge));
                        window.set_cursor_style(
                            match edge {
                                ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
                                ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
                                ResizeEdge::TopLeft | ResizeEdge::BottomRight => {
                                    CursorStyle::ResizeUpLeftDownRight
                                }
                                ResizeEdge::TopRight | ResizeEdge::BottomLeft => {
                                    CursorStyle::ResizeUpRightDownLeft
                                }
                            },
                            &hitbox,
                        );
                    },
                )
                .size_full()
                .absolute(),
            ),
            _ => div,
        })
}

/// Which window edge/corner `pos` is over, within the `shadow`-wide resize grip.
fn resize_edge(
    pos: gpui::Point<Pixels>,
    shadow: Pixels,
    window_size: gpui::Size<Pixels>,
    tiling: Tiling,
) -> Option<ResizeEdge> {
    let inner = Bounds::new(gpui::Point::default(), window_size).inset(shadow * 1.5);
    if inner.contains(&pos) {
        return None;
    }

    let corner = size(shadow * 1.5, shadow * 1.5);
    let tl = Bounds::new(point(px(0.), px(0.)), corner);
    if !tiling.top && tl.contains(&pos) {
        return Some(ResizeEdge::TopLeft);
    }
    let tr = Bounds::new(point(window_size.width - corner.width, px(0.)), corner);
    if !tiling.top && tr.contains(&pos) {
        return Some(ResizeEdge::TopRight);
    }
    let bl = Bounds::new(point(px(0.), window_size.height - corner.height), corner);
    if !tiling.bottom && bl.contains(&pos) {
        return Some(ResizeEdge::BottomLeft);
    }
    let br = Bounds::new(
        point(window_size.width - corner.width, window_size.height - corner.height),
        corner,
    );
    if !tiling.bottom && br.contains(&pos) {
        return Some(ResizeEdge::BottomRight);
    }

    if !tiling.top && pos.y < shadow {
        Some(ResizeEdge::Top)
    } else if !tiling.bottom && pos.y > window_size.height - shadow {
        Some(ResizeEdge::Bottom)
    } else if !tiling.left && pos.x < shadow {
        Some(ResizeEdge::Left)
    } else if !tiling.right && pos.x > window_size.width - shadow {
        Some(ResizeEdge::Right)
    } else {
        None
    }
}
