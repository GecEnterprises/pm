//! pm's markdown rendering — a preset over gpui-component's `TextView`
//! (PM-55 pattern A), wired to pm's own syntect highlighter for fenced code
//! blocks.
//!
//! The highlighter matters more than it looks: pm already highlights every
//! file it renders in the diff view via `pm_core::highlight::Highlighter`
//! (syntect). The kit ships its own tree-sitter highlighter for markdown code
//! blocks. Left alone, a Rust snippet pasted into a ticket body and the same
//! Rust file in the diff view would be colored by two different engines and
//! could disagree. Routing both through the same `Highlighter` keeps them
//! consistent — see PM-9.

use std::ops::Range;
use std::sync::Arc;

use gpui::{App, Global, HighlightStyle, Hsla, SharedString};
use gpui_base::text::{markdown, CodeBlock, TextView, TextViewStyle};
use pm_core::highlight::{Highlighter, Rgba};

use crate::theme::ActiveTheme as _;

struct MarkdownHighlighter(Arc<Highlighter>);
impl Global for MarkdownHighlighter {}

/// Load pm's syntect highlighter for markdown code blocks. Call once at app
/// startup, alongside [`crate::theme::init`].
pub fn init(cx: &mut App) {
    cx.set_global(MarkdownHighlighter(Arc::new(Highlighter::new())));
}

/// Render `source` as pm-styled markdown.
///
/// The highlighter and the code styling are both set per-instance
/// (`TextView::code_block_highlighter` / `TextView::style`) rather than
/// through the kit's global `TextViewDefaults`, deliberately:
/// `gpui_component::Theme::change`/`sync_base` (run by every
/// [`crate::theme::set_theme`] call, i.e. on every new window) reinstalls
/// `TextViewDefaults` wholesale, which would silently drop anything installed
/// globally back to the kit's defaults. A per-instance override has no such
/// lifecycle to race.
#[track_caller]
pub fn view(source: impl Into<SharedString>, cx: &App) -> TextView {
    let hl = cx.global::<MarkdownHighlighter>().0.clone();
    markdown(source)
        .code_block_highlighter(move |block| highlight_code_block(&hl, block))
        .style(code_style(cx))
}

/// Give inline code and fenced code blocks their own muted, recessed
/// background instead of the kit's default, which derives both from
/// `theme.accent` — the same blue pm uses for selected rows and text
/// selection. Left alone, every code span reads as a highlighted/active UI
/// element instead of code. `code_background` covers fenced blocks;
/// `inline_code` covers single-backtick spans — both get the same tint so
/// they read as one visual language.
///
/// Starts from the kit's own theme-derived style (`TextViewStyle::from_theme`)
/// so foreground/link/selection/border stay whatever `project_onto_kit`
/// already set, and only the code-specific fields are overridden.
fn code_style(cx: &App) -> TextViewStyle {
    let base_theme = gpui_base::Theme::global(cx);
    let style = TextViewStyle::from_theme(&base_theme);

    let theme = cx.theme();
    // `panel` (pm's *raised* surface — sidebar, popovers) is lighter than
    // `bg`, which is backwards for code: a code block should read as recessed
    // below the page, not raised above it. Darken `bg` itself instead.
    let code_bg = darken(theme.colors.bg, 0.55);

    let radius = gpui::px(4.0);
    let mut code_block = gpui::StyleRefinement::default();
    code_block.corner_radii.top_left = Some(radius.into());
    code_block.corner_radii.top_right = Some(radius.into());
    code_block.corner_radii.bottom_left = Some(radius.into());
    code_block.corner_radii.bottom_right = Some(radius.into());

    style
        .with_code_background(code_bg)
        .with_inline_code(HighlightStyle { background_color: Some(code_bg), ..Default::default() })
        .with_code_block(code_block)
}

/// Scale `color`'s lightness by `factor` (`0.0..=1.0`, lower = darker),
/// keeping hue and saturation. Multiplicative rather than a fixed subtraction
/// so it behaves sanely on both a near-black dark-theme `bg` and, later, a
/// light theme's near-white one (PM-18).
fn darken(color: Hsla, factor: f32) -> Hsla {
    Hsla { l: (color.l * factor).clamp(0.0, 1.0), ..color }
}

fn highlight_code_block(hl: &Highlighter, block: &CodeBlock) -> Vec<(Range<usize>, HighlightStyle)> {
    let code = block.code();
    hl.highlight_by_lang(block.lang().as_deref(), &code)
        .into_iter()
        .map(|(range, color)| {
            (
                range,
                HighlightStyle { color: Some(to_hsla(color)), ..Default::default() },
            )
        })
        .collect()
}

fn to_hsla(c: Rgba) -> Hsla {
    gpui::Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into()
}
