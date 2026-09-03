//! Side-by-side image diff. Both panels share one zoom/pan gesture so HEAD and
//! working stay aligned. Built on plain `div` + `img` (gpui decodes and caches
//! by content hash), inside the same split scaffolding as the text diff.
//!
//! Interaction: scroll to zoom, drag to pan, double-click to reset. Pan is
//! clamped so the image always covers the panel once zoomed in.

use std::sync::Arc;

use gpui::{
    canvas, div, img, prelude::*, px, relative, rgb, Context, Image, ImageFormat, MouseButton,
    MouseMoveEvent, ObjectFit, ScrollDelta, ScrollWheelEvent, SharedString,
};

use pm_core::content::ImageKind;
use pm_core::state::Content;

use crate::app::{DragPreview, Pm, ResizeHandle};
use crate::theme::*;

/// Shared view transform for the image diff. `zoom` 1.0 fits the panel; `pan` is
/// a pixel offset from centre, applied to both sides.
#[derive(Clone, Copy)]
pub struct ImageView {
    pub zoom: f32,
    pub pan: (f32, f32),
}

impl Default for ImageView {
    fn default() -> Self {
        Self { zoom: 1.0, pan: (0.0, 0.0) }
    }
}

impl ImageView {
    /// Keep the (scaled) image covering `panel`, so it can't be lost off-screen.
    fn clamp_pan(&mut self, panel_w: f32, panel_h: f32) {
        let max_x = ((panel_w * (self.zoom - 1.0)) / 2.0).max(0.0);
        let max_y = ((panel_h * (self.zoom - 1.0)) / 2.0).max(0.0);
        self.pan.0 = self.pan.0.clamp(-max_x, max_x);
        self.pan.1 = self.pan.1.clamp(-max_y, max_y);
    }
}

fn to_format(kind: ImageKind) -> ImageFormat {
    match kind {
        ImageKind::Png => ImageFormat::Png,
        ImageKind::Jpeg => ImageFormat::Jpeg,
    }
}

impl Pm {
    pub(crate) fn image_diff_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Content::Image { kind, old, new } = &self.state.content else {
            return div();
        };
        let fmt = to_format(*kind);
        let view = self.image_view;
        let split = self.diff_split.clamp(DIFF_SPLIT_MIN, DIFF_SPLIT_MAX);

        let pane = self.image_pane;
        let pane_w = f32::from(pane.size.width);
        let pane_h = f32::from(pane.size.height);
        let left_w = (pane_w * split).max(1.0);
        let right_w = (pane_w * (1.0 - split)).max(1.0);

        let entity = cx.entity();
        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(BG))
            .child(
                canvas(
                    move |b, _w, cx| entity.update(cx, |pm, _| pm.image_pane = b),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div().h_full().w(relative(split)).flex_none().child(image_panel(
                    "img-old",
                    old.as_deref(),
                    fmt,
                    view,
                    (left_w, pane_h),
                    "HEAD",
                    cx,
                )),
            )
            .child(self.image_divider())
            .child(
                div().h_full().flex_1().child(image_panel(
                    "img-new",
                    new.as_deref(),
                    fmt,
                    view,
                    (right_w, pane_h),
                    "working",
                    cx,
                )),
            )
    }

    fn image_divider(&self) -> impl IntoElement {
        div()
            .id("image-split")
            .w(px(5.0))
            .h_full()
            .flex_none()
            .bg(rgb(BORDER))
            .cursor_col_resize()
            .on_drag(ResizeHandle::DiffSplit, |_, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| DragPreview)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
}

fn image_panel(
    id: &'static str,
    bytes: Option<&[u8]>,
    fmt: ImageFormat,
    view: ImageView,
    panel: (f32, f32),
    label: &'static str,
    cx: &mut Context<Pm>,
) -> impl IntoElement {
    let (panel_w, panel_h) = panel;

    let content = match bytes {
        None => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(DIM))
            .child(SharedString::from(format!("{label}: absent")))
            .into_any_element(),
        Some(b) => {
            let src = Arc::new(Image::from_bytes(fmt, b.to_vec()));
            // Grow from the centre, then apply the (clamped) pan.
            let ox = -(panel_w * (view.zoom - 1.0)) / 2.0 + view.pan.0;
            let oy = -(panel_h * (view.zoom - 1.0)) / 2.0 + view.pan.1;
            div()
                .absolute()
                .left(px(ox))
                .top(px(oy))
                .w(relative(view.zoom))
                .h(relative(view.zoom))
                .child(img(src).size_full().object_fit(ObjectFit::Contain))
                .into_any_element()
        }
    };

    div()
        .id(id)
        .size_full()
        .relative()
        .overflow_hidden()
        .bg(rgb(BG))
        .on_scroll_wheel(cx.listener(move |pm, e: &ScrollWheelEvent, _, cx| {
            let dy = match e.delta {
                ScrollDelta::Pixels(p) => f32::from(p.y),
                ScrollDelta::Lines(p) => p.y * 20.0,
            };
            let factor = (1.0 + dy * 0.0015).clamp(0.5, 2.0);
            let v = &mut pm.image_view;
            let old_zoom = v.zoom;
            v.zoom = (v.zoom * factor).clamp(0.5, 40.0);
            // Keep the pan proportional so the same region stays under the cursor
            // (approximately — anchored to panel centre).
            let ratio = v.zoom / old_zoom;
            v.pan = (v.pan.0 * ratio, v.pan.1 * ratio);
            v.clamp_pan(panel_w, panel_h);
            cx.notify();
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|pm, e: &gpui::MouseDownEvent, _, cx| {
                if e.click_count >= 2 {
                    pm.image_view = ImageView::default();
                } else {
                    pm.image_drag = Some((f32::from(e.position.x), f32::from(e.position.y)));
                }
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |pm, e: &MouseMoveEvent, _, cx| {
            if let Some((lx, ly)) = pm.image_drag {
                let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
                pm.image_view.pan.0 += x - lx;
                pm.image_view.pan.1 += y - ly;
                pm.image_view.clamp_pan(panel_w, panel_h);
                pm.image_drag = Some((x, y));
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|pm, _, _, cx| {
                pm.image_drag = None;
                cx.notify();
            }),
        )
        .child(content)
        .child(
            div()
                .absolute()
                .top_1()
                .left_2()
                .px_1()
                .rounded_sm()
                .bg(rgb(PANEL))
                .text_size(px(10.0))
                .text_color(rgb(DIM))
                .child(SharedString::from(label)),
        )
}
