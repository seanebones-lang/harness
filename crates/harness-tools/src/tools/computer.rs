//! Anthropic computer-use tool (computer-use-2025-01-24 spec).
//!
//! Implements: screenshot, mouse_move, left_click, right_click, double_click,
//! type_text, key (press keyboard shortcut), scroll.
//!
//! SAFETY: Only registered when `[computer_use] enabled = true` AND the model is Claude 4.7+.
//! The TUI shows a red `[COMPUTER USE LIVE]` banner whenever this tool is active.
//!
//! Implementation uses system commands to avoid heavy native dependencies:
//! - Screenshot: `screencapture` (macOS), `scrot` (Linux), `maim` (Linux)
//! - Mouse/keyboard: `xdotool` (Linux), `cliclick` (macOS), `enigo` would require native libs

use anyhow::{Context, Result};
use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::{debug, warn};

use crate::registry::Tool;

/// The Anthropic computer-use tool implementation.
pub struct ComputerUseTool;

#[async_trait]
impl Tool for ComputerUseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "computer",
            "Control the computer: take screenshots, move the mouse, click, type, scroll, and press keys. \
             ⚠️  This tool has direct access to your screen and input devices. \
             Actions: screenshot, mouse_move, left_click, right_click, double_click, middle_click, \
             type, key, scroll, cursor_position.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "screenshot",
                            "cursor_position",
                            "mouse_move",
                            "left_click",
                            "right_click",
                            "double_click",
                            "middle_click",
                            "type",
                            "key",
                            "scroll"
                        ],
                        "description": "The computer action to perform"
                    },
                    "coordinate": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "minItems": 2,
                        "maxItems": 2,
                        "description": "[x, y] screen coordinates for mouse actions"
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to type (for 'type' action) or key combo (for 'key' action, e.g. 'ctrl+c')"
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["up", "down", "left", "right"],
                        "description": "Scroll direction"
                    },
                    "amount": {
                        "type": "integer",
                        "description": "Scroll amount in clicks"
                    }
                },
                "required": ["action"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("screenshot");
        debug!(action, "computer use execute");

        match action {
            "screenshot" => take_screenshot().await,
            "cursor_position" => get_cursor_position().await,
            "mouse_move" => {
                let (x, y) = extract_coord(&args)?;
                mouse_move(x, y).await
            }
            "left_click" => {
                let (x, y) = extract_coord(&args)?;
                mouse_click(x, y, "left").await
            }
            "right_click" => {
                let (x, y) = extract_coord(&args)?;
                mouse_click(x, y, "right").await
            }
            "double_click" => {
                let (x, y) = extract_coord(&args)?;
                mouse_double_click(x, y).await
            }
            "middle_click" => {
                let (x, y) = extract_coord(&args)?;
                mouse_click(x, y, "middle").await
            }
            "type" => {
                let text = args["text"].as_str().unwrap_or("").to_string();
                type_text(&text).await
            }
            "key" => {
                let text = args["text"].as_str().unwrap_or("").to_string();
                press_key(&text).await
            }
            "scroll" => {
                let (x, y) = extract_coord(&args)?;
                let direction = args["direction"].as_str().unwrap_or("down");
                let amount = args["amount"].as_i64().unwrap_or(3) as i32;
                scroll(x, y, direction, amount).await
            }
            _ => anyhow::bail!("Unknown computer action: {action}"),
        }
    }
}

fn extract_coord(args: &Value) -> Result<(i32, i32)> {
    let coord = args["coordinate"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("'coordinate' array is required for this action"))?;
    let x = coord.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y = coord.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    Ok((x, y))
}

// ── macOS implementations ────────────────────────────────────────────────────

async fn take_screenshot() -> Result<String> {
    let tmp = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .context("creating temp file for screenshot")?;
    let path = tmp.path().to_path_buf();
    let path_str = path.to_str().unwrap_or("screen.png");

    let captured = if cfg!(target_os = "macos") {
        Command::new("screencapture")
            .args(["-x", "-t", "png", path_str])
            .status()
            .await
            .context("screencapture failed")?
            .success()
    } else {
        // Linux: try scrot, then maim
        if Command::new("which")
            .arg("scrot")
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
        {
            Command::new("scrot")
                .arg(path_str)
                .status()
                .await
                .context("scrot failed")?
                .success()
        } else if Command::new("which")
            .arg("maim")
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
        {
            Command::new("maim")
                .arg(path_str)
                .status()
                .await
                .context("maim failed")?
                .success()
        } else {
            anyhow::bail!("No screenshot tool found. Install: brew install --cask flameshot (or scrot/maim on Linux)");
        }
    };

    if !captured {
        anyhow::bail!("Screenshot capture failed");
    }

    // Read and base64-encode
    let bytes = tokio::fs::read(&path).await.context("reading screenshot")?;
    let b64 = encode_base64(&bytes);
    Ok(format!("data:image/png;base64,{b64}"))
}

async fn get_cursor_position() -> Result<String> {
    if cfg!(target_os = "macos") {
        // Use cliclick if available
        if let Ok(out) = Command::new("cliclick").arg("p:.").output().await {
            let pos = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok(format!("Cursor position: {pos}"));
        }
    }
    Ok("Cursor position: unknown (install cliclick on macOS: brew install cliclick)".to_string())
}

async fn mouse_move(x: i32, y: i32) -> Result<String> {
    if cfg!(target_os = "macos") {
        if Command::new("cliclick")
            .arg(format!("m:{x},{y}"))
            .status()
            .await
            .is_ok()
        {
            return Ok(format!("Mouse moved to {x},{y}"));
        }
    } else if Command::new("which")
        .arg("xdotool")
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
    {
        Command::new("xdotool")
            .args(["mousemove", &x.to_string(), &y.to_string()])
            .status()
            .await
            .ok();
        return Ok(format!("Mouse moved to {x},{y}"));
    }
    warn!("No mouse control tool available (install cliclick on macOS, xdotool on Linux)");
    Ok(format!("Mouse move to {x},{y} — no control tool available"))
}

async fn mouse_click(x: i32, y: i32, button: &str) -> Result<String> {
    if cfg!(target_os = "macos") {
        let cliclick_btn = match button {
            "right" => "rc",
            "middle" => "mc",
            _ => "c",
        };
        if Command::new("cliclick")
            .arg(format!("{cliclick_btn}:{x},{y}"))
            .status()
            .await
            .is_ok()
        {
            return Ok(format!("{button} click at {x},{y}"));
        }
    } else if Command::new("which")
        .arg("xdotool")
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
    {
        let btn = match button {
            "right" => "3",
            "middle" => "2",
            _ => "1",
        };
        Command::new("xdotool")
            .args(["mousemove", &x.to_string(), &y.to_string(), "click", btn])
            .status()
            .await
            .ok();
        return Ok(format!("{button} click at {x},{y}"));
    }
    Ok(format!(
        "{button} click at {x},{y} — no control tool available"
    ))
}

async fn mouse_double_click(x: i32, y: i32) -> Result<String> {
    if cfg!(target_os = "macos") {
        if Command::new("cliclick")
            .arg(format!("dc:{x},{y}"))
            .status()
            .await
            .is_ok()
        {
            return Ok(format!("Double click at {x},{y}"));
        }
    } else if Command::new("which")
        .arg("xdotool")
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
    {
        Command::new("xdotool")
            .args([
                "mousemove",
                &x.to_string(),
                &y.to_string(),
                "click",
                "--repeat",
                "2",
                "1",
            ])
            .status()
            .await
            .ok();
        return Ok(format!("Double click at {x},{y}"));
    }
    Ok(format!(
        "Double click at {x},{y} — no control tool available"
    ))
}

async fn type_text(text: &str) -> Result<String> {
    if cfg!(target_os = "macos") {
        if Command::new("cliclick")
            .arg(format!("t:{text}"))
            .status()
            .await
            .is_ok()
        {
            let preview: String = text.chars().take(40).collect();
            return Ok(format!("Typed: {preview}"));
        }
    } else if Command::new("which")
        .arg("xdotool")
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
    {
        Command::new("xdotool")
            .args(["type", "--clearmodifiers", text])
            .status()
            .await
            .ok();
        let preview: String = text.chars().take(40).collect();
        return Ok(format!("Typed: {preview}"));
    }
    Ok(format!(
        "Type '{}' — no control tool available",
        &text[..text.len().min(40)]
    ))
}

async fn press_key(key: &str) -> Result<String> {
    // Normalise key combo (e.g. "ctrl+c" → platform-specific)
    if cfg!(target_os = "macos") {
        // cliclick uses kc: for key codes; use osascript for combos
        let script =
            format!("tell application \"System Events\" to keystroke \"{key}\" using {{}}",);
        // For simple keys, try cliclick
        if Command::new("cliclick")
            .arg(format!("kp:{key}"))
            .status()
            .await
            .is_ok()
        {
            return Ok(format!("Key pressed: {key}"));
        }
        // Fall back to osascript (works for combos like "cmd+c")
        let _ = Command::new("osascript")
            .args(["-e", &script])
            .status()
            .await;
        return Ok(format!("Key pressed: {key}"));
    } else if Command::new("which")
        .arg("xdotool")
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
    {
        let xdo_key = key.replace("cmd+", "super+");
        Command::new("xdotool")
            .args(["key", &xdo_key])
            .status()
            .await
            .ok();
        return Ok(format!("Key pressed: {key}"));
    }
    Ok(format!("Key press {key} — no control tool available"))
}

async fn scroll(x: i32, y: i32, direction: &str, amount: i32) -> Result<String> {
    if cfg!(target_os = "macos") {
        let (dx, dy) = match direction {
            "up" => (0, amount),
            "down" => (0, -amount),
            "left" => (-amount, 0),
            "right" => (amount, 0),
            _ => (0, -amount),
        };
        // Move to position first, then scroll
        let _ = Command::new("cliclick")
            .arg(format!("m:{x},{y}"))
            .status()
            .await;
        if Command::new("cliclick")
            .arg(format!("w:{dx},{dy}"))
            .status()
            .await
            .is_ok()
        {
            return Ok(format!("Scrolled {direction} {amount}x at {x},{y}"));
        }
    } else if Command::new("which")
        .arg("xdotool")
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
    {
        let btn = match direction {
            "up" => "4",
            "left" => "6",
            "right" => "7",
            _ => "5",
        };
        for _ in 0..amount {
            Command::new("xdotool")
                .args(["mousemove", &x.to_string(), &y.to_string(), "click", btn])
                .status()
                .await
                .ok();
        }
        return Ok(format!("Scrolled {direction} {amount}x at {x},{y}"));
    }
    Ok(format!(
        "Scroll {direction} at {x},{y} — no control tool available"
    ))
}

fn encode_base64(data: &[u8]) -> String {
    use std::fmt::Write;
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 {
            chunk[1] as usize
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as usize
        } else {
            0
        };
        let _ = write!(out, "{}", TABLE[(b0 >> 2) & 63] as char);
        let _ = write!(out, "{}", TABLE[((b0 << 4) | (b1 >> 4)) & 63] as char);
        let _ = write!(
            out,
            "{}",
            if chunk.len() > 1 {
                TABLE[((b1 << 2) | (b2 >> 6)) & 63] as char
            } else {
                '='
            }
        );
        let _ = write!(
            out,
            "{}",
            if chunk.len() > 2 {
                TABLE[b2 & 63] as char
            } else {
                '='
            }
        );
    }
    out
}
