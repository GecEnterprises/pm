//! pm's concrete palette + pixel metrics, built as a [`fremantle::theme::Theme`]
//! and installed as the active theme in [`crate::app::Pm::new`]. Read anywhere
//! via `cx.theme()` ([`ActiveTheme`], re-exported below so existing
//! `use crate::theme::*;` call sites pick it up automatically).

pub use fremantle::theme::{ActiveTheme, Appearance, Theme, ThemeColors, ThemeMetrics};

/// pm's dark theme: every literal value here is unchanged from the old flat
/// consts this module used to define.
pub fn build() -> Theme {
    Theme {
        appearance: Appearance::Dark,
        colors: ThemeColors {
            bg: gpui::rgb(0x1e1e1e).into(),
            panel: gpui::rgb(0x252526).into(),
            border: gpui::rgb(0x333333).into(),
            text: gpui::rgb(0xd4d4d4).into(),
            dim: gpui::rgb(0x808080).into(),
            select: gpui::rgb(0x094771).into(),
            add_bg: gpui::rgb(0x18321f).into(),
            del_bg: gpui::rgb(0x3a1d1d).into(),
            // Tint for changed files / dirs in the Explorer tree.
            changed: gpui::rgb(0x4ec9b0).into(),
            // Hover fill for the window close button.
            close_hover: gpui::rgb(0xc42b1c).into(),
        },
        metrics: ThemeMetrics {
            // Diff row height / line-height, in px.
            row_h: 18.0,
            // "Changes" list row height, in px.
            list_row_h: 24.0,
            // Explorer tree row height, in px.
            tree_row_h: 22.0,
            // Explorer indent per depth level, in px.
            tree_indent: 14.0,
            // File-type icon size, in px.
            icon_size: 14.0,
            // Scrollbar track thickness, in px.
            bar: 12.0,
            // Line-number column width, in px.
            gutter_w: 52.0,
            // Line-number column right padding, in px.
            gutter_pad: 8.0,
            // Text left padding inside a diff column, in px.
            text_pad_l: 8.0,
            // Centre divider width, in px.
            divider_w: 1.0,
            sidebar_min: 180.0,
            // The diff pane always keeps at least this many px.
            sidebar_max_margin: 320.0,
            section_header_h: 26.0,
            section_split_h: 6.0,
            resize_handle_w: 6.0,
            diff_split_min: 0.15,
            diff_split_max: 0.85,
            // Custom title bar height, in px (matches Zed's Windows title bar).
            title_bar_h: 32.0,
            // Status bar height, in px.
            status_bar_h: 24.0,
            // Corner radius for client-side window decorations (Linux), in px.
            client_decoration_rounding: 10.0,
            // Shadow / resize-grip inset for client-side decorations (Linux), in px.
            client_decoration_shadow: 10.0,
            // Monospace font for diff text. Vendored — see `assets/fonts` and
            // `pm::fonts::load`.
            body_font: "JetBrains Mono",
            body_font_size: 12.5,
            // Proportional font for UI chrome (sidebar, headers). Also vendored.
            ui_font: "Roboto",
        },
        base_rem: 16.0,
    }
}
