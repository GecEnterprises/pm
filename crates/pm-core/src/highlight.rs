//! Syntax highlighting via `syntect`. A file's text is turned into per-line
//! runs of `(text, color)` that the diff view renders as coloured spans.

use std::path::Path;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Color, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// An RGBA colour, components in `0.0..=1.0`. Mirrors `gpui::Rgba`'s layout;
/// `pm-ui` converts at the paint boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// A single coloured run within a line.
pub struct Span {
    pub text: String,
    pub color: Rgba,
}

/// One source line, split into coloured spans. Empty for a blank line.
pub type Line = Vec<Span>;

/// Files larger than this are shown without highlighting (syntect is eager).
const MAX_BYTES: usize = 2 * 1024 * 1024;

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
}

impl Highlighter {
    pub fn new() -> Self {
        let mut themes = ThemeSet::load_defaults();
        let theme = themes
            .themes
            .remove("base16-eighties.dark")
            .expect("bundled theme present");
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            theme,
        }
    }

    /// Highlight `text`, choosing a grammar from `path`'s extension.
    pub fn highlight(&self, path: &Path, text: &str) -> Vec<Line> {
        if text.len() > MAX_BYTES {
            return text.lines().map(|l| plain_line(l)).collect();
        }

        let syntax = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|ext| self.syntaxes.find_syntax_by_extension(ext))
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());

        let mut hl = HighlightLines::new(syntax, &self.theme);
        let mut lines = Vec::new();
        for raw in LinesWithEndings::from(text) {
            let spans = match hl.highlight_line(raw, &self.syntaxes) {
                Ok(runs) => runs
                    .into_iter()
                    .map(|(style, piece)| Span {
                        text: piece.trim_end_matches('\n').to_string(),
                        color: to_rgba(style.foreground),
                    })
                    .filter(|s| !s.text.is_empty())
                    .collect(),
                Err(_) => plain_line(raw.trim_end_matches('\n')),
            };
            lines.push(spans);
        }
        lines
    }
}

fn plain_line(text: &str) -> Line {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Span {
            text: text.to_string(),
            color: Rgba {
                r: 0.83,
                g: 0.83,
                b: 0.83,
                a: 1.0,
            },
        }]
    }
}

fn to_rgba(c: Color) -> Rgba {
    Rgba {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a: 1.0,
    }
}
