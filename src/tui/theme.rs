//! TUI color theme loaded from `~/.harness/theme.toml`.

use std::path::Path;

use ratatui::style::Color;

#[derive(Clone)]
pub(crate) struct Theme {
    pub(crate) user_color: Color,
    pub(crate) assistant_color: Color,
    pub(crate) streaming_color: Color,
    pub(crate) error_color: Color,
    pub(crate) tool_in_color: Color,
    pub(crate) tool_out_color: Color,
    pub(crate) dim_color: Color,
    pub(crate) border_color: Color,
    pub(crate) accent_color: Color,
    pub(crate) search_hl_color: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            user_color: Color::Cyan,
            assistant_color: Color::Green,
            streaming_color: Color::Yellow,
            error_color: Color::Red,
            tool_in_color: Color::Magenta,
            tool_out_color: Color::Blue,
            dim_color: Color::DarkGray,
            border_color: Color::Gray,
            accent_color: Color::Cyan,
            search_hl_color: Color::LightYellow,
        }
    }
}

impl Theme {
    pub(crate) fn load() -> Self {
        let path = dirs::home_dir()
            .unwrap_or_default()
            .join(".harness/theme.toml");
        Self::load_from_path(&path)
    }

    /// Load theme from an explicit path (missing/invalid → defaults).
    pub(crate) fn load_from_path(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        Self::load_from_str(&text)
    }

    /// Parse theme TOML body (invalid → defaults).
    pub(crate) fn load_from_str(text: &str) -> Self {
        let Ok(val) = text.parse::<toml::Value>() else {
            return Self::default();
        };
        let get = |key: &str, def: Color| -> Color {
            val.get(key)
                .and_then(|v| v.as_str())
                .and_then(parse_color)
                .unwrap_or(def)
        };
        Self {
            user_color: get("user", Color::Cyan),
            assistant_color: get("assistant", Color::Green),
            streaming_color: get("streaming", Color::Yellow),
            error_color: get("error", Color::Red),
            tool_in_color: get("tool_in", Color::Magenta),
            tool_out_color: get("tool_out", Color::Blue),
            dim_color: get("dim", Color::DarkGray),
            border_color: get("border", Color::Gray),
            accent_color: get("accent", Color::Cyan),
            search_hl_color: get("search_hl", Color::LightYellow),
        }
    }

    pub(crate) fn assistant_label<'a>(&self, model: &str) -> &'a str {
        assistant_label_for_model(model)
    }
}

/// Map model id substring → short transcript label.
pub(crate) fn assistant_label_for_model(model: &str) -> &'static str {
    if model.contains("claude") {
        "claude"
    } else if model.contains("grok") {
        "grok"
    } else if model.contains("gpt") {
        "gpt"
    } else if model.contains("qwen") {
        "qwen"
    } else {
        "ai"
    }
}

pub(crate) fn parse_color(s: &str) -> Option<Color> {
    match s.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "lightyellow" => Some(Color::LightYellow),
        "lightcyan" => Some(Color::LightCyan),
        "lightgreen" => Some(Color::LightGreen),
        _ => None,
    }
}

/// Truncate tool result previews for resume chat rows.
pub(crate) fn tool_result_preview(result: &str, max_chars: usize) -> String {
    if result.len() > max_chars {
        format!("{}… ({} bytes)", &result[..max_chars], result.len())
    } else {
        result.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_color_named_and_unknown() {
        assert_eq!(parse_color("RED"), Some(Color::Red));
        assert_eq!(parse_color("grey"), Some(Color::Gray));
        assert_eq!(parse_color("darkgrey"), Some(Color::DarkGray));
        assert_eq!(parse_color("lightyellow"), Some(Color::LightYellow));
        assert_eq!(parse_color("not-a-color"), None);
        assert_eq!(parse_color(""), None);
    }

    #[test]
    fn assistant_label_for_model_matrix() {
        assert_eq!(assistant_label_for_model("claude-sonnet-4-6"), "claude");
        assert_eq!(assistant_label_for_model("grok-4.5"), "grok");
        assert_eq!(assistant_label_for_model("gpt-5.5"), "gpt");
        assert_eq!(assistant_label_for_model("qwen2.5-coder"), "qwen");
        assert_eq!(assistant_label_for_model("mistral-large"), "ai");
        let t = Theme::default();
        assert_eq!(t.assistant_label("grok-4.5"), "grok");
    }

    #[test]
    fn load_from_str_overrides_and_invalid_defaults() {
        let t = Theme::load_from_str(
            r#"
user = "red"
assistant = "blue"
unknown_key = "green"
streaming = "not-a-color"
"#,
        );
        assert_eq!(t.user_color, Color::Red);
        assert_eq!(t.assistant_color, Color::Blue);
        // invalid color falls back to default yellow
        assert_eq!(t.streaming_color, Color::Yellow);

        let bad = Theme::load_from_str("{{{not toml");
        assert_eq!(bad.user_color, Theme::default().user_color);
    }

    #[test]
    fn load_from_path_missing_and_valid() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.toml");
        assert_eq!(
            Theme::load_from_path(&missing).user_color,
            Theme::default().user_color
        );

        let path = dir.path().join("theme.toml");
        fs::write(&path, "error = \"magenta\"\n").unwrap();
        let t = Theme::load_from_path(&path);
        assert_eq!(t.error_color, Color::Magenta);
        assert_eq!(t.user_color, Color::Cyan); // default
    }
}
