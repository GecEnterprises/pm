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

/// Register gpui-component and its globals. Call **once** at app startup,
/// before any window opens and before [`set_theme`].
///
/// Kept separate from `set_theme` on purpose: `set_theme` runs per window (from
/// `Pm::new`), whereas `gpui_component::init` binds the kit's key contexts and
/// installs its globals, so running it more than once would double-bind.
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
}

/// Install `theme` as the active theme, and project it onto the gpui-component
/// theme so kit components adopt the same palette.
///
/// This is the seam described in PM-55 as "pattern A": one palette definition
/// owned by the consumer, applied to both pm's custom elements (via
/// [`ActiveTheme`] below) and to every kit component (via the kit's own
/// `ActiveTheme`). The two theme types stay separate because pm's custom
/// painting wants named metrics the kit has no concept of (gutter widths, diff
/// split bounds), while the kit wants a far larger semantic palette than pm
/// defines by hand.
///
/// Requires [`init`] to have run — the kit's `Theme::global_mut` panics
/// otherwise.
pub fn set_theme(cx: &mut App, theme: Theme) {
    project_onto_kit(cx, &theme);
    cx.set_global(GlobalTheme(Arc::new(theme)));
}

/// Map pm's ten named colors onto the kit's semantic palette.
///
/// The kit's `ThemeColor` has ~140 fields; we set the ones that are actually
/// reachable from pm's chrome and let its defaults cover the rest. Anything
/// left at a kit default is a component pm doesn't use yet — when one starts
/// being used and looks wrong, the fix belongs here rather than at the call
/// site.
fn project_onto_kit(cx: &mut App, theme: &Theme) {
    let c = &theme.colors;
    let m = &theme.metrics;

    // Seed the kit's own light/dark tables first; this resets every field to a
    // coherent baseline before we overwrite pm's subset.
    gpui_component::Theme::change(
        match theme.appearance {
            Appearance::Dark => gpui_component::ThemeMode::Dark,
            Appearance::Light => gpui_component::ThemeMode::Light,
        },
        None,
        cx,
    );

    let kit = gpui_component::Theme::global_mut(cx);

    kit.font_family = m.ui_font.into();
    kit.font_size = gpui::px(m.body_font_size);
    kit.mono_font_family = m.body_font.into();
    kit.mono_font_size = gpui::px(m.body_font_size);
    kit.radius = gpui::px(2.0);
    kit.radius_lg = gpui::px(4.0);

    let k = &mut kit.colors;
    k.background = c.bg;
    k.foreground = c.text;
    k.border = c.border;
    k.muted_foreground = c.dim;
    k.caret = c.text;

    // Panels: everything pm paints as a raised/secondary surface.
    k.popover = c.panel;
    k.popover_foreground = c.text;
    k.input = c.panel;
    k.sidebar = c.panel;
    k.sidebar_foreground = c.text;
    k.sidebar_border = c.border;
    k.title_bar = c.panel;
    k.title_bar_border = c.border;
    k.status_bar = c.panel;
    k.status_bar_border = c.border;
    k.tab_bar = c.panel;
    k.list = c.panel;
    k.table = c.panel;

    // Selection: pm has exactly one selected-row colour and uses it everywhere.
    k.selection = c.select;
    k.accent = c.select;
    k.accent_foreground = c.text;
    k.ring = c.select;
    k.list_active = c.select;
    k.table_active = c.select;
    k.sidebar_accent = c.select;
    k.tab_active = c.select;

    // Diff semantics carry over to the kit's success/danger roles.
    k.success = c.add_bg;
    k.danger = c.del_bg;
    k.info = c.changed;
    k.button_danger_hover = c.close_hover;
    k.danger_hover = c.close_hover;
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
