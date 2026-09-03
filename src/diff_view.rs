//! The diff body: one custom gpui [`Element`] that owns a pixel scroll offset,
//! paints only the visible rows (line-number gutters + syntax-highlighted text,
//! full-width row tint), and draws real draggable scrollbars. Modelled on Zed's
//! `EditorElement`.

use std::collections::HashMap;

use gpui::{
    fill, font, point, px, rgb, rgba, size, App, Bounds, ContentMask, DispatchPhase, Element,
    ElementId, Entity, Font, GlobalElementId, HitboxBehavior, HitboxId, InspectorElementId,
    IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Rgba,
    ScrollWheelEvent, SharedString, ShapedLine, Style, TextAlign, TextRun, Window, WindowTextSystem,
};

use crate::diff::RowKind;
use crate::highlight::Line;
use crate::scroll::{Axis, BarInfo, ScrollDrag};
use crate::{
    Pm, ADD_BG, BAR, BG, BODY_FONT, BODY_FONT_SIZE, BORDER, DEL_BG, DIM, DIVIDER_W, GUTTER_PAD,
    GUTTER_W, MAX_ROWS, ROW_H, TEXT_PAD_L,
};

/// Shaped lines for the current file, reused across frames until [`clear`](Self::clear).
#[derive(Default)]
pub struct ShapeCache {
    /// `(is_right_column, row_index) -> shaped line` (`None` for a blank/absent line).
    lines: HashMap<(bool, usize), Option<ShapedLine>>,
    /// Shaped gutter line-numbers, keyed by the number itself.
    nums: HashMap<usize, ShapedLine>,
    /// Widest shaped line per column `[left, right]`.
    max_w: [Pixels; 2],
}

impl ShapeCache {
    pub fn clear(&mut self) {
        self.lines.clear();
        self.nums.clear();
        self.max_w = [px(0.0), px(0.0)];
    }

    fn ensure(
        &mut self,
        key: (bool, usize),
        line: Option<&Line>,
        ts: &WindowTextSystem,
        font: &Font,
        size: Pixels,
    ) {
        if self.lines.contains_key(&key) {
            return;
        }
        let shaped = line.and_then(|line| {
            let (text, runs) = runs_for_line(line, font);
            if text.is_empty() {
                return None;
            }
            let sl = ts.shape_line(text, size, &runs, None);
            let col = key.0 as usize;
            if sl.width() > self.max_w[col] {
                self.max_w[col] = sl.width();
            }
            Some(sl)
        });
        self.lines.insert(key, shaped);
    }

    fn ensure_num(&mut self, n: usize, ts: &WindowTextSystem, font: &Font, size: Pixels) {
        if self.nums.contains_key(&n) {
            return;
        }
        let text: SharedString = n.to_string().into();
        let run = TextRun {
            len: text.len(),
            font: font.clone(),
            color: rgb(DIM).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let sl = ts.shape_line(text, size, &[run], None);
        self.nums.insert(n, sl);
    }
}

fn runs_for_line(line: &Line, font: &Font) -> (SharedString, Vec<TextRun>) {
    let mut text = String::new();
    let mut runs = Vec::with_capacity(line.len());
    for span in line {
        let clean: String = span.text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        if clean.is_empty() {
            continue;
        }
        runs.push(TextRun {
            len: clean.len(),
            font: font.clone(),
            color: span.color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
        text.push_str(&clean);
    }
    (text.into(), runs)
}

fn row_bg(kind: RowKind) -> (Option<Rgba>, Option<Rgba>) {
    match kind {
        RowKind::Equal => (None, None),
        RowKind::Add => (None, Some(rgb(ADD_BG))),
        RowKind::Remove => (Some(rgb(DEL_BG)), None),
        RowKind::Modify => (Some(rgb(DEL_BG)), Some(rgb(ADD_BG))),
    }
}

pub struct DiffView {
    pm: Entity<Pm>,
}

pub fn diff_view(pm: Entity<Pm>) -> DiffView {
    DiffView { pm }
}

impl IntoElement for DiffView {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct DiffPrepaint {
    mid: f32,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    first: usize,
    off_y: f32,
    off_x: [f32; 2],
    col_text_w: [f32; 2],
    left_text: Bounds<Pixels>,
    right_text: Bounds<Pixels>,
    divider: Bounds<Pixels>,
    /// `[X-left, X-right, Y]`.
    bars: [Option<BarInfo>; 3],
    body_id: HitboxId,
    bar_ids: [Option<HitboxId>; 3],
    left_lines: Vec<Option<ShapedLine>>,
    right_lines: Vec<Option<ShapedLine>>,
    left_nums: Vec<Option<ShapedLine>>,
    right_nums: Vec<Option<ShapedLine>>,
    kinds: Vec<RowKind>,
}

impl Element for DiffView {
    type RequestLayoutState = ();
    type PrepaintState = DiffPrepaint;

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
    ) -> DiffPrepaint {
        let left = f32::from(bounds.left());
        let top = f32::from(bounds.top());
        let w = f32::from(bounds.size.width).max(0.0);
        let h = f32::from(bounds.size.height).max(0.0);
        let mid = left + (w / 2.0).floor();

        let text_lo = [left + GUTTER_W, mid + GUTTER_W];
        let col_text_w = [
            (mid - DIVIDER_W - text_lo[0]).max(0.0),
            (left + w - text_lo[1]).max(0.0),
        ];
        let left_text = Bounds::new(
            point(px(text_lo[0]), px(top)),
            size(px(col_text_w[0]), px(h)),
        );
        let right_text = Bounds::new(
            point(px(text_lo[1]), px(top)),
            size(px(col_text_w[1]), px(h)),
        );
        let divider = Bounds::new(
            point(px(mid - DIVIDER_W), px(top)),
            size(px(DIVIDER_W), px(h)),
        );

        let body_font = font(BODY_FONT);
        let font_size = px(BODY_FONT_SIZE);

        let (
            row_count,
            first,
            off_y,
            off_x,
            max_w,
            left_lines,
            right_lines,
            left_nums,
            right_nums,
            kinds,
        ) = self.pm.update(cx, |pm, _cx| {
                let ts = window.text_system();
                let Pm {
                    rows,
                    old_lines,
                    new_lines,
                    shaped,
                    diff,
                    ..
                } = pm;
                let row_count = rows.len().min(MAX_ROWS);

                // One-time measurement pass so `max_w` (and the H-thumb size) is exact.
                if shaped.lines.is_empty() && row_count > 0 {
                    for i in 0..row_count {
                        let l = rows[i].left_no.and_then(|k| old_lines.get(k - 1));
                        shaped.ensure((false, i), l, ts, &body_font, font_size);
                        let r = rows[i].right_no.and_then(|k| new_lines.get(k - 1));
                        shaped.ensure((true, i), r, ts, &body_font, font_size);
                    }
                }

                diff.y.content = size(px(0.0), px(row_count as f32 * ROW_H));
                diff.y.viewport = bounds.size;
                diff.y.clamp();
                for c in 0..2 {
                    diff.x[c].content = size(shaped.max_w[c], px(0.0));
                    diff.x[c].viewport = size(px(col_text_w[c]), px(0.0));
                    diff.x[c].clamp();
                }
                let off_y = f32::from(diff.y.offset.y);
                let off_x = [f32::from(diff.x[0].offset.x), f32::from(diff.x[1].offset.x)];

                let first = (off_y / ROW_H).floor().max(0.0) as usize;
                let last = (((off_y + h) / ROW_H).ceil() as usize).min(row_count);
                let visible = first..last;

                for i in visible.clone() {
                    // Safety net if the measurement pass was skipped for this key.
                    if !shaped.lines.contains_key(&(false, i)) {
                        let l = rows[i].left_no.and_then(|k| old_lines.get(k - 1));
                        shaped.ensure((false, i), l, ts, &body_font, font_size);
                    }
                    if !shaped.lines.contains_key(&(true, i)) {
                        let r = rows[i].right_no.and_then(|k| new_lines.get(k - 1));
                        shaped.ensure((true, i), r, ts, &body_font, font_size);
                    }
                    if let Some(n) = rows[i].left_no {
                        shaped.ensure_num(n, ts, &body_font, font_size);
                    }
                    if let Some(n) = rows[i].right_no {
                        shaped.ensure_num(n, ts, &body_font, font_size);
                    }
                }

                let mut left_lines = Vec::with_capacity(visible.len());
                let mut right_lines = Vec::with_capacity(visible.len());
                let mut left_nums = Vec::with_capacity(visible.len());
                let mut right_nums = Vec::with_capacity(visible.len());
                let mut kinds = Vec::with_capacity(visible.len());
                for i in visible.clone() {
                    left_lines.push(shaped.lines.get(&(false, i)).cloned().flatten());
                    right_lines.push(shaped.lines.get(&(true, i)).cloned().flatten());
                    left_nums.push(rows[i].left_no.and_then(|n| shaped.nums.get(&n).cloned()));
                    right_nums.push(rows[i].right_no.and_then(|n| shaped.nums.get(&n).cloned()));
                    kinds.push(rows[i].kind);
                }

                (
                    row_count,
                    first,
                    off_y,
                    off_x,
                    shaped.max_w,
                    left_lines,
                    right_lines,
                    left_nums,
                    right_nums,
                    kinds,
                )
            });

        let row_count_h = row_count as f32 * ROW_H;
        let v_over = row_count_h > h + 0.5;
        let mw = [f32::from(max_w[0]), f32::from(max_w[1])];
        let h_over = [
            mw[0] > col_text_w[0] + 0.5,
            mw[1] > col_text_w[1] + 0.5,
        ];
        let any_h = h_over[0] || h_over[1];

        let v_track_len = (h - if any_h { BAR } else { 0.0 }).max(0.0);
        let h_track_len = [
            col_text_w[0].max(0.0),
            (col_text_w[1] - if v_over { BAR } else { 0.0 }).max(0.0),
        ];

        let bars = [
            if h_over[0] {
                BarInfo::new(text_lo[0], h_track_len[0], mw[0], col_text_w[0], off_x[0])
            } else {
                None
            },
            if h_over[1] {
                BarInfo::new(text_lo[1], h_track_len[1], mw[1], col_text_w[1], off_x[1])
            } else {
                None
            },
            if v_over {
                BarInfo::new(top, v_track_len, row_count_h, h, off_y)
            } else {
                None
            },
        ];

        let body = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let bar_ids = [
            bars[0].map(|_| {
                window
                    .insert_hitbox(
                        Bounds::new(
                            point(px(text_lo[0]), px(top + h - BAR)),
                            size(px(h_track_len[0]), px(BAR)),
                        ),
                        HitboxBehavior::Normal,
                    )
                    .id
            }),
            bars[1].map(|_| {
                window
                    .insert_hitbox(
                        Bounds::new(
                            point(px(text_lo[1]), px(top + h - BAR)),
                            size(px(h_track_len[1]), px(BAR)),
                        ),
                        HitboxBehavior::Normal,
                    )
                    .id
            }),
            bars[2].map(|_| {
                window
                    .insert_hitbox(
                        Bounds::new(
                            point(px(left + w - BAR), px(top)),
                            size(px(BAR), px(v_track_len)),
                        ),
                        HitboxBehavior::Normal,
                    )
                    .id
            }),
        ];

        DiffPrepaint {
            mid,
            left,
            top,
            width: w,
            height: h,
            first,
            off_y,
            off_x,
            col_text_w,
            left_text,
            right_text,
            divider,
            bars,
            body_id: body.id,
            bar_ids,
            left_lines,
            right_lines,
            left_nums,
            right_nums,
            kinds,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        p: &mut DiffPrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let line_height = px(ROW_H);
        let (left, top, w, mid) = (p.left, p.top, p.width, p.mid);

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, rgb(BG)));

            for (k, &kind) in p.kinds.iter().enumerate() {
                let i = p.first + k;
                let y = top + i as f32 * ROW_H - p.off_y;
                let (lbg, rbg) = row_bg(kind);
                if let Some(c) = lbg {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(px(left), px(y)),
                            size(px(mid - DIVIDER_W - left), px(ROW_H)),
                        ),
                        c,
                    ));
                }
                if let Some(c) = rbg {
                    window.paint_quad(fill(
                        Bounds::new(point(px(mid), px(y)), size(px(left + w - mid), px(ROW_H))),
                        c,
                    ));
                }
            }

            window.paint_quad(fill(p.divider, rgb(BORDER)));

            for (k, num) in p.left_nums.iter().enumerate() {
                if let Some(sl) = num {
                    let y = px(top + (p.first + k) as f32 * ROW_H - p.off_y);
                    let x = px(left + GUTTER_W - GUTTER_PAD - f32::from(sl.width()));
                    sl.paint(point(x, y), line_height, TextAlign::Left, None, window, cx)
                        .ok();
                }
            }
            for (k, num) in p.right_nums.iter().enumerate() {
                if let Some(sl) = num {
                    let y = px(top + (p.first + k) as f32 * ROW_H - p.off_y);
                    let x = px(mid + GUTTER_W - GUTTER_PAD - f32::from(sl.width()));
                    sl.paint(point(x, y), line_height, TextAlign::Left, None, window, cx)
                        .ok();
                }
            }

            let left_text = p.left_text;
            window.with_content_mask(Some(ContentMask { bounds: left_text }), |window| {
                for (k, line) in p.left_lines.iter().enumerate() {
                    if let Some(sl) = line {
                        let y = px(top + (p.first + k) as f32 * ROW_H - p.off_y);
                        let x = px(left + GUTTER_W + TEXT_PAD_L - p.off_x[0]);
                        sl.paint(point(x, y), line_height, TextAlign::Left, None, window, cx)
                            .ok();
                    }
                }
            });
            let right_text = p.right_text;
            window.with_content_mask(Some(ContentMask { bounds: right_text }), |window| {
                for (k, line) in p.right_lines.iter().enumerate() {
                    if let Some(sl) = line {
                        let y = px(top + (p.first + k) as f32 * ROW_H - p.off_y);
                        let x = px(mid + GUTTER_W + TEXT_PAD_L - p.off_x[1]);
                        sl.paint(point(x, y), line_height, TextAlign::Left, None, window, cx)
                            .ok();
                    }
                }
            });

            let h = p.height;
            for idx in 0..3 {
                let Some(bar) = p.bars[idx] else { continue };
                let vertical = idx == 2;
                let hovered = p.bar_ids[idx].is_some_and(|id| id.is_hovered(window));
                let thickness = BAR - 4.0;
                let (track, thumb) = if vertical {
                    (
                        Bounds::new(
                            point(px(left + w - BAR), px(top)),
                            size(px(BAR), px(bar.track_len)),
                        ),
                        Bounds::new(
                            point(px(left + w - BAR + 2.0), px(bar.thumb_lo)),
                            size(px(thickness), px(bar.thumb_len)),
                        ),
                    )
                } else {
                    (
                        Bounds::new(
                            point(px(bar.track_lo), px(top + h - BAR)),
                            size(px(bar.track_len), px(BAR)),
                        ),
                        Bounds::new(
                            point(px(bar.thumb_lo), px(top + h - BAR + 2.0)),
                            size(px(bar.thumb_len), px(thickness)),
                        ),
                    )
                };
                window.paint_quad(fill(track, rgba(0xffffff08)));
                window.paint_quad(fill(
                    thumb,
                    if hovered { rgb(0x7a7a7a) } else { rgb(0x5a5a5a) },
                ));
            }
        });

        self.register_mouse(window, p, bounds);
    }
}

impl DiffView {
    fn register_mouse(&self, window: &mut Window, p: &DiffPrepaint, bounds: Bounds<Pixels>) {
        let pm = self.pm.clone();
        let body_id = p.body_id;
        let bars = p.bars;
        let bar_ids = p.bar_ids;
        let mid = p.mid;
        let page_y = p.height;
        let page_x = p.col_text_w;
        let _ = bounds;

        {
            let pm = pm.clone();
            window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !body_id.should_handle_scroll(window) {
                    return;
                }
                let d = e.delta.pixel_delta(px(ROW_H));
                let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                let shift = e.modifiers.shift;
                let col = if f32::from(e.position.x) < mid { 0 } else { 1 };
                let mut moved = false;
                pm.update(cx, |pm, cx| {
                    let y0 = pm.diff.y.offset.y;
                    pm.diff.y.offset.y = y0 - px(dy);
                    pm.diff.y.clamp();
                    let hx = if shift { dx + dy } else { dx };
                    let x0 = pm.diff.x[col].offset.x;
                    pm.diff.x[col].offset.x = x0 - px(hx);
                    pm.diff.x[col].clamp();
                    moved = pm.diff.y.offset.y != y0 || pm.diff.x[col].offset.x != x0;
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
                for idx in 0..3 {
                    let (Some(bar), Some(id)) = (bars[idx], bar_ids[idx]) else {
                        continue;
                    };
                    if !id.is_hovered(window) {
                        continue;
                    }
                    let vertical = idx == 2;
                    let col = if vertical { 0 } else { idx };
                    let axis = if vertical { Axis::Y } else { Axis::X };
                    let pos = if vertical {
                        f32::from(e.position.y)
                    } else {
                        f32::from(e.position.x)
                    };
                    let page = if vertical { page_y } else { page_x[col] };
                    pm.update(cx, |pm, cx| {
                        if bar.thumb_hit(pos) {
                            pm.diff.drag = Some(ScrollDrag {
                                axis,
                                col,
                                last: pos,
                            });
                        } else {
                            let dir = if pos < bar.thumb_lo { -1.0 } else { 1.0 };
                            match axis {
                                Axis::Y => {
                                    pm.diff.y.offset.y = pm.diff.y.offset.y + px(dir * page);
                                    pm.diff.y.clamp();
                                }
                                Axis::X => {
                                    pm.diff.x[col].offset.x =
                                        pm.diff.x[col].offset.x + px(dir * page);
                                    pm.diff.x[col].clamp();
                                }
                            }
                        }
                        cx.notify();
                    });
                    cx.stop_propagation();
                    break;
                }
            });
        }

        {
            let pm = pm.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _window, cx| {
                if phase != DispatchPhase::Bubble || pm.read(cx).diff.drag.is_none() {
                    return;
                }
                let mut consumed = false;
                pm.update(cx, |pm, cx| {
                    let Some(drag) = pm.diff.drag else { return };
                    if e.pressed_button != Some(MouseButton::Left) {
                        pm.diff.drag = None;
                        cx.notify();
                        return;
                    }
                    let idx = if drag.axis == Axis::Y { 2 } else { drag.col };
                    let Some(bar) = bars[idx] else { return };
                    let pos = if drag.axis == Axis::Y {
                        f32::from(e.position.y)
                    } else {
                        f32::from(e.position.x)
                    };
                    let delta = pos - drag.last;
                    match drag.axis {
                        Axis::Y => {
                            let cur = f32::from(pm.diff.y.offset.y);
                            pm.diff.y.offset.y = px(bar.drag(cur, delta));
                            pm.diff.y.clamp();
                        }
                        Axis::X => {
                            let cur = f32::from(pm.diff.x[drag.col].offset.x);
                            pm.diff.x[drag.col].offset.x = px(bar.drag(cur, delta));
                            pm.diff.x[drag.col].clamp();
                        }
                    }
                    pm.diff.drag = Some(ScrollDrag { last: pos, ..drag });
                    cx.notify();
                    consumed = true;
                });
                if consumed {
                    cx.stop_propagation();
                }
            });
        }

        {
            let pm = pm.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _window, cx| {
                if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
                    return;
                }
                pm.update(cx, |pm, cx| {
                    if pm.diff.drag.take().is_some() {
                        cx.notify();
                    }
                });
            });
        }
    }
}
