//! TUI rendering: pure functions that take `&AppState` (or `&mut AppState` for
//! scroll bookkeeping) and produce ratatui frames. No business logic here —
//! this module is the visual layer only.
//!
//! Extracted from `tui/mod.rs` (May 2026) as part of the god-file decomposition.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use crate::highlight::Highlighter;

use super::theme::Theme;
use super::{AppState, PendingConfirm};

pub(crate) fn draw_all(
    f: &mut ratatui::Frame,
    state: &mut AppState,
    hl: &Highlighter,
    theme: &Theme,
) {
    let area = f.area();

    let right_pct = state.right_panel_pct as u16;
    let left_pct = 100u16.saturating_sub(right_pct);

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),    // main panels
            Constraint::Length(4), // input box (taller for multi-line)
            Constraint::Length(1), // status bar
        ])
        .split(area);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(left_pct),
            Constraint::Percentage(right_pct),
        ])
        .split(root[0]);

    // Compute item counts BEFORE drawing so scroll bounds are up to date
    let chat_item_count = compute_chat_items(state);
    let event_item_count = state.event_log.len();
    state.chat_items_len = chat_item_count;
    state.event_items_len = event_item_count;

    draw_chat(f, state, main[0], hl, theme);
    draw_event_log(f, state, main[1], theme);
    draw_input(f, state, root[1], theme);
    draw_status(f, state, root[2], theme);

    // Overlays (drawn on top)
    if state.show_welcome {
        draw_welcome_overlay(f, theme);
        return;
    }

    // Slash autocomplete popup
    if !state.slash_suggestions.is_empty() {
        draw_slash_popup(f, state, root[1], theme);
    }

    // Search bar overlay (bottom of chat panel)
    if state.search_mode {
        draw_search_bar(f, state, main[0], theme);
    }

    if let Some(pc) = &state.pending_confirm {
        draw_confirm_overlay(f, pc, theme);
    }
}

fn compute_chat_items(state: &AppState) -> usize {
    // Estimate: each message = header line + content lines + blank
    state
        .chat
        .iter()
        .map(|m| 1 + m.content.lines().count() + 1)
        .sum::<usize>()
        + if !state.streaming.is_empty() {
            1 + state.streaming.lines().count()
        } else {
            0
        }
}

fn draw_chat(
    f: &mut ratatui::Frame,
    state: &mut AppState,
    area: Rect,
    hl: &Highlighter,
    theme: &Theme,
) {
    let mut items: Vec<ListItem> = Vec::new();
    let search_q = if state.search_mode {
        state.search_query.to_lowercase()
    } else {
        String::new()
    };

    for (msg_idx, msg) in state.chat.iter().enumerate() {
        let is_search_match = !search_q.is_empty() && state.search_matches.contains(&msg_idx);
        let (color, label) = match msg.role.as_str() {
            "user" => (theme.user_color, "you"),
            "assistant" => (theme.assistant_color, theme.assistant_label(&state.model)),
            _ => (theme.error_color, "err"),
        };
        let ts_str = if state.timestamps_visible {
            let elapsed = msg.ts.elapsed();
            let secs = state
                .session_start
                .elapsed()
                .as_secs()
                .saturating_sub(elapsed.as_secs());
            format!(" +{secs}s")
        } else {
            String::new()
        };
        let header_style = if is_search_match {
            Style::default()
                .fg(theme.search_hl_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        };
        items.push(ListItem::new(Line::from(Span::styled(
            format!("┌ [{label}]{ts_str}"),
            header_style,
        ))));

        if msg.role == "assistant" {
            let rendered = hl.render_message(&msg.content, Style::default().fg(Color::White));
            for line in rendered {
                items.push(ListItem::new(prefix_line(line, "│ ")));
            }
        } else {
            for raw in msg.content.lines() {
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("│ {raw}"),
                    Style::default().fg(Color::White),
                ))));
            }
        }
        items.push(ListItem::new(Line::from(Span::raw(""))));
    }

    // Streaming text
    if !state.streaming.is_empty() {
        let label = theme.assistant_label(&state.model);
        let spinner = state.spinner_char();
        items.push(ListItem::new(Line::from(Span::styled(
            format!("┌ [{label}] {spinner}"),
            Style::default()
                .fg(theme.streaming_color)
                .add_modifier(Modifier::BOLD),
        ))));
        for line in state.streaming.lines() {
            items.push(ListItem::new(Line::from(Span::styled(
                format!("│ {line}"),
                Style::default().fg(theme.streaming_color),
            ))));
        }
    }

    // If no messages, show hint
    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  Type a message and press Enter · /help for commands · F1 for shortcuts",
            Style::default().fg(theme.dim_color),
        ))));
    }

    let title = if state.busy {
        let elapsed = state
            .tool_start
            .map(|t| format!(" {:.0}s", t.elapsed().as_secs_f32()))
            .unwrap_or_default();
        format!(" Chat {}  {} ", state.spinner_char(), elapsed)
    } else {
        format!(
            " Chat [{} turns] ",
            state.chat.iter().filter(|m| m.role == "user").count()
        )
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(theme.border_color)),
        )
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, area, &mut state.chat_scroll);
}

fn prefix_line(line: Line<'static>, prefix: &'static str) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix)];
    spans.extend(line.spans);
    Line::from(spans)
}

fn draw_event_log(f: &mut ratatui::Frame, state: &mut AppState, area: Rect, theme: &Theme) {
    let items: Vec<ListItem> = state
        .event_log
        .iter()
        .map(|line| {
            let color = if line.starts_with('→') {
                theme.tool_in_color
            } else if line.starts_with('←') {
                theme.tool_out_color
            } else if line.starts_with('⚠') || line.starts_with("error") {
                theme.error_color
            } else if line.starts_with("memory") || line.starts_with("cache") {
                theme.dim_color
            } else if line.starts_with("swarm") {
                Color::LightCyan
            } else {
                theme.border_color
            };
            ListItem::new(Line::from(Span::styled(
                line.as_str(),
                Style::default().fg(color),
            )))
        })
        .collect();

    let title = " Events ";
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(theme.border_color)),
        )
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, area, &mut state.event_scroll);
}

fn draw_input(f: &mut ratatui::Frame, state: &AppState, area: Rect, theme: &Theme) {
    // Show cursor position as a visual block
    let input_with_cursor = if state.busy {
        "  (agent running…)".to_string()
    } else {
        // Insert a block cursor character at cursor_pos
        let before = &state.input[..state.cursor_pos];
        let after = &state.input[state.cursor_pos..];
        format!("  {before}█{after}")
    };

    let title = if !state.tab_completions.is_empty() {
        let cur = state
            .tab_completions
            .get(state.tab_completion_idx)
            .map(|s| s.as_str())
            .unwrap_or("");
        format!(" Message  [Tab→{cur}] ")
    } else if state.search_mode {
        format!(" Search: {} ", state.search_query)
    } else if let Some(idx) = state.history_idx {
        format!(
            " History [{}/{}] — ↑↓ to navigate, Enter to send ",
            idx + 1,
            state.input_history.len()
        )
    } else {
        " Message  [Enter send · Shift+Enter newline · /help · @file Tab] ".to_string()
    };

    let input_widget = Paragraph::new(input_with_cursor)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(if state.busy {
                    Style::default().fg(theme.dim_color)
                } else {
                    Style::default().fg(theme.border_color)
                }),
        )
        .style(Style::default().fg(if state.busy {
            Color::DarkGray
        } else {
            Color::White
        }))
        .wrap(Wrap { trim: false });
    f.render_widget(input_widget, area);
}

fn draw_status(f: &mut ratatui::Frame, state: &AppState, area: Rect, theme: &Theme) {
    let style = if state.computer_use_active {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if state.pending_confirm.is_some() {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if state.busy {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(theme.dim_color)
    };

    let mut indicators = String::new();
    if state.computer_use_active {
        indicators.push_str("[⚠CU] ");
    }
    if state.plan_mode {
        indicators.push_str("[PLAN] ");
    }
    if state.recording_voice {
        indicators.push_str("[🎙REC] ");
    }
    if state.focus_active() {
        indicators.push_str(&format!("[FOCUS {}m] ", state.focus_mins_remaining()));
    }
    if state.search_mode {
        indicators.push_str("[SEARCH] ");
    }

    // Left side: indicators + status message
    let left = format!("{indicators}{}", state.status);
    // Right side: persistent cost/token/session info
    let right = &state.status_right;

    // Build the status line
    let width = area.width as usize;
    let left_len = left.chars().count();
    let right_len = right.chars().count();
    let pad = if left_len + right_len + 2 < width {
        " ".repeat(width - left_len - right_len - 2)
    } else {
        String::new()
    };
    let text = format!(" {left}{pad}{right} ");

    f.render_widget(Paragraph::new(text).style(style), area);
}

fn draw_slash_popup(f: &mut ratatui::Frame, state: &AppState, input_area: Rect, theme: &Theme) {
    let suggestions = &state.slash_suggestions;
    if suggestions.is_empty() {
        return;
    }

    let height = (suggestions.len() as u16).min(8) + 2;
    let width = suggestions
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(20)
        .min(60) as u16
        + 4;

    let x = input_area.x + 2;
    let y = input_area.y.saturating_sub(height);
    let popup = Rect::new(x, y, width.min(input_area.width.saturating_sub(4)), height);

    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = suggestions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let style = if i == state.slash_suggest_idx {
                Style::default().fg(Color::Black).bg(theme.accent_color)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(format!(" {s} "), style)))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Commands ")
            .border_style(Style::default().fg(theme.accent_color)),
    );
    f.render_widget(list, popup);
}

fn draw_search_bar(f: &mut ratatui::Frame, state: &AppState, chat_area: Rect, theme: &Theme) {
    let width = 40u16.min(chat_area.width - 4);
    let bar = Rect::new(
        chat_area.x + chat_area.width.saturating_sub(width + 2),
        chat_area.y + chat_area.height.saturating_sub(3),
        width,
        1,
    );
    let nmatches = state.search_matches.len();
    let match_info = if nmatches > 0 {
        format!(" [{}/{nmatches}]", state.search_match_pos + 1)
    } else {
        String::new()
    };
    let text = format!("/ {}{match_info} Esc:close", state.search_query);
    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::Black).bg(theme.search_hl_color)),
        bar,
    );
}

fn draw_welcome_overlay(f: &mut ratatui::Frame, theme: &Theme) {
    let area = f.area();
    let width = (area.width as f32 * 0.65).min(72.0) as u16;
    let height = 20u16;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, popup_area);

    let lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " Welcome to Harness — May 2026",
            Style::default()
                .fg(theme.accent_color)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Your AI coding assistant for 16-hour days.",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Try these first prompts:",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "   Read README.md and summarize this project.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "   Run the tests and show me which are failing.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "   Refactor src/main.rs to be cleaner.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Keybindings:",
            Style::default().fg(Color::Gray),
        )),
        Line::from(vec![
            Span::styled(
                "   Enter",
                Style::default()
                    .fg(theme.accent_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" send  "),
            Span::styled(
                "Shift+Enter",
                Style::default()
                    .fg(theme.accent_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" newline  "),
            Span::styled(
                "↑↓",
                Style::default()
                    .fg(theme.accent_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" scroll/history"),
        ]),
        Line::from(vec![
            Span::styled(
                "   Ctrl+F",
                Style::default()
                    .fg(theme.accent_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" search  "),
            Span::styled(
                "Ctrl+Y",
                Style::default()
                    .fg(theme.accent_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" copy  "),
            Span::styled(
                "Ctrl+S",
                Style::default()
                    .fg(theme.accent_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" voice  "),
            Span::styled(
                "F1",
                Style::default()
                    .fg(theme.accent_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" help"),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Type /help or press F1 for all commands.",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            " Use @filename to pin files · Tab to autocomplete.",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Press Enter to get started",
            Style::default()
                .fg(theme.accent_color)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent_color))
        .title(Span::styled(
            " harness — first run ",
            Style::default()
                .fg(theme.accent_color)
                .add_modifier(Modifier::BOLD),
        ));

    f.render_widget(Paragraph::new(lines).block(block), popup_area);
}

fn draw_confirm_overlay(f: &mut ratatui::Frame, pc: &PendingConfirm, _theme: &Theme) {
    let area = f.area();
    let width = (area.width as f32 * 0.70) as u16;
    let height = (area.height as f32 * 0.55) as u16;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, popup_area);

    let title = format!(" Plan mode — {} ", pc.tool_name);
    let preview_lines: Vec<Line> = pc
        .preview
        .lines()
        .map(|l| {
            let color = if l.starts_with("+ ") {
                Color::Green
            } else if l.starts_with("- ") {
                Color::Red
            } else if l.starts_with("$ ") {
                Color::Yellow
            } else {
                Color::White
            };
            Line::from(Span::styled(format!(" {l}"), Style::default().fg(color)))
        })
        .collect();

    let mut content: Vec<Line> = preview_lines;
    content.push(Line::from(Span::raw("")));
    content.push(Line::from(vec![
        Span::styled(
            " y",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" approve   "),
        Span::styled(
            "n",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" deny   "),
        Span::styled(
            "a",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" always allow   "),
        Span::styled(
            "Esc",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" skip   "),
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" approve"),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));

    let para = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, popup_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn empty_state() -> AppState {
        AppState::new("test-model")
    }

    #[test]
    fn compute_chat_items_handles_empty_state() {
        let st = empty_state();
        assert_eq!(compute_chat_items(&st), 0);
    }

    #[test]
    fn compute_chat_items_counts_header_content_blank_per_message() {
        let mut st = empty_state();
        st.chat.push(super::super::ChatMessage {
            role: "user".into(),
            content: "hi".into(), // 1 line
            ts: Instant::now(),
        });
        // 1 (header) + 1 (content) + 1 (blank) == 3
        assert_eq!(compute_chat_items(&st), 3);

        st.chat.push(super::super::ChatMessage {
            role: "assistant".into(),
            content: "line1\nline2\nline3".into(), // 3 lines
            ts: Instant::now(),
        });
        // prev 3 + 1+3+1 = 8
        assert_eq!(compute_chat_items(&st), 8);
    }

    #[test]
    fn compute_chat_items_includes_streaming_buffer() {
        let mut st = empty_state();
        st.streaming = "stream-line-a\nstream-line-b".into();
        // 1 (header) + 2 (lines) = 3
        assert_eq!(compute_chat_items(&st), 3);
    }

    #[test]
    fn prefix_line_prepends_prefix_span() {
        let line = Line::from(vec![Span::raw("abc"), Span::raw("def")]);
        let prefixed = prefix_line(line, "│ ");
        assert_eq!(prefixed.spans.len(), 3);
        assert_eq!(prefixed.spans[0].content, "│ ");
        assert_eq!(prefixed.spans[1].content, "abc");
        assert_eq!(prefixed.spans[2].content, "def");
    }
}
