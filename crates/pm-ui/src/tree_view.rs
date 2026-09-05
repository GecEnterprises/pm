//! The Explorer file tree: a custom [`Element`] built on the same scroll model as
//! [`crate::list_view`], with indent + chevron + file-type icon per row.

use std::path::PathBuf;

use gpui::{
    fill, font, point, px, rgb, rgba, size, App, Bounds, ContentMask, DispatchPhase, Element,
    ElementId, Entity, GlobalElementId, HitboxBehavior, HitboxId, InspectorElementId, IntoElement,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollWheelEvent,
    SharedString, ShapedLine, Style, TextAlign, TextRun, TransformationMatrix, Window,
};

use crate::icons;
use fremantle::scroll::{Axis, BarInfo, ScrollDrag};
use crate::app::Pm;
use crate::theme::{
    BAR, BORDER, CHANGED, DIM, ICON_SIZE, PANEL, SELECT, TEXT, TREE_INDENT, TREE_ROW_H, UI_FONT,
};

pub struct TreeView {
    pm: Entity<Pm>,
}

pub fn tree_view(pm: Entity<Pm>) -> TreeView {
    TreeView { pm }
}

impl IntoElement for TreeView {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct TreeRow {
    depth: usize,
    changed: bool,
    selected: bool,
    name: ShapedLine,
    chevron: Option<(&'static str, &'static [u8])>,
    icon: (&'static str, &'static [u8]),
}

pub struct TreePrepaint {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    first: usize,
    off_y: f32,
    count: usize,
    hover: Option<usize>,
    rows: Vec<TreeRow>,
    bar: Option<BarInfo>,
    body_id: HitboxId,
    bar_id: Option<HitboxId>,
    row_h: f32,
    bar_w: f32,
    icon_sz: f32,
    indent: f32,
}

impl Element for TreeView {
    type RequestLayoutState = ();
    type PrepaintState = TreePrepaint;

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
    ) -> TreePrepaint {
        let left = f32::from(bounds.left());
        let top = f32::from(bounds.top());
        let w = f32::from(bounds.size.width).max(0.0);
        let h = f32::from(bounds.size.height).max(0.0);

        let s = crate::theme::scale_of(window);
        let row_h = (TREE_ROW_H * s).round(); // whole px — see PM-54 / diff_view
        let bar_w = BAR * s;
        let icon_sz = ICON_SIZE * s;
        let indent = TREE_INDENT * s;
        let name_font = font(UI_FONT);
        let font_size = px(13.0 * s);

        let (first, off_y, count, hover, rows) = self.pm.update(cx, |pm, _cx| {
            let ts = window.text_system();
            let count = pm.state.visible.len();

            pm.tree_scroll.content = size(px(0.0), px(count as f32 * row_h));
            pm.tree_scroll.viewport = bounds.size;
            pm.tree_scroll.clamp();
            let off_y = f32::from(pm.tree_scroll.offset.y);

            let first = (off_y / row_h).floor().max(0.0) as usize;
            let last = (((off_y + h) / row_h).ceil() as usize).min(count);

            let mut rows = Vec::with_capacity(last - first);
            for vi in first..last {
                let e = &pm.state.tree[pm.state.visible[vi]];
                let changed = pm.state.changed.contains(&e.rel);
                let selected = pm.state.tree_selected.as_deref() == Some(e.rel.as_path());
                let expanded = e.is_dir && pm.state.expanded.contains(&e.rel);

                let leaf = e
                    .rel
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let s: SharedString = leaf.into();
                let run = TextRun {
                    len: s.len(),
                    font: name_font.clone(),
                    color: if changed { rgb(CHANGED) } else { rgb(TEXT) }.into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let name = ts.shape_line(s, font_size, &[run], None);

                rows.push(TreeRow {
                    depth: e.depth,
                    changed,
                    selected,
                    name,
                    chevron: e.is_dir.then(|| icons::chevron_bytes(expanded)),
                    icon: icons::icon_bytes_for(&e.rel, e.is_dir, expanded),
                });
            }
            (first, off_y, count, pm.tree_hover, rows)
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

        TreePrepaint {
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
            icon_sz,
            indent,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        p: &mut TreePrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (left, top, w) = (p.left, p.top, p.width);
        let (row_h, bar_w, icon_sz, indent) = (p.row_h, p.bar_w, p.icon_sz, p.indent);

        let sq = |x: f32, y: f32, s: f32| {
            Bounds::new(
                point(px(x), px(y + (row_h - s) / 2.0)),
                size(px(s), px(s)),
            )
        };

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, rgb(PANEL)));

            for (k, row) in p.rows.iter().enumerate() {
                let vi = p.first + k;
                let y = top + vi as f32 * row_h - p.off_y;
                let row_bounds =
                    Bounds::new(point(px(left), px(y)), size(px(w), px(row_h)));
                if row.selected {
                    window.paint_quad(fill(row_bounds, rgb(SELECT)));
                } else if p.hover == Some(vi) {
                    window.paint_quad(fill(row_bounds, rgb(BORDER)));
                }

                let x0 = left + 8.0 + row.depth as f32 * indent;
                if let Some((key, data)) = row.chevron {
                    window
                        .paint_svg(
                            sq(x0, y, 12.0),
                            key.into(),
                            Some(data),
                            TransformationMatrix::unit(),
                            rgb(DIM).into(),
                            cx,
                        )
                        .ok();
                }
                let (ikey, idata) = row.icon;
                window
                    .paint_svg(
                        sq(x0 + 16.0, y, icon_sz),
                        ikey.into(),
                        Some(idata),
                        TransformationMatrix::unit(),
                        if row.changed { rgb(CHANGED) } else { rgb(DIM) }.into(),
                        cx,
                    )
                    .ok();

                row.name
                    .paint(
                        point(px(x0 + 16.0 + icon_sz + 6.0), px(y)),
                        px(row_h),
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
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

impl TreeView {
    fn register_mouse(&self, window: &mut Window, p: &TreePrepaint) {
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
                    let y0 = pm.tree_scroll.offset.y;
                    pm.tree_scroll.offset.y = y0 - px(dy);
                    pm.tree_scroll.clamp();
                    moved = pm.tree_scroll.offset.y != y0;
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
                                pm.tree_drag = Some(ScrollDrag {
                                    axis: Axis::Y,
                                    col: 0,
                                    last: pos,
                                });
                            } else {
                                let dir = if pos < bar.thumb_lo { -1.0 } else { 1.0 };
                                pm.tree_scroll.offset.y =
                                    pm.tree_scroll.offset.y + px(dir * page_y);
                                pm.tree_scroll.clamp();
                            }
                            cx.notify();
                        });
                        cx.stop_propagation();
                        return;
                    }
                }
                let off_y = f32::from(pm.read(cx).tree_scroll.offset.y);
                if let Some(vi) = row_at(window, pos, off_y) {
                    pm.update(cx, |pm, cx| {
                        let idx = pm.state.visible[vi];
                        let rel: PathBuf = pm.state.tree[idx].rel.clone();
                        if pm.state.tree[idx].is_dir {
                            if !pm.state.expanded.remove(&rel) {
                                pm.state.expanded.insert(rel.clone());
                            }
                            pm.state.tree_selected = Some(rel);
                            pm.state.rebuild_visible();
                        } else {
                            pm.state.tree_selected = Some(rel.clone());
                            pm.open_path(rel);
                        }
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
                let dragging = pm.read(cx).tree_drag.is_some();
                if dragging {
                    let mut consumed = false;
                    pm.update(cx, |pm, cx| {
                        let Some(drag) = pm.tree_drag else { return };
                        if e.pressed_button != Some(MouseButton::Left) {
                            pm.tree_drag = None;
                            cx.notify();
                            return;
                        }
                        let Some(bar) = bar else { return };
                        let cur = f32::from(pm.tree_scroll.offset.y);
                        pm.tree_scroll.offset.y = px(bar.drag(cur, pos - drag.last));
                        pm.tree_scroll.clamp();
                        pm.tree_drag = Some(ScrollDrag { last: pos, ..drag });
                        cx.notify();
                        consumed = true;
                    });
                    if consumed {
                        cx.stop_propagation();
                    }
                    return;
                }
                let off_y = f32::from(pm.read(cx).tree_scroll.offset.y);
                let hover = row_at(window, pos, off_y);
                pm.update(cx, |pm, cx| {
                    if pm.tree_hover != hover {
                        pm.tree_hover = hover;
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
                    if pm.tree_drag.take().is_some() {
                        cx.notify();
                    }
                });
            });
        }
    }
}
