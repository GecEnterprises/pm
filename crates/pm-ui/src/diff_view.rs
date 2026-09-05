//! The diff body: one custom gpui [`Element`] that owns a pixel scroll offset,
//! paints only the visible rows (line-number gutters + syntax-highlighted text,
//! full-width row tint), and draws real draggable scrollbars. Modelled on Zed's
//! `EditorElement`.

use std::collections::HashMap;

use gpui::{
    fill, font, point, px, rgb, rgba, size, App, Bounds, ContentMask, CursorStyle, DispatchPhase,
    Element, ElementId, Entity, Font, GlobalElementId, Hitbox, HitboxBehavior, HitboxId,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Rgba, ScrollWheelEvent, SharedString, ShapedLine, Style, TextAlign,
    TextRun, Window, WindowTextSystem,
};

use pm_core::diff::RowKind;
use pm_core::highlight::{Line, Rgba as CoreRgba};

fn to_hsla(c: CoreRgba) -> gpui::Hsla {
    gpui::Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into()
}
use fremantle::scroll::{Axis, BarInfo, ScrollDrag};
use crate::app::Pm;
use crate::theme::{
    ADD_BG, BAR, BASE_REM, BG, BODY_FONT, BODY_FONT_SIZE, BORDER, DEL_BG, DIFF_SPLIT_MAX,
    DIFF_SPLIT_MIN, DIM, DIVIDER_W, GUTTER_PAD, GUTTER_W, ROW_H, TEXT_PAD_L,
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

    /// Cleaned text of a shaped line (`None` for a blank / absent side).
    pub fn line(&self, col: usize, row: usize) -> Option<&str> {
        self.lines
            .get(&(col == 1, row))
            .and_then(|o| o.as_ref())
            .map(|sl| sl.text.as_ref())
    }

    /// x offset (px, column-local) of byte `byte` within a shaped line.
    pub fn line_x(&self, col: usize, row: usize, byte: usize) -> Option<f32> {
        self.lines
            .get(&(col == 1, row))
            .and_then(|o| o.as_ref())
            .map(|sl| f32::from(sl.x_for_index(byte.min(sl.len()))))
    }

    /// Byte index nearest column-local x (px) within a shaped line.
    pub fn byte_at_x(&self, col: usize, row: usize, x: f32) -> usize {
        match self.lines.get(&(col == 1, row)).and_then(|o| o.as_ref()) {
            Some(sl) if x > 0.0 => sl.index_for_x(px(x)).unwrap_or(sl.len()),
            _ => 0,
        }
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
            color: to_hsla(span.color),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
        text.push_str(&clean);
    }
    (text.into(), runs)
}

/// Extra scrollable space below the last line, so the tail of a file isn't
/// jammed against the bottom edge — you can pull it up ~60% of a viewport past
/// the end (PM-22). Only when the file actually overflows.
fn bottom_overhang(content_h: f32, viewport_h: f32) -> f32 {
    if content_h > viewport_h + 0.5 {
        (viewport_h * 0.6).min(content_h)
    } else {
        0.0
    }
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

/// Caret + selection geometry for the visible window, resolved in prepaint.
struct CaretView {
    col: usize,
    focused: bool,
    /// per visible row: column-local selection x-range (before `-off_x`).
    sel: Vec<Option<(f32, f32)>>,
    /// (index into the visible window, column-local x) of the caret head.
    head: Option<(usize, f32)>,
}

pub struct DiffPrepaint {
    mid: f32,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    first: usize,
    row_count: usize,
    off_y: f32,
    off_x: [f32; 2],
    col_text_w: [f32; 2],
    left_text: Bounds<Pixels>,
    right_text: Bounds<Pixels>,
    divider: Bounds<Pixels>,
    divider_hitbox: Hitbox,
    /// `[X-left, X-right, Y]`.
    bars: [Option<BarInfo>; 3],
    body: Hitbox,
    bar_ids: [Option<HitboxId>; 3],
    left_lines: Vec<Option<ShapedLine>>,
    right_lines: Vec<Option<ShapedLine>>,
    left_nums: Vec<Option<ShapedLine>>,
    right_nums: Vec<Option<ShapedLine>>,
    kinds: Vec<RowKind>,
    caret: Option<CaretView>,
    autoscroll: Option<(f32, f32)>,
    // zoom-scaled metrics (1× constant × rem ratio), used by paint + mouse
    row_h: f32,
    gutter_w: f32,
    gutter_pad: f32,
    text_pad_l: f32,
    divider_w: f32,
    bar_w: f32,
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
        let split = self
            .pm
            .read(cx)
            .diff_split
            .clamp(DIFF_SPLIT_MIN, DIFF_SPLIT_MAX);
        let mid = left + (w * split).floor();

        // Whole-window zoom (PM-36): every metric here is a 1× constant times the
        // rem-size ratio, so the diff scales with the rest of the UI.
        let s = f32::from(window.rem_size()) / BASE_REM;
        // Row height MUST be a whole logical pixel (PM-54). gpui quantises glyph
        // baselines to ¼-physical-pixel; a fractional row height (any non-100%
        // zoom, e.g. 19.8 at 110%) makes the gap between successive lines wobble
        // ±¼px as you scroll — the "squiggling lines". gpui's own
        // `line_height_in_pixels` rounds for exactly this reason.
        let row_h = (ROW_H * s).round();
        let gutter_w = GUTTER_W * s;
        let gutter_pad = GUTTER_PAD * s;
        let text_pad_l = TEXT_PAD_L * s;
        let divider_w = (DIVIDER_W * s).max(1.0);
        let bar_w = BAR * s;

        let text_lo = [left + gutter_w, mid + gutter_w];
        let col_text_w = [
            (mid - divider_w - text_lo[0]).max(0.0),
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
            point(px(mid - divider_w), px(top)),
            size(px(divider_w), px(h)),
        );

        let body_font = font(BODY_FONT);
        let font_size = px(BODY_FONT_SIZE * s);

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
            caret,
            autoscroll,
        ) = self.pm.update(cx, |pm, _cx| {
                let ts = window.text_system();
                let Pm {
                    state,
                    shaped,
                    diff,
                    diff_focus,
                    autoscroll,
                    mouse_pos,
                    diff_viewport_h,
                    ..
                } = pm;
                let pm_core::AppState {
                    rows,
                    old_lines,
                    new_lines,
                    caret,
                    ..
                } = state;
                let row_count = rows.len().min(pm_core::MAX_ROWS);
                *diff_viewport_h = h;

                // One-time measurement pass so `max_w` (and the H-thumb size) is exact.
                if shaped.lines.is_empty() && row_count > 0 {
                    for i in 0..row_count {
                        let l = rows[i].left_no.and_then(|k| old_lines.get(k - 1));
                        shaped.ensure((false, i), l, ts, &body_font, font_size);
                        let r = rows[i].right_no.and_then(|k| new_lines.get(k - 1));
                        shaped.ensure((true, i), r, ts, &body_font, font_size);
                    }
                }

                // Middle-click autoscroll: pan proportional to distance from origin.
                if let Some((ox, oy)) = *autoscroll {
                    let (mx, my) = *mouse_pos;
                    let dead = 12.0;
                    let vel = |d: f32| {
                        if d.abs() > dead {
                            (d - d.signum() * dead) * 0.05
                        } else {
                            0.0
                        }
                    };
                    let col = if ox < mid { 0 } else { 1 };
                    diff.y.offset.y = diff.y.offset.y + px(vel(my - oy));
                    diff.x[col].offset.x = diff.x[col].offset.x + px(vel(mx - ox));
                }

                let content_h = row_count as f32 * row_h;
                diff.y.content =
                    size(px(0.0), px(content_h + bottom_overhang(content_h, h)));
                diff.y.viewport = bounds.size;
                diff.y.clamp();
                for c in 0..2 {
                    diff.x[c].content = size(shaped.max_w[c], px(0.0));
                    diff.x[c].viewport = size(px(col_text_w[c]), px(0.0));
                    diff.x[c].clamp();
                }
                let off_y = f32::from(diff.y.offset.y);
                let off_x = [f32::from(diff.x[0].offset.x), f32::from(diff.x[1].offset.x)];

                let first = (off_y / row_h).floor().max(0.0) as usize;
                let last = (((off_y + h) / row_h).ceil() as usize).min(row_count);
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

                let caret_view = (*caret).map(|cur| {
                    let (a, b) = cur.ordered();
                    // File line shown at display row `i` in the caret's column, if any.
                    let file_row = |i: usize| match cur.col {
                        0 => rows[i].left_no,
                        _ => rows[i].right_no,
                    }
                    .map(|n| n - 1);

                    let sel = visible
                        .clone()
                        .map(|i| {
                            let fr = file_row(i)?;
                            if !cur.has_selection() || fr < a.file_row || fr > b.file_row {
                                return None;
                            }
                            let line_w = shaped
                                .line(cur.col, i)
                                .and_then(|s| shaped.line_x(cur.col, i, s.len()))
                                .unwrap_or(0.0);
                            let x0 = if fr == a.file_row {
                                shaped.line_x(cur.col, i, a.byte).unwrap_or(0.0)
                            } else {
                                0.0
                            };
                            let x1 = if fr == b.file_row {
                                shaped.line_x(cur.col, i, b.byte).unwrap_or(0.0)
                            } else {
                                line_w + 4.0
                            };
                            Some((x0, x1))
                        })
                        .collect();

                    let head = visible.clone().find(|&i| file_row(i) == Some(cur.head.file_row)).map(|i| {
                        (
                            i - visible.start,
                            shaped.line_x(cur.col, i, cur.head.byte).unwrap_or(0.0),
                        )
                    });

                    CaretView {
                        col: cur.col,
                        focused: diff_focus.is_focused(window),
                        sel,
                        head,
                    }
                });

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
                    caret_view,
                    *autoscroll,
                )
            });

        let row_count_h = row_count as f32 * row_h;
        let v_over = row_count_h > h + 0.5;
        let mw = [f32::from(max_w[0]), f32::from(max_w[1])];
        let h_over = [
            mw[0] > col_text_w[0] + 0.5,
            mw[1] > col_text_w[1] + 0.5,
        ];
        let any_h = h_over[0] || h_over[1];

        let v_track_len = (h - if any_h { bar_w } else { 0.0 }).max(0.0);
        let h_track_len = [
            col_text_w[0].max(0.0),
            (col_text_w[1] - if v_over { bar_w } else { 0.0 }).max(0.0),
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
                let total = row_count_h + bottom_overhang(row_count_h, h);
                BarInfo::new(top, v_track_len, total, h, off_y)
            } else {
                None
            },
        ];

        let body = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let divider_hitbox = window.insert_hitbox(
            Bounds::new(
                point(px(mid - divider_w - 2.0 * s), px(top)),
                size(px(4.0 * s + divider_w), px(h)),
            ),
            HitboxBehavior::Normal,
        );
        let bar_ids = [
            bars[0].map(|_| {
                window
                    .insert_hitbox(
                        Bounds::new(
                            point(px(text_lo[0]), px(top + h - bar_w)),
                            size(px(h_track_len[0]), px(bar_w)),
                        ),
                        HitboxBehavior::Normal,
                    )
                    .id
            }),
            bars[1].map(|_| {
                window
                    .insert_hitbox(
                        Bounds::new(
                            point(px(text_lo[1]), px(top + h - bar_w)),
                            size(px(h_track_len[1]), px(bar_w)),
                        ),
                        HitboxBehavior::Normal,
                    )
                    .id
            }),
            bars[2].map(|_| {
                window
                    .insert_hitbox(
                        Bounds::new(
                            point(px(left + w - bar_w), px(top)),
                            size(px(bar_w), px(v_track_len)),
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
            row_count,
            off_y,
            off_x,
            col_text_w,
            left_text,
            right_text,
            divider,
            divider_hitbox,
            bars,
            body,
            bar_ids,
            left_lines,
            right_lines,
            left_nums,
            right_nums,
            kinds,
            caret,
            autoscroll,
            row_h,
            gutter_w,
            gutter_pad,
            text_pad_l,
            divider_w,
            bar_w,
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
        let (row_h, gutter_w, gutter_pad, text_pad_l, divider_w, bar_w) = (
            p.row_h, p.gutter_w, p.gutter_pad, p.text_pad_l, p.divider_w, p.bar_w,
        );
        let line_height = px(row_h);
        let (left, top, w, mid) = (p.left, p.top, p.width, p.mid);

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, rgb(BG)));

            for (k, &kind) in p.kinds.iter().enumerate() {
                let i = p.first + k;
                let y = top + i as f32 * row_h - p.off_y;
                let (lbg, rbg) = row_bg(kind);
                if let Some(c) = lbg {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(px(left), px(y)),
                            size(px(mid - divider_w - left), px(row_h)),
                        ),
                        c,
                    ));
                }
                if let Some(c) = rbg {
                    window.paint_quad(fill(
                        Bounds::new(point(px(mid), px(y)), size(px(left + w - mid), px(row_h))),
                        c,
                    ));
                }
            }

            window.paint_quad(fill(p.divider, rgb(BORDER)));

            for (k, num) in p.left_nums.iter().enumerate() {
                if let Some(sl) = num {
                    let y = px(top + (p.first + k) as f32 * row_h - p.off_y);
                    let x = px(left + gutter_w - gutter_pad - f32::from(sl.width()));
                    sl.paint(point(x, y), line_height, TextAlign::Left, None, window, cx)
                        .ok();
                }
            }
            for (k, num) in p.right_nums.iter().enumerate() {
                if let Some(sl) = num {
                    let y = px(top + (p.first + k) as f32 * row_h - p.off_y);
                    let x = px(mid + gutter_w - gutter_pad - f32::from(sl.width()));
                    sl.paint(point(x, y), line_height, TextAlign::Left, None, window, cx)
                        .ok();
                }
            }

            for (col, text_rect, lines, col_x0, off) in [
                (0usize, p.left_text, &p.left_lines, left + gutter_w + text_pad_l, p.off_x[0]),
                (1usize, p.right_text, &p.right_lines, mid + gutter_w + text_pad_l, p.off_x[1]),
            ] {
                let caret = p.caret.as_ref().filter(|cv| cv.col == col);
                window.with_content_mask(Some(ContentMask { bounds: text_rect }), |window| {
                    if let Some(cv) = caret {
                        for (k, span) in cv.sel.iter().enumerate() {
                            if let Some((x0, x1)) = span {
                                let y = top + (p.first + k) as f32 * row_h - p.off_y;
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(px(col_x0 - off + x0), px(y)),
                                        size(px((x1 - x0).max(1.5)), px(row_h)),
                                    ),
                                    rgba(0x3a5f8a99),
                                ));
                            }
                        }
                    }
                    for (k, line) in lines.iter().enumerate() {
                        if let Some(sl) = line {
                            let y = px(top + (p.first + k) as f32 * row_h - p.off_y);
                            let x = px(col_x0 - off);
                            sl.paint(point(x, y), line_height, TextAlign::Left, None, window, cx)
                                .ok();
                        }
                    }
                    if let Some(cv) = caret {
                        if cv.focused {
                            if let Some((k, hx)) = cv.head {
                                let y = top + (p.first + k) as f32 * row_h - p.off_y;
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(px(col_x0 - off + hx), px(y + 1.0)),
                                        size(px(1.5), px(row_h - 2.0)),
                                    ),
                                    rgb(0xd4d4d4),
                                ));
                            }
                        }
                    }
                });
            }

            let h = p.height;
            for idx in 0..3 {
                let Some(bar) = p.bars[idx] else { continue };
                let vertical = idx == 2;
                let hovered = p.bar_ids[idx].is_some_and(|id| id.is_hovered(window));
                let thickness = (bar_w - 4.0 * (bar_w / BAR)).max(2.0);
                let (track, thumb) = if vertical {
                    (
                        Bounds::new(
                            point(px(left + w - bar_w), px(top)),
                            size(px(bar_w), px(bar.track_len)),
                        ),
                        Bounds::new(
                            point(px(left + w - bar_w + 2.0), px(bar.thumb_lo)),
                            size(px(thickness), px(bar.thumb_len)),
                        ),
                    )
                } else {
                    (
                        Bounds::new(
                            point(px(bar.track_lo), px(top + h - bar_w)),
                            size(px(bar.track_len), px(bar_w)),
                        ),
                        Bounds::new(
                            point(px(bar.thumb_lo), px(top + h - bar_w + 2.0)),
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

            if let Some((ox, oy)) = p.autoscroll {
                window.paint_quad(fill(
                    Bounds::new(point(px(ox - 5.0), px(oy - 5.0)), size(px(10.0), px(10.0))),
                    rgba(0xd4d4d466),
                ));
            }
        });

        if p.autoscroll.is_some() {
            window.request_animation_frame();
        }

        // A window-wide cursor keeps it stable while dragging (the pointer leaves
        // the thin handle); otherwise only show it on hover. Both are paint-only.
        if self.pm.read(cx).diff_split_drag.is_some() {
            window.set_window_cursor_style(CursorStyle::ResizeColumn);
        } else {
            // Read-only, but it's text: an I-beam over the panes reads as
            // selectable/inspectable rather than clickable. The divider's own
            // hitbox (registered after `body`) still wins as a resize handle.
            window.set_cursor_style(CursorStyle::IBeam, &p.body);
            window.set_cursor_style(CursorStyle::ResizeColumn, &p.divider_hitbox);
        }
        self.register_mouse(window, p);
    }
}

impl DiffView {
    fn register_mouse(&self, window: &mut Window, p: &DiffPrepaint) {
        let pm = self.pm.clone();
        let body_id = p.body.id;
        let divider_id = p.divider_hitbox.id;
        let bars = p.bars;
        let bar_ids = p.bar_ids;
        let mid = p.mid;
        let left = p.left;
        let top = p.top;
        let width = p.width;
        let page_y = p.height;
        let page_x = p.col_text_w;
        let row_count = p.row_count;
        let (row_h, gutter_w, text_pad_l) = (p.row_h, p.gutter_w, p.text_pad_l);
        let text_lo = [left + gutter_w, mid + gutter_w];

        // Resolve a window-space point to (column, visual row, column-local x px).
        let hit = move |px_x: f32, px_y: f32, off_y: f32, off_x: [f32; 2]| {
            let col = if px_x < mid { 0usize } else { 1 };
            let row = (((px_y - top + off_y) / row_h).floor().max(0.0) as usize)
                .min(row_count.saturating_sub(1));
            let x_local = px_x - text_lo[col] - text_pad_l + off_x[col];
            (col, row, x_local)
        };

        {
            let pm = pm.clone();
            window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !body_id.should_handle_scroll(window) {
                    return;
                }
                let d = e.delta.pixel_delta(px(row_h));
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
                if phase != DispatchPhase::Bubble {
                    return;
                }

                // Middle click toggles autoscroll (pan) mode.
                if e.button == MouseButton::Middle && body_id.is_hovered(window) {
                    let origin = (f32::from(e.position.x), f32::from(e.position.y));
                    pm.update(cx, |pm, cx| {
                        pm.autoscroll = if pm.autoscroll.is_some() { None } else { Some(origin) };
                        cx.notify();
                    });
                    cx.stop_propagation();
                    return;
                }

                if e.button != MouseButton::Left {
                    return;
                }
                if divider_id.is_hovered(window) {
                    let x = f32::from(e.position.x);
                    pm.update(cx, |pm, cx| {
                        pm.diff_split_drag = Some(x);
                        cx.notify();
                    });
                    cx.stop_propagation();
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
                    return;
                }

                // Not on a bar or the divider — place / extend the caret.
                if body_id.is_hovered(window) {
                    let mx = f32::from(e.position.x);
                    let my = f32::from(e.position.y);
                    let clicks = e.click_count;
                    let shift = e.modifiers.shift;
                    let focus = pm.read(cx).diff_focus.clone();
                    window.focus(&focus, cx);
                    pm.update(cx, |pm, cx| {
                        pm.autoscroll = None;
                        let off_y = f32::from(pm.diff.y.offset.y);
                        let off_x = [
                            f32::from(pm.diff.x[0].offset.x),
                            f32::from(pm.diff.x[1].offset.x),
                        ];
                        let (col, row, x_local) = hit(mx, my, off_y, off_x);
                        pm.click_text(col, row, x_local, shift, clicks, cx);
                    });
                    cx.stop_propagation();
                }
            });
        }

        {
            let pm = pm.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }

                let mx = f32::from(e.position.x);
                let my = f32::from(e.position.y);
                let (dragging_text, panning) = {
                    let pm = pm.read(cx);
                    (pm.text_drag, pm.autoscroll.is_some())
                };
                if panning {
                    pm.update(cx, |pm, cx| {
                        pm.mouse_pos = (mx, my);
                        cx.notify();
                    });
                }
                if dragging_text {
                    if e.pressed_button != Some(MouseButton::Left) {
                        pm.update(cx, |pm, cx| {
                            pm.text_drag = false;
                            cx.notify();
                        });
                        return;
                    }
                    pm.update(cx, |pm, cx| {
                        let off_y = f32::from(pm.diff.y.offset.y);
                        let off_x = [
                            f32::from(pm.diff.x[0].offset.x),
                            f32::from(pm.diff.x[1].offset.x),
                        ];
                        let (_, row, x_local) = hit(mx, my, off_y, off_x);
                        pm.drag_text(row, x_local, cx);
                    });
                    cx.stop_propagation();
                    return;
                }

                if pm.read(cx).diff_split_drag.is_some() {
                    let mut consumed = false;
                    pm.update(cx, |pm, cx| {
                        if e.pressed_button != Some(MouseButton::Left) {
                            pm.diff_split_drag = None;
                            cx.notify();
                            return;
                        }
                        let frac = ((f32::from(e.position.x) - left) / width.max(1.0))
                            .clamp(DIFF_SPLIT_MIN, DIFF_SPLIT_MAX);
                        if pm.diff_split != frac {
                            pm.diff_split = frac;
                            cx.notify();
                        }
                        pm.diff_split_drag = Some(f32::from(e.position.x));
                        consumed = true;
                    });
                    if consumed {
                        cx.stop_propagation();
                    }
                    return;
                }
                if pm.read(cx).diff.drag.is_none() {
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
                    let a = pm.diff.drag.take().is_some();
                    let b = pm.diff_split_drag.take().is_some();
                    let c = pm.text_drag;
                    pm.text_drag = false;
                    if a || b || c {
                        cx.notify();
                    }
                });
            });
        }
    }
}
