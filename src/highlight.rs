//! Syntax-highlighted code rendering for the TUI chat panel.
//! Parses fenced code blocks in assistant messages, highlights them with
//! syntect (base16-ocean.dark theme), and returns ratatui Lines.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines,
    highlighting::ThemeSet,
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

// ── Content block ─────────────────────────────────────────────────────────────

pub enum Block {
    Text(String),
    Code { lang: String, code: String },
}

/// Parse a message string into alternating text / fenced-code blocks.
pub fn parse_blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut in_code = false;
    let mut lang = String::new();
    let mut code_buf = String::new();
    let mut text_buf = String::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("```") {
            if in_code {
                if !code_buf.is_empty() {
                    blocks.push(Block::Code { lang: lang.clone(), code: code_buf.clone() });
                }
                code_buf.clear();
                in_code = false;
            } else {
                if !text_buf.is_empty() {
                    blocks.push(Block::Text(text_buf.clone()));
                    text_buf.clear();
                }
                lang = rest.trim().to_string();
                in_code = true;
            }
        } else if in_code {
            code_buf.push_str(line);
            code_buf.push('\n');
        } else {
            text_buf.push_str(line);
            text_buf.push('\n');
        }
    }

    if !text_buf.trim().is_empty() {
        blocks.push(Block::Text(text_buf));
    }
    if in_code && !code_buf.is_empty() {
        // Unclosed fence — treat as code anyway
        blocks.push(Block::Code { lang, code: code_buf });
    }

    blocks
}

// ── Highlighter ───────────────────────────────────────────────────────────────

/// Lazily-initialised syntax highlighter. Create once, reuse per frame.
pub struct Highlighter {
    ss: SyntaxSet,
    ts: ThemeSet,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            ss: SyntaxSet::load_defaults_newlines(),
            ts: ThemeSet::load_defaults(),
        }
    }

    /// Render a code string as ratatui Lines with syntax colouring.
    pub fn highlight_code(&self, code: &str, lang: &str) -> Vec<Line<'static>> {
        let syntax = self
            .ss
            .find_syntax_by_token(lang)
            .unwrap_or_else(|| self.ss.find_syntax_plain_text());

        let theme = &self.ts.themes["base16-ocean.dark"];
        let mut hl = HighlightLines::new(syntax, theme);

        let mut lines: Vec<Line<'static>> = Vec::new();

        // Code block header  e.g. "  rust"
        if !lang.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  {lang}"),
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )));
        }

        for line_str in LinesWithEndings::from(code) {
            let ranges = hl
                .highlight_line(line_str, &self.ss)
                .unwrap_or_default();

            let spans: Vec<Span<'static>> = ranges
                .iter()
                .map(|(syn_style, text)| {
                    let fg = syn_color_to_ratatui(syn_style.foreground);
                    Span::styled(text.to_string(), Style::default().fg(fg))
                })
                .collect();

            lines.push(Line::from(spans));
        }

        lines
    }

    /// Render a full message (with possible code blocks) as ratatui Lines.
    pub fn render_message(&self, text: &str, text_style: Style) -> Vec<Line<'static>> {
        let blocks = parse_blocks(text);
        let mut lines: Vec<Line<'static>> = Vec::new();

        for block in blocks {
            match block {
                Block::Text(t) => {
                    for raw in t.lines() {
                        lines.push(Line::from(Span::styled(
                            raw.to_string(),
                            text_style,
                        )));
                    }
                }
                Block::Code { lang, code } => {
                    // Divider above code block
                    lines.push(Line::from(Span::styled(
                        "  ─────────────────────",
                        Style::default().fg(Color::DarkGray),
                    )));
                    lines.extend(self.highlight_code(&code, &lang));
                    lines.push(Line::from(Span::styled(
                        "  ─────────────────────",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
        }

        lines
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

// ── Color conversion ──────────────────────────────────────────────────────────

fn syn_color_to_ratatui(c: syntect::highlighting::Color) -> Color {
    // syntect uses RGBA; map straight to ratatui RGB
    Color::Rgb(c.r, c.g, c.b)
}
