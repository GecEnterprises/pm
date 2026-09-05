//! The "Commit History" list: a custom [`Element`] modelled on
//! [`crate::list_view`]. Row 0 is "Working Tree" (the default diff); the rest are
//! recent commits. Clicking a row re-points the diff (`Pm::select_commit`).

use gpui::{
    fill, font, point, px, rgb, rgba, size, App, Bounds, ContentMask, DispatchPhase, Element,
    ElementId, Entity, GlobalElementId, HitboxBehavior, HitboxId, InspectorElementId, IntoElement,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollWheelEvent,
    SharedString, ShapedLine, Style, TextAlign, TextRun, Window,
};

use pm_core::DiffTarget;

use crate::app::Pm;
use fremantle::scroll::{Axis, BarInfo, ScrollDrag};
use crate::theme::{BAR, BORDER, DIM, LIST_ROW_H, PANEL, SELECT, TEXT, UI_FONT};

pub struct HistoryView {
    pm: Entity<Pm>,
}

pub fn history_view(pm: Entity<Pm>) -> HistoryView {
    HistoryView { pm }
}

impl IntoElement for HistoryView {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct CommitRow {
    selected: bool,
    /// `<sha>  <summary>` or "Working Tree", pre-shaped.
    label: ShapedLine,
    /// Right-aligned relative time (`None` for the Working Tree row).
    time: Option<ShapedLine>,
}

pub struct HistoryPrepaint {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    first: usize,
    off_y: f32,
    count: usize,
    hover: Option<usize>,
    rows: Vec<CommitRow>,
    bar: Option<BarInfo>,
    body_id: HitboxId,
    bar_id: Option<HitboxId>,
    row_h: f32,
    bar_w: f32,
}

/// Compact "time since" label, e.g. `3d`, `2w`, `5mo`.
pub(crate) fn rel_time(secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(secs);
    let d = (now - secs).max(0);
    match d {
        0..=59 => "now".to_string(),
        60..=3599 => format!("{}m", d / 60),
        3600..=86399 => format!("{}h", d / 3600),
        86400..=604799 => format!("{}d", d / 86400),
        604800..=2591999 => format!("{}w", d / 604800),
        2592000..=31535999 => format!("{}mo", d / 2592000),
        _ => format!("{}y", d / 31536000),
    }
}

impl Element for HistoryView {
    type RequestLayoutState = ();
    type PrepaintState = HistoryPrepaint;

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
    ) -> HistoryPrepaint {
        let left = f32::from(bounds.left());
        let top = f32::from(bounds.top());
        let w = f32::from(bounds.size.width).max(0.0);
        let h = f32::from(bounds.size.height).max(0.0);

        let s = crate::theme::scale_of(window);
        let row_h = (LIST_ROW_H * s).round(); // whole px — see PM-54 / diff_view
        let bar_w = BAR * s;
        let ui = font(UI_FONT);
        let text_size = px(12.5 * s);
        let small = px(11.0 * s);

        let run = |text: &str, color: u32| TextRun {
            len: text.len(),
            font: ui.clone(),
            color: rgb(color).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let (first, off_y, count, hover, rows) = self.pm.update(cx, |pm, _cx| {
            let ts = window.text_system();
            let count = pm.state.commits.len() + 1;

            pm.history_scroll.content = size(px(0.0), px(count as f32 * row_h));
            pm.history_scroll.viewport = bounds.size;
            pm.history_scroll.clamp();
            let off_y = f32::from(pm.history_scroll.offset.y);

            let first = (off_y / row_h).floor().max(0.0) as usize;
            let last = (((off_y + h) / row_h).ceil() as usize).min(count);

            let mut rows = Vec::with_capacity(last - first);
            for i in first..last {
                if i == 0 {
                    let s: SharedString = "\u{25cf}  Working Tree".into();
                    let label = ts.shape_line(s.clone(), text_size, &[run(&s, TEXT)], None);
                    rows.push(CommitRow {
                        selected: pm.state.target == DiffTarget::WorkingTree,
                        label,
                        time: None,
                    });
                    continue;
                }
                let c = &pm.state.commits[i - 1];
                let selected = matches!(pm.state.target, DiffTarget::Commit(o) if o == c.id);

                let sha = format!("{}  ", c.short_id);
                let text = format!("{sha}{}", c.summary);
                let label = ts.shape_line(
                    text.clone().into(),
                    text_size,
                    &[run(&sha, DIM), run(&c.summary, TEXT)],
                    None,
                );

                let t: SharedString = rel_time(c.time).into();
                let time = ts.shape_line(t.clone(), small, &[run(&t, DIM)], None);

                rows.push(CommitRow {
                    selected,
                    label,
                    time: Some(time),
                });
            }
            (first, off_y, count, pm.history_hover, rows)
        });

        let content_h = count as f32 * row_h;
        let bar = if content_h > h + 0.5 {
            BarInfo::new(top, h, content_h, h, off_y)
        } else {
            None
        };

        let body = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let bar_id = bar.map(|_| {
            window
                .insert_hitbox(
                    Bounds::new(point(px(left + w - bar_w), px(top)), size(px(bar_w), px(h))),
                    HitboxBehavior::Normal,
                )
                .id
        });

        HistoryPrepaint {
            left,
            top,
            width: w,
            height: h,
            first,
            off_y,
            count,
            hover,
            rows,
            bar,
            body_id: body.id,
            bar_id,
            row_h,
            bar_w,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        p: &mut HistoryPrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (left, top, w) = (p.left, p.top, p.width);
        let (row_h, bar_w) = (p.row_h, p.bar_w);

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, rgb(PANEL)));

            for (k, row) in p.rows.iter().enumerate() {
                let i = p.first + k;
                let y = top + i as f32 * row_h - p.off_y;
                let rb = Bounds::new(point(px(left), px(y)), size(px(w), px(row_h)));
                if row.selected {
                    window.paint_quad(fill(rb, rgb(SELECT)));
                } else if p.hover == Some(i) {
                    window.paint_quad(fill(rb, rgb(BORDER)));
                }

                // Right-aligned relative time first, so the label can be clipped
                // by the content mask without overlapping it.
                let time_w = row
                    .time
                    .as_ref()
                    .map(|t| f32::from(t.width()) + 10.0)
                    .unwrap_or(0.0);
                if let Some(t) = &row.time {
                    let tx = left + w - 10.0 - f32::from(t.width());
                    t.paint(point(px(tx), px(y)), px(row_h), TextAlign::Left, None, window, cx)
                        .ok();
                }

                let label_bounds = Bounds::new(
                    point(px(left), px(y)),
                    size(px((w - time_w - 12.0).max(0.0)), px(row_h)),
                );
                window.with_content_mask(Some(ContentMask { bounds: label_bounds }), |window| {
                    row.label
                        .paint(
                            point(px(left + 10.0), px(y)),
                            px(row_h),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        )
                        .ok();
                });
            }

            if let Some(bar) = p.bar {
                let hovered = p.bar_id.is_some_and(|id| id.is_hovered(window));
                let thickness = (bar_w - 4.0 * (bar_w / BAR)).max(2.0);
                window.paint_quad(fill(
                    Bounds::new(
                        point(px(left + w - bar_w), px(top)),
                        size(px(bar_w), px(bar.track_len)),
                    ),
                    rgba(0xffffff08),
                ));
                window.paint_quad(fill(
                    Bounds::new(
                        point(px(left + w - bar_w + 2.0), px(bar.thumb_lo)),
                        size(px(thickness), px(bar.thumb_len)),
                    ),
                    if hovered { rgb(0x7a7a7a) } else { rgb(0x5a5a5a) },
                ));
            }
        });

        self.register_mouse(window, p);
    }
}

impl HistoryView {
    fn register_mouse(&self, window: &mut Window, p: &HistoryPrepaint) {
        let pm = self.pm.clone();
        let body_id = p.body_id;
        let bar = p.bar;
        let bar_id = p.bar_id;
        let top = p.top;
        let row_h = p.row_h;
        let page_y = p.height;
        let count = p.count;

        let row_at = move |window: &Window, py: f32, off_y: f32| -> Option<usize> {
            if !body_id.is_hovered(window) {
                return None;
            }
            let row = ((py - top + off_y) / row_h).floor();
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
                let dy = f32::from(e.delta.pixel_delta(px(row_h)).y);
                let mut moved = false;
                pm.update(cx, |pm, cx| {
                    let y0 = pm.history_scroll.offset.y;
                    pm.history_scroll.offset.y = y0 - px(dy);
                    pm.history_scroll.clamp();
                    moved = pm.history_scroll.offset.y != y0;
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
                                pm.history_drag = Some(ScrollDrag {
                                    axis: Axis::Y,
                                    col: 0,
                                    last: pos,
                                });
                            } else {
                                let dir = if pos < bar.thumb_lo { -1.0 } else { 1.0 };
                                pm.history_scroll.offset.y =
                                    pm.history_scroll.offset.y + px(dir * page_y);
                                pm.history_scroll.clamp();
                            }
                            cx.notify();
                        });
                        cx.stop_propagation();
                        return;
                    }
                }
                let off_y = f32::from(pm.read(cx).history_scroll.offset.y);
                if let Some(row) = row_at(window, pos, off_y) {
                    pm.update(cx, |pm, cx| {
                        pm.select_commit(row);
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
                let dragging = pm.read(cx).history_drag.is_some();
                if dragging {
                    let mut consumed = false;
                    pm.update(cx, |pm, cx| {
                        let Some(drag) = pm.history_drag else { return };
                        if e.pressed_button != Some(MouseButton::Left) {
                            pm.history_drag = None;
                            cx.notify();
                            return;
                        }
                        let Some(bar) = bar else { return };
                        let cur = f32::from(pm.history_scroll.offset.y);
                        pm.history_scroll.offset.y = px(bar.drag(cur, pos - drag.last));
                        pm.history_scroll.clamp();
                        pm.history_drag = Some(ScrollDrag { last: pos, ..drag });
                        cx.notify();
                        consumed = true;
                    });
                    if consumed {
                        cx.stop_propagation();
                    }
                    return;
                }
                let off_y = f32::from(pm.read(cx).history_scroll.offset.y);
                let hover = row_at(window, pos, off_y);
                pm.update(cx, |pm, cx| {
                    if pm.history_hover != hover {
                        pm.history_hover = hover;
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
                    if pm.history_drag.take().is_some() {
                        cx.notify();
                    }
                });
            });
        }
    }
}
