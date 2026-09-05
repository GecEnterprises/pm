//! Scroll geometry shared by any scrollable region. No gpui elements — just
//! the math for clamping an offset and sizing a scrollbar thumb. Modelled on
//! Zed's `EditorElement` scrollbar layout.

use gpui::{px, Pixels, Point, Size};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    X,
    Y,
}

/// Minimum scrollbar-thumb length, in px.
pub const MIN_THUMB: f32 = 25.0;

/// One scrollable region. `offset` is the distance scrolled away from the origin,
/// `>= 0` on both axes (the opposite sign convention to gpui's internal negative
/// `ScrollHandle` offsets). `content` / `viewport` are refreshed every prepaint
/// and are zero before the first layout.
#[derive(Clone, Copy, Default, Debug)]
pub struct ScrollState {
    pub offset: Point<Pixels>,
    pub content: Size<Pixels>,
    pub viewport: Size<Pixels>,
}

impl ScrollState {
    pub fn max_offset(&self) -> Point<Pixels> {
        Point {
            x: (self.content.width - self.viewport.width).max(px(0.0)),
            y: (self.content.height - self.viewport.height).max(px(0.0)),
        }
    }

    pub fn clamp(&mut self) {
        let max = self.max_offset();
        self.offset.x = self.offset.x.clamp(px(0.0), max.x);
        self.offset.y = self.offset.y.clamp(px(0.0), max.y);
    }
}

/// An in-progress scrollbar-thumb drag.
#[derive(Clone, Copy, Debug)]
pub struct ScrollDrag {
    pub axis: Axis,
    /// Which text column the drag started in (`0` left, `1` right). Ignored for `Axis::Y`.
    pub col: usize,
    /// Last pointer position along `axis`, in px.
    pub last: f32,
}

/// Everything a paint-time mouse handler needs to hit-test and drag one
/// scrollbar, computed during prepaint. All coordinates are window-space px
/// along the bar's axis.
#[derive(Clone, Copy, Debug)]
pub struct BarInfo {
    pub track_lo: f32,
    pub track_len: f32,
    pub thumb_lo: f32,
    pub thumb_len: f32,
    /// Pointer px travelled per px of content offset.
    pub unit: f32,
    pub max_offset: f32,
}

impl BarInfo {
    /// Zed thumb math. `total` = content extent, `page` = viewport extent along
    /// the axis, `offset` >= 0. Returns `None` when the content fits.
    pub fn new(track_lo: f32, track_len: f32, total: f32, page: f32, offset: f32) -> Option<Self> {
        if total <= page + 0.5 || track_len <= 0.0 {
            return None;
        }
        let thumb_len = (track_len * page / total).clamp(MIN_THUMB, track_len);
        let max_offset = total - page;
        let unit = (track_len - thumb_len) / max_offset;
        let offset = offset.clamp(0.0, max_offset);
        Some(Self {
            track_lo,
            track_len,
            thumb_lo: track_lo + offset * unit,
            thumb_len,
            unit,
            max_offset,
        })
    }

    pub fn thumb_hit(&self, pos: f32) -> bool {
        pos >= self.thumb_lo && pos < self.thumb_lo + self.thumb_len
    }

    /// Content offset after moving the thumb so the pointer travelled `pointer_delta` px.
    pub fn drag(&self, current_offset: f32, pointer_delta: f32) -> f32 {
        (current_offset + pointer_delta / self.unit).clamp(0.0, self.max_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_bar_when_content_fits() {
        assert!(BarInfo::new(0.0, 100.0, 80.0, 100.0, 0.0).is_none());
        assert!(BarInfo::new(0.0, 100.0, 100.0, 100.0, 0.0).is_none());
    }

    #[test]
    fn thumb_proportional_and_positioned() {
        // 400px content, 100px viewport, 100px track, scrolled halfway (150 of 300).
        let b = BarInfo::new(0.0, 100.0, 400.0, 100.0, 150.0).unwrap();
        assert!((b.thumb_len - 25.0).abs() < 0.01); // 100 * 100/400
        assert!((b.thumb_lo - 37.5).abs() < 0.01); // 150/300 * (100 - 25)
    }

    #[test]
    fn drag_is_one_to_one_with_pointer() {
        let b = BarInfo::new(0.0, 100.0, 400.0, 100.0, 0.0).unwrap();
        // moving the thumb the full track travel scrolls the full range
        assert!((b.drag(0.0, b.track_len - b.thumb_len) - b.max_offset).abs() < 0.01);
        assert_eq!(b.drag(0.0, -50.0), 0.0); // clamped
    }

    #[test]
    fn clamp_pins_offset_within_range() {
        let mut s = ScrollState {
            offset: gpui::point(px(999.0), px(-5.0)),
            content: gpui::size(px(200.0), px(1000.0)),
            viewport: gpui::size(px(150.0), px(400.0)),
        };
        s.clamp();
        assert_eq!(s.offset.x, px(50.0));
        assert_eq!(s.offset.y, px(0.0));
    }
}
