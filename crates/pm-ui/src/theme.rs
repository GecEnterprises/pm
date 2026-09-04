//! Colours and pixel metrics for the pm UI.
//!
//! The `*_H` / `*_W` / font-size metrics below are quoted at 1× (100% zoom).
//! Whole-window scaling (PM-36) works by `window.set_rem_size(BASE_REM * scale)`:
//! gpui's spacing helpers (`px_2`, `gap_1`, `p_4`, …) are already rem-based, and
//! [`rm`] turns a 1× px metric into rems so it scales too. Custom elements that
//! paint raw pixels read `window.rem_size() / BASE_REM` and multiply.

use gpui::{rems, Rems, Window};

/// rem size at 1× zoom.
pub const BASE_REM: f32 = 16.0;

/// A 1×-pixel metric expressed in rems, so `set_rem_size` scales it.
pub fn rm(px_at_1x: f32) -> Rems {
    rems(px_at_1x / BASE_REM)
}

/// The current zoom factor, for custom elements that paint raw pixels: multiply
/// each 1× metric by this.
pub fn scale_of(window: &Window) -> f32 {
    f32::from(window.rem_size()) / BASE_REM
}

pub const BG: u32 = 0x1e1e1e;
pub const PANEL: u32 = 0x252526;
pub const BORDER: u32 = 0x333333;
pub const TEXT: u32 = 0xd4d4d4;
pub const DIM: u32 = 0x808080;
pub const SELECT: u32 = 0x094771;
pub const ADD_BG: u32 = 0x18321f;
pub const DEL_BG: u32 = 0x3a1d1d;
/// Tint for changed files / dirs in the Explorer tree.
pub const CHANGED: u32 = 0x4ec9b0;

/// Diff row height / line-height, in px.
pub const ROW_H: f32 = 18.0;
/// "Changes" list row height, in px.
pub const LIST_ROW_H: f32 = 24.0;
/// Explorer tree row height, in px.
pub const TREE_ROW_H: f32 = 22.0;
/// Explorer indent per depth level, in px.
pub const TREE_INDENT: f32 = 14.0;
/// File-type icon size, in px.
pub const ICON_SIZE: f32 = 14.0;
/// Scrollbar track thickness, in px.
pub const BAR: f32 = 12.0;
/// Line-number column width, in px.
pub const GUTTER_W: f32 = 52.0;
/// Line-number column right padding, in px.
pub const GUTTER_PAD: f32 = 8.0;
/// Text left padding inside a diff column, in px.
pub const TEXT_PAD_L: f32 = 8.0;
/// Centre divider width, in px.
pub const DIVIDER_W: f32 = 1.0;
/// Monospace font for diff text. Vendored — see `assets/fonts` and
/// `pm::fonts::load`.
pub const BODY_FONT: &str = "JetBrains Mono";
pub const BODY_FONT_SIZE: f32 = 12.5;
/// Proportional font for UI chrome (sidebar, headers). Also vendored.
pub const UI_FONT: &str = "Roboto";

pub const SIDEBAR_MIN: f32 = 180.0;
/// The diff pane always keeps at least this many px.
pub const SIDEBAR_MAX_MARGIN: f32 = 320.0;
pub const SECTION_HEADER_H: f32 = 26.0;
pub const SECTION_SPLIT_H: f32 = 6.0;
pub const RESIZE_HANDLE_W: f32 = 6.0;
pub const DIFF_SPLIT_MIN: f32 = 0.15;
pub const DIFF_SPLIT_MAX: f32 = 0.85;

/// Custom title bar height, in px (matches Zed's Windows title bar).
pub const TITLE_BAR_H: f32 = 32.0;
/// Status bar height, in px.
pub const STATUS_BAR_H: f32 = 24.0;
/// Corner radius for client-side window decorations (Linux), in px.
pub const CLIENT_DECORATION_ROUNDING: f32 = 10.0;
/// Shadow / resize-grip inset for client-side decorations (Linux), in px.
pub const CLIENT_DECORATION_SHADOW: f32 = 10.0;
/// Hover fill for the window close button.
pub const CLOSE_HOVER: u32 = 0xc42b1c;
