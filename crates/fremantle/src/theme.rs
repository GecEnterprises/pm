//! A structured, swappable theme: colors + pixel metrics, installed once as a
//! gpui [`Global`] and read anywhere via the [`ActiveTheme`] extension trait —
//! the same `cx.theme()` pattern Zed's `theme` crate uses (`Global` holding an
//! `Arc<Theme>`, an ext trait adding `.theme()` to `App`). Each consuming app
//! builds its own [`Theme`] value; fremantle ships no built-in palette.

use std::sync::Arc;

use gpui::{rems, App, Global, Hsla, Rems, Window};

/// Whether a theme reads as light or dark — informational for now (no
/// built-in light/dark pair is shipped), but lets a consumer branch on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}

/// Named colors for every surface pm-ui (or another consumer) paints.
#[derive(Clone, Copy, Debug)]
pub struct ThemeColors {
    pub bg: Hsla,
    pub panel: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub dim: Hsla,
    pub select: Hsla,
    pub add_bg: Hsla,
    pub del_bg: Hsla,
    pub changed: Hsla,
    pub close_hover: Hsla,
}

/// Pixel metrics quoted at 1x zoom (see [`Theme::rm`]).
#[derive(Clone, Copy, Debug)]
pub struct ThemeMetrics {
    pub row_h: f32,
    pub list_row_h: f32,
    pub tree_row_h: f32,
    pub tree_indent: f32,
    pub icon_size: f32,
    pub bar: f32,
    pub gutter_w: f32,
    pub gutter_pad: f32,
    pub text_pad_l: f32,
    pub divider_w: f32,
    pub sidebar_min: f32,
    pub sidebar_max_margin: f32,
    pub section_header_h: f32,
    pub section_split_h: f32,
    pub resize_handle_w: f32,
    pub diff_split_min: f32,
    pub diff_split_max: f32,
    pub title_bar_h: f32,
    pub status_bar_h: f32,
    pub client_decoration_rounding: f32,
    pub client_decoration_shadow: f32,
    pub body_font: &'static str,
    pub body_font_size: f32,
    pub ui_font: &'static str,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub appearance: Appearance,
    pub colors: ThemeColors,
    pub metrics: ThemeMetrics,
    /// rem size at 1x zoom — see [`Theme::rm`].
    pub base_rem: f32,
}

impl Theme {
    /// A 1x-pixel metric expressed in rems, so `window.set_rem_size` (whole-
    /// window zoom) scales it. gpui's spacing helpers (`px_2`, `gap_1`, `p_4`,
    /// …) are already rem-based; custom elements that paint raw pixels read
    /// [`Theme::scale_of`] and multiply instead.
    pub fn rm(&self, px_at_1x: f32) -> Rems {
        rems(px_at_1x / self.base_rem)
    }

    /// The current zoom factor, for custom elements that paint raw pixels:
    /// multiply each 1x metric by this.
    pub fn scale_of(&self, window: &Window) -> f32 {
        f32::from(window.rem_size()) / self.base_rem
    }
}

struct GlobalTheme(Arc<Theme>);
impl Global for GlobalTheme {}

/// Install `theme` as the active theme for this app. Call once at startup,
/// before any window opens.
pub fn set_theme(cx: &mut App, theme: Theme) {
    cx.set_global(GlobalTheme(Arc::new(theme)));
}

/// Read the active theme — panics if [`set_theme`] hasn't been called yet.
pub trait ActiveTheme {
    fn theme(&self) -> &Arc<Theme>;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Arc<Theme> {
        &self.global::<GlobalTheme>().0
    }
}
