//! Colours and pixel metrics for the pm UI.

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
/// Monospace font for diff text.
pub const BODY_FONT: &str = "Consolas";
pub const BODY_FONT_SIZE: f32 = 12.5;
/// Hard cap on the number of diff rows laid out.

pub const SIDEBAR_MIN: f32 = 180.0;
/// The diff pane always keeps at least this many px.
pub const SIDEBAR_MAX_MARGIN: f32 = 320.0;
pub const SECTION_HEADER_H: f32 = 26.0;
pub const SECTION_SPLIT_H: f32 = 6.0;
pub const RESIZE_HANDLE_W: f32 = 6.0;
pub const DIFF_SPLIT_MIN: f32 = 0.15;
pub const DIFF_SPLIT_MAX: f32 = 0.85;
