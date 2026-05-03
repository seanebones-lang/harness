//! TUI color theme loaded from `~/.harness/theme.toml`.

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
        if !path.exists() {
            return Self::default();
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
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
