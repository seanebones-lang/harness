//! Syntax-highlighted code rendering for the TUI chat panel.
//! Parses fenced code blocks in assistant messages, highlights them with
//! syntect (base16-ocean.dark theme), and returns ratatui Lines.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet, util::LinesWithEndings,
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
                    blocks.push(Block::Code {
                        lang: lang.clone(),
                        code: code_buf.clone(),
                    });
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
        blocks.push(Block::Code {
            lang,
            code: code_buf,
        });
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
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        for line_str in LinesWithEndings::from(code) {
            let ranges = hl.highlight_line(line_str, &self.ss).unwrap_or_default();

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
                        lines.push(Line::from(Span::styled(raw.to_string(), text_style)));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn is_text(b: &Block) -> bool {
        matches!(b, Block::Text(_))
    }

    fn is_code(b: &Block) -> bool {
        matches!(b, Block::Code { .. })
    }

    #[test]
    fn parse_blocks_empty_input() {
        assert!(parse_blocks("").is_empty());
        assert!(parse_blocks("   \n\n").is_empty());
    }

    #[test]
    fn parse_blocks_plain_text_only() {
        let blocks = parse_blocks("hello\nworld\n");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Text(t) => assert!(t.contains("hello") && t.contains("world")),
            Block::Code { .. } => panic!("expected text"),
        }
    }

    #[test]
    fn parse_blocks_single_fenced_rust() {
        let src = "intro\n```rust\nfn main() {}\n```\noutro\n";
        let blocks = parse_blocks(src);
        assert_eq!(blocks.len(), 3);
        assert!(is_text(&blocks[0]));
        match &blocks[1] {
            Block::Code { lang, code } => {
                assert_eq!(lang, "rust");
                assert!(code.contains("fn main()"));
            }
            Block::Text(_) => panic!("expected code"),
        }
        assert!(is_text(&blocks[2]));
    }

    #[test]
    fn parse_blocks_empty_lang_token() {
        let blocks = parse_blocks("```\nprint(1)\n```\n");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Code { lang, code } => {
                assert!(lang.is_empty());
                assert!(code.contains("print(1)"));
            }
            Block::Text(_) => panic!("expected code"),
        }
    }

    #[test]
    fn parse_blocks_unclosed_fence_treated_as_code() {
        let blocks = parse_blocks("before\n```py\nx = 1\n");
        assert!(blocks.iter().any(is_code));
        let code = blocks.iter().find_map(|b| match b {
            Block::Code { lang, code } => Some((lang.as_str(), code.as_str())),
            _ => None,
        });
        let (lang, code) = code.expect("unclosed code");
        assert_eq!(lang, "py");
        assert!(code.contains("x = 1"));
    }

    #[test]
    fn parse_blocks_empty_fence_does_not_emit_code() {
        // Opening + closing with no body → no Code block
        let blocks = parse_blocks("pre\n```rs\n```\npost\n");
        assert!(blocks.iter().all(is_text));
        assert!(!blocks.iter().any(is_code));
    }

    #[test]
    fn parse_blocks_multiple_fences() {
        let src = "```a\n1\n```\nmid\n```b\n2\n```\n";
        let blocks = parse_blocks(src);
        let codes: Vec<_> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Code { lang, code } => Some((lang.clone(), code.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(codes.len(), 2);
        assert_eq!(codes[0].0, "a");
        assert!(codes[0].1.contains('1'));
        assert_eq!(codes[1].0, "b");
        assert!(codes[1].1.contains('2'));
    }

    #[test]
    fn parse_blocks_lang_is_trimmed() {
        let blocks = parse_blocks("```  go  \npackage main\n```\n");
        match &blocks[0] {
            Block::Code { lang, .. } => assert_eq!(lang, "go"),
            Block::Text(_) => panic!("expected code"),
        }
    }

    #[test]
    fn highlighter_new_and_default() {
        let a = Highlighter::new();
        let b = Highlighter::default();
        // Both construct usable highlighters
        assert!(!a.highlight_code("x = 1\n", "python").is_empty());
        assert!(!b.highlight_code("x = 1\n", "python").is_empty());
    }

    #[test]
    fn highlight_code_lang_header_when_lang_set() {
        let hl = Highlighter::new();
        let lines = hl.highlight_code("let x = 1;\n", "rust");
        assert!(!lines.is_empty());
        // First line is the italic lang label "  rust"
        let header: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(header, "  rust");
        assert!(lines.len() >= 2);
    }

    #[test]
    fn highlight_code_no_header_when_lang_empty() {
        let hl = Highlighter::new();
        let lines = hl.highlight_code("plain\n", "");
        assert!(!lines.is_empty());
        let first: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_ne!(first.trim(), "");
        assert!(!first.contains("  ")); // no "  {lang}" header line content pattern forced
                                        // empty lang must not inject a DarkGray italic header-only line
        assert!(
            first.contains("plain")
                || lines
                    .iter()
                    .any(|l| { l.spans.iter().any(|s| s.content.contains("plain")) })
        );
    }

    #[test]
    fn highlight_code_unknown_lang_falls_back_to_plain() {
        let hl = Highlighter::new();
        let lines = hl.highlight_code("not_a_real_lang_token xyz\n", "not-a-real-syntax-xyz");
        assert!(!lines.is_empty());
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect::<Vec<_>>()
            .join("");
        assert!(joined.contains("not_a_real_lang_token") || joined.contains("xyz"));
    }

    #[test]
    fn highlight_code_empty_source() {
        let hl = Highlighter::new();
        let lines = hl.highlight_code("", "rust");
        // Only the language header line
        assert_eq!(lines.len(), 1);
        let header: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(header, "  rust");
    }

    #[test]
    fn render_message_wraps_code_with_dividers() {
        let hl = Highlighter::new();
        let style = Style::default().fg(Color::White);
        let lines = hl.render_message("hi\n```rs\nfn x(){}\n```\nbye\n", style);
        let texts: Vec<String> = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        assert!(texts.iter().any(|t| t == "hi"));
        assert!(texts.iter().any(|t| t == "bye"));
        let dividers = texts.iter().filter(|t| t.contains("─────")).count();
        assert!(dividers >= 2, "expected divider above and below code");
        assert!(texts.iter().any(|t| t.contains("rs") || t.contains("fn")));
    }

    #[test]
    fn render_message_text_only() {
        let hl = Highlighter::new();
        let style = Style::default();
        let lines = hl.render_message("one\ntwo\n", style);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn syn_color_to_ratatui_maps_rgb() {
        let c = syntect::highlighting::Color {
            r: 10,
            g: 20,
            b: 30,
            a: 255,
        };
        match syn_color_to_ratatui(c) {
            Color::Rgb(r, g, b) => {
                assert_eq!((r, g, b), (10, 20, 30));
            }
            other => panic!("expected Rgb, got {other:?}"),
        }
    }
}
