//! The changed-files sidebar: a custom [`Element`] with the same scroll model as
//! [`crate::diff_view`], minus the columns and horizontal axis.

use gpui::{
    fill, font, point, px, rgb, rgba, size, App, Bounds, ContentMask, DispatchPhase, Element,
    ElementId, Entity, GlobalElementId, HitboxBehavior, HitboxId, InspectorElementId, IntoElement,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollWheelEvent,
    SharedString, ShapedLine, Style, TextAlign, TextRun, Window,
};

use crate::scroll::{Axis, BarInfo, ScrollDrag};
use crate::{Pm, BAR, BORDER, LIST_ROW_H, PANEL, SELECT, TEXT};

pub struct ListView {
    pm: Entity<Pm>,
}

pub fn list_view(pm: Entity<Pm>) -> ListView {
    ListView { pm }
}

impl IntoElement for ListView {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct ListPrepaint {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    first: usize,
    off_y: f32,
    count: usize,
    selected: Option<usize>,
    hover: Option<usize>,
    names: Vec<ShapedLine>,
    bar: Option<BarInfo>,
    body_id: HitboxId,
    bar_id: Option<HitboxId>,
}

impl Element for ListView {
    type RequestLayoutState = ();
    type PrepaintState = ListPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = gpui::relative(1.0).into();
        style.size.height = gpui::relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> ListPrepaint {
        let left = f32::from(bounds.left());
        let top = f32::from(bounds.top());
        let w = f32::from(bounds.size.width).max(0.0);
        let h = f32::from(bounds.size.height).max(0.0);

        let name_font = font("Segoe UI");
        let font_size = px(13.0);

        let (first, off_y, count, selected, hover, names) = self.pm.update(cx, |pm, _cx| {
            let ts = window.text_system();
            let count = pm.files.len();

            pm.list_scroll.content = size(px(0.0), px(count as f32 * LIST_ROW_H));
            pm.list_scroll.viewport = bounds.size;
            pm.list_scroll.clamp();
            let off_y = f32::from(pm.list_scroll.offset.y);

            let first = (off_y / LIST_ROW_H).floor().max(0.0) as usize;
            let last = (((off_y + h) / LIST_ROW_H).ceil() as usize).min(count);

            let mut names = Vec::with_capacity(last - first);
            for i in first..last {
                let s: SharedString = pm.files[i].to_string_lossy().into_owned().into();
                let run = TextRun {
                    len: s.len(),
                    font: name_font.clone(),
                    color: rgb(TEXT).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                names.push(ts.shape_line(s, font_size, &[run], None));
            }
            (first, off_y, count, pm.selected, pm.hover_file, names)
        });

        let content_h = count as f32 * LIST_ROW_H;
        let bar = if content_h > h + 0.5 {
            BarInfo::new(top, h, content_h, h, off_y)
        } else {
            None
        };

        let body = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let bar_id = bar.map(|_| {
            window
                .insert_hitbox(
                    Bounds::new(
                        point(px(left + w - BAR), px(top)),
                        size(px(BAR), px(h)),
                    ),
                    HitboxBehavior::Normal,
                )
                .id
        });

        ListPrepaint {
            left,
            top,
            width: w,
            height: h,
            first,
            off_y,
            count,
            selected,
            hover,
            names,
            bar,
            body_id: body.id,
            bar_id,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        p: &mut ListPrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (left, top, w) = (p.left, p.top, p.width);

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, rgb(PANEL)));

            for (k, name) in p.names.iter().enumerate() {
                let i = p.first + k;
                let y = top + i as f32 * LIST_ROW_H - p.off_y;
                let row = Bounds::new(point(px(left), px(y)), size(px(w), px(LIST_ROW_H)));
                if p.selected == Some(i) {
                    window.paint_quad(fill(row, rgb(SELECT)));
                } else if p.hover == Some(i) {
                    window.paint_quad(fill(row, rgb(BORDER)));
                }
                name.paint(
                    point(px(left + 12.0), px(y)),
                    px(LIST_ROW_H),
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
            }

            if let Some(bar) = p.bar {
                let hovered = p.bar_id.is_some_and(|id| id.is_hovered(window));
                let thickness = BAR - 4.0;
                window.paint_quad(fill(
                    Bounds::new(
                        point(px(left + w - BAR), px(top)),
                        size(px(BAR), px(bar.track_len)),
                    ),
                    rgba(0xffffff08),
                ));
                window.paint_quad(fill(
                    Bounds::new(
                        point(px(left + w - BAR + 2.0), px(bar.thumb_lo)),
                        size(px(thickness), px(bar.thumb_len)),
                    ),
                    if hovered { rgb(0x7a7a7a) } else { rgb(0x5a5a5a) },
                ));
            }
        });

        self.register_mouse(window, p);
    }
}

impl ListView {
    fn register_mouse(&self, window: &mut Window, p: &ListPrepaint) {
        let pm = self.pm.clone();
        let body_id = p.body_id;
        let bar = p.bar;
        let bar_id = p.bar_id;
        let top = p.top;
        let page_y = p.height;
        let count = p.count;

        let row_at = move |window: &Window, py: f32, off_y: f32| -> Option<usize> {
            if !body_id.is_hovered(window) {
                return None;
            }
            let row = ((py - top + off_y) / LIST_ROW_H).floor();
            if row < 0.0 {
                return None;
            }
            let row = row as usize;
            (row < count).then_some(row)
        };

        {
            let pm = pm.clone();
            window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !body_id.should_handle_scroll(window) {
                    return;
                }
                let dy = f32::from(e.delta.pixel_delta(px(LIST_ROW_H)).y);
                let mut moved = false;
                pm.update(cx, |pm, cx| {
                    let y0 = pm.list_scroll.offset.y;
                    pm.list_scroll.offset.y = y0 - px(dy);
                    pm.list_scroll.clamp();
                    moved = pm.list_scroll.offset.y != y0;
                    if moved {
                        cx.notify();
                    }
                });
                if moved {
                    cx.stop_propagation();
                }
            });
        }

        {
            let pm = pm.clone();
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
                    return;
                }
                let pos = f32::from(e.position.y);
                if let (Some(bar), Some(id)) = (bar, bar_id) {
                    if id.is_hovered(window) {
                        pm.update(cx, |pm, cx| {
                            if bar.thumb_hit(pos) {
                                pm.list_drag = Some(ScrollDrag {
                                    axis: Axis::Y,
                                    col: 0,
                                    last: pos,
                                });
                            } else {
                                let dir = if pos < bar.thumb_lo { -1.0 } else { 1.0 };
                                pm.list_scroll.offset.y =
                                    pm.list_scroll.offset.y + px(dir * page_y);
                                pm.list_scroll.clamp();
                            }
                            cx.notify();
                        });
                        cx.stop_propagation();
                        return;
                    }
                }
                let off_y = f32::from(pm.read(cx).list_scroll.offset.y);
                if let Some(row) = row_at(window, pos, off_y) {
                    pm.update(cx, |pm, cx| {
                        pm.select(row);
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
            });
        }

        {
            let pm = pm.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let pos = f32::from(e.position.y);
                let dragging = pm.read(cx).list_drag.is_some();
                if dragging {
                    let mut consumed = false;
                    pm.update(cx, |pm, cx| {
                        let Some(drag) = pm.list_drag else { return };
                        if e.pressed_button != Some(MouseButton::Left) {
                            pm.list_drag = None;
                            cx.notify();
                            return;
                        }
                        let Some(bar) = bar else { return };
                        let cur = f32::from(pm.list_scroll.offset.y);
                        pm.list_scroll.offset.y = px(bar.drag(cur, pos - drag.last));
                        pm.list_scroll.clamp();
                        pm.list_drag = Some(ScrollDrag { last: pos, ..drag });
                        cx.notify();
                        consumed = true;
                    });
                    if consumed {
                        cx.stop_propagation();
                    }
                    return;
                }
                let off_y = f32::from(pm.read(cx).list_scroll.offset.y);
                let hover = row_at(window, pos, off_y);
                pm.update(cx, |pm, cx| {
                    if pm.hover_file != hover {
                        pm.hover_file = hover;
                        cx.notify();
                    }
                });
            });
        }

        {
            let pm = pm.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _window, cx| {
                if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
                    return;
                }
                pm.update(cx, |pm, cx| {
                    if pm.list_drag.take().is_some() {
                        cx.notify();
                    }
                });
            });
        }
    }
}
