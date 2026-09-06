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
use gpui_base::text::{markdown, CodeBlock, TextView};
use pm_core::highlight::{Highlighter, Rgba};

struct MarkdownHighlighter(Arc<Highlighter>);
impl Global for MarkdownHighlighter {}

/// Load pm's syntect highlighter for markdown code blocks. Call once at app
/// startup, alongside [`crate::theme::init`].
pub fn init(cx: &mut App) {
    cx.set_global(MarkdownHighlighter(Arc::new(Highlighter::new())));
}

/// Render `source` as pm-styled markdown.
///
/// The highlighter is set per-instance (`TextView::code_block_highlighter`)
/// rather than through the kit's global `TextViewDefaults`, deliberately:
/// `gpui_component::Theme::change`/`sync_base` (run by every
/// [`crate::theme::set_theme`] call, i.e. on every new window) reinstalls
/// `TextViewDefaults` wholesale, which would silently drop a
/// globally-installed highlighter back to the kit's default. A per-instance
/// override has no such lifecycle to race.
#[track_caller]
pub fn view(source: impl Into<SharedString>, cx: &App) -> TextView {
    let hl = cx.global::<MarkdownHighlighter>().0.clone();
    markdown(source).code_block_highlighter(move |block| highlight_code_block(&hl, block))
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
