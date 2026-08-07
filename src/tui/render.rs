//! TUI rendering: pure functions that take `&AppState` (or `&mut AppState` for
//! scroll bookkeeping) and produce ratatui frames. No business logic here —
//! this module is the visual layer only.
//!
//! Extracted from `tui/mod.rs` (May 2026) as part of the god-file decomposition.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};

use crate::highlight::Highlighter;

use super::theme::Theme;
use super::{AppState, PendingConfirm, PendingSampling};

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        if raw_line.len() <= width {
            lines.push(raw_line.to_string());
            continue;
        }
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            if current.len() + word.len() + 1 > width {
                if !current.is_empty() {
                    lines.push(current);
                }
                current = word.to_string();
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

/// Estimate list rows for transcript + optional streaming/busy spinner line.
fn compute_chat_items(state: &AppState) -> usize {
    compute_chat_items_from(
        state.chat.iter().map(|m| (m.role.as_str(), m.content.as_str())),
        &state.streaming,
        state.busy,
    )
}

fn compute_chat_items_from<'a>(
    chat: impl IntoIterator<Item = (&'a str, &'a str)>,
    streaming: &str,
    busy: bool,
) -> usize {
    chat.into_iter()
        .map(|(role, content)| {
            if role == "event" {
                content.lines().count().max(1)
            } else {
                1 + content.lines().count().max(1) + 1
            }
        })
        .sum::<usize>()
        + if !streaming.is_empty() || busy {
            1 + streaming.lines().count().max(if busy { 1 } else { 0 })
        } else {
            0
        }
}

pub(crate) fn draw_all(
    f: &mut ratatui::Frame,
    state: &mut AppState,
    hl: &Highlighter,
    theme: &Theme,
) {
    let area = f.area();

    // Hermes-style single column: transcript · input · status (no side panel).
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),    // transcript
            Constraint::Length(3), // compact input
            Constraint::Length(1), // status bar
        ])
        .split(area);

    let chat_item_count = compute_chat_items(state);
    state.chat_items_len = chat_item_count;
    state.event_items_len = state.event_log.len();

    draw_chat(f, state, root[0], hl, theme);
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

    // Search bar overlay (bottom of transcript)
    if state.search_mode {
        draw_search_bar(f, state, root[0], theme);
    }

    if let Some(pc) = &state.pending_confirm {
        draw_confirm_overlay(f, pc, theme);
    }
    if let Some(ps) = &state.pending_sampling {
        draw_sampling_overlay(f, ps, theme);
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
    let content_width = area.width.saturating_sub(4) as usize;

    for (msg_idx, msg) in state.chat.iter().enumerate() {
        let is_search_match = !search_q.is_empty() && state.search_matches.contains(&msg_idx);

        // Inline tool/system events (Hermes-style single stream)
        if msg.role == "event" {
            let color = if msg.content.starts_with('→') {
                theme.tool_in_color
            } else if msg.content.starts_with('←') {
                theme.tool_out_color
            } else if msg.content.starts_with('⚠')
                || msg.content.contains("error")
                || msg.content.starts_with("[error")
            {
                theme.error_color
            } else if msg.content.starts_with("[swarm") || msg.content.contains("swarm") {
                Color::LightCyan
            } else {
                theme.dim_color
            };
            let style = if is_search_match {
                Style::default()
                    .fg(theme.search_hl_color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };
            for raw in msg.content.lines() {
                for wrapped in wrap_text(raw, content_width.saturating_sub(2)) {
                    items.push(ListItem::new(Line::from(Span::styled(
                        format!("  {wrapped}"),
                        style,
                    ))));
                }
            }
            continue;
        }

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
        // Hermes-like: bold role label, plain body (no box drawing)
        items.push(ListItem::new(Line::from(Span::styled(
            format!("{label}{ts_str}"),
            header_style,
        ))));

        if msg.role == "assistant" {
            let rendered = hl.render_message(&msg.content, Style::default().fg(Color::White));
            for line in rendered {
                let plain = line
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>();
                for wrapped in wrap_text(&plain, content_width) {
                    items.push(ListItem::new(Line::from(Span::styled(
                        wrapped,
                        Style::default().fg(Color::White),
                    ))));
                }
            }
        } else {
            for raw in msg.content.lines() {
                for wrapped in wrap_text(raw, content_width) {
                    items.push(ListItem::new(Line::from(Span::styled(
                        wrapped,
                        Style::default().fg(Color::White),
                    ))));
                }
            }
        }
        items.push(ListItem::new(Line::from(Span::raw(""))));
    }

    // Streaming / busy spinner
    if !state.streaming.is_empty() || state.busy {
        let label = theme.assistant_label(&state.model);
        let spinner = state.spinner_char();
        items.push(ListItem::new(Line::from(Span::styled(
            format!("{label} {spinner}"),
            Style::default()
                .fg(theme.streaming_color)
                .add_modifier(Modifier::BOLD),
        ))));
        if !state.streaming.is_empty() {
            for line in state.streaming.lines() {
                for wrapped in wrap_text(line, content_width) {
                    items.push(ListItem::new(Line::from(Span::styled(
                        wrapped,
                        Style::default().fg(theme.streaming_color),
                    ))));
                }
            }
        } else {
            items.push(ListItem::new(Line::from(Span::styled(
                "thinking…",
                Style::default().fg(theme.dim_color),
            ))));
        }
    }

    // If no messages, show hint
    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "Type a message and press Enter · /help for commands · Esc quit",
            Style::default().fg(theme.dim_color),
        ))));
    }

    let title = if state.busy {
        let elapsed = state
            .tool_start
            .map(|t| format!(" {:.0}s", t.elapsed().as_secs_f32()))
            .unwrap_or_default();
        format!(
            " NextEleven Harness · {}{} ",
            state.spinner_char(),
            elapsed
        )
    } else {
        format!(
            " NextEleven Harness · {} turns · {} ",
            state.chat.iter().filter(|m| m.role == "user").count(),
            state.model
        )
    };

    let total_items = items.len();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(theme.border_color)),
        )
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    if total_items > 0 && state.chat_follow {
        let selected = state.chat_scroll.selected().unwrap_or(0);
        if selected == 0 || selected >= total_items.saturating_sub(5) {
            state
                .chat_scroll
                .select(Some(total_items.saturating_sub(1)));
        }
    }

    f.render_stateful_widget(list, area, &mut state.chat_scroll);

    if total_items > area.height.saturating_sub(2) as usize {
        let position = state.chat_scroll.selected().unwrap_or(0);
        let mut scroll_state = ScrollbarState::new(total_items).position(position);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut scroll_state,
        );
    }
}

#[allow(dead_code)]
fn prefix_line(line: Line<'static>, prefix: &'static str) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix)];
    spans.extend(line.spans);
    Line::from(spans)
}

/// Color class for event-log lines (tool in/out, error, memory, swarm, default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventLineKind {
    ToolIn,
    ToolOut,
    Error,
    Dim,
    Swarm,
    Default,
}

pub(crate) fn event_line_kind(line: &str) -> EventLineKind {
    if line.starts_with('→') {
        EventLineKind::ToolIn
    } else if line.starts_with('←') {
        EventLineKind::ToolOut
    } else if line.starts_with('⚠') || line.starts_with("error") {
        EventLineKind::Error
    } else if line.starts_with("memory") || line.starts_with("cache") {
        EventLineKind::Dim
    } else if line.starts_with("swarm") {
        EventLineKind::Swarm
    } else {
        EventLineKind::Default
    }
}

fn event_line_color(line: &str, theme: &Theme) -> Color {
    match event_line_kind(line) {
        EventLineKind::ToolIn => theme.tool_in_color,
        EventLineKind::ToolOut => theme.tool_out_color,
        EventLineKind::Error => theme.error_color,
        EventLineKind::Dim => theme.dim_color,
        EventLineKind::Swarm => Color::LightCyan,
        EventLineKind::Default => theme.border_color,
    }
}

#[allow(dead_code)] // kept for optional debug dumps; layout is single-panel
fn draw_event_log(f: &mut ratatui::Frame, state: &mut AppState, area: Rect, theme: &Theme) {
    let items: Vec<ListItem> = state
        .event_log
        .iter()
        .map(|line| {
            let color = event_line_color(line, theme);
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

    let total = state.event_log.len();
    if total > area.height.saturating_sub(2) as usize {
        let position = state.event_scroll.selected().unwrap_or(0);
        let mut scroll_state = ScrollbarState::new(total).position(position);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut scroll_state,
        );
    }
}

#[allow(dead_code)] // swarm dumps into transcript via /swarm · F2
fn draw_swarm_panel(f: &mut ratatui::Frame, state: &mut AppState, area: Rect, theme: &Theme) {
    let items: Vec<ListItem> = state
        .swarm_lines
        .iter()
        .map(|line| {
            let color = if line.starts_with("swarm ") {
                Color::Cyan
            } else if line.starts_with("active ") {
                Color::LightCyan
            } else if line.starts_with('*') {
                Color::Green
            } else if line.starts_with('!') {
                Color::Yellow
            } else if line.contains("failed") {
                theme.error_color
            } else if line.contains("done") {
                theme.dim_color
            } else {
                theme.border_color
            };
            ListItem::new(Line::from(Span::styled(
                line.as_str(),
                Style::default().fg(color),
            )))
        })
        .collect();

    let total_items = items.len();
    let title = format!(
        " Swarm {} ",
        if state.swarm_active > 0 {
            format!("· {} active", state.swarm_active)
        } else {
            "· idle".into()
        }
    );
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    // Keep selection at bottom when following new swarm snapshots.
    if total_items > 0 {
        let sel = state.swarm_scroll.selected().unwrap_or(0);
        if sel == 0 || sel + 1 >= total_items.saturating_sub(1) {
            state
                .swarm_scroll
                .select(Some(total_items.saturating_sub(1)));
        }
    }

    f.render_stateful_widget(list, area, &mut state.swarm_scroll);

    if total_items > area.height.saturating_sub(2) as usize {
        let position = state.swarm_scroll.selected().unwrap_or(0);
        let mut scroll_state = ScrollbarState::new(total_items).position(position);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut scroll_state,
        );
    }
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
        " ›  Enter send · Shift+Enter newline · /help ".to_string()
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
        if let Some(label) = state.confirm_bar_label.as_deref() {
            indicators.push_str(&format!("[{label}] "));
        } else {
            indicators.push_str("[PLAN] ");
        }
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
    if state.swarm_active > 0 {
        indicators.push_str(&format!("[swarm:{}] ", state.swarm_active));
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
            " Welcome to NextEleven Harness",
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

    let title = if let Some(diff) = &pc.file_diff {
        format!(
            " Diff review — {} ({}/{}) ",
            diff.path.display(),
            pc.hunk_index + 1,
            diff.hunks.len().max(1)
        )
    } else {
        format!(" Plan mode — {} ", pc.tool_name)
    };

    let preview_lines: Vec<Line> = if let Some(diff) = &pc.file_diff {
        if let Some(hunk) = diff.hunks.get(pc.hunk_index) {
            crate::diff_review::format_hunk_for_display(hunk)
                .into_iter()
                .map(|(op, line)| {
                    let color = match op {
                        '+' => Color::Green,
                        '-' => Color::Red,
                        _ => Color::White,
                    };
                    Line::from(Span::styled(format!(" {line}"), Style::default().fg(color)))
                })
                .collect()
        } else {
            vec![Line::from(pc.preview.clone())]
        }
    } else {
        pc.preview
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
            .collect()
    };

    let mut content: Vec<Line> = preview_lines;
    if let Some(diff) = &pc.file_diff {
        let mut buf = crate::diff_review::StagingBuffer::default();
        buf.entries.insert(diff.path.clone(), diff.clone());
        content.push(Line::from(Span::styled(
            format!("  {}", crate::diff_review::render_staging_summary(&buf)),
            Style::default().fg(Color::DarkGray),
        )));
    }
    content.push(Line::from(Span::raw("")));
    content.push(Line::from(vec![
        Span::styled(
            " y",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" accept hunk   "),
        Span::styled(
            "n",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" reject hunk   "),
        Span::styled(
            "[",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("/"),
        Span::styled(
            "]",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" nav   "),
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" approve all   "),
        Span::styled(
            "Esc",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" skip"),
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

fn draw_sampling_overlay(f: &mut ratatui::Frame, ps: &PendingSampling, _theme: &Theme) {
    let area = f.area();
    let width = (area.width as f32 * 0.70) as u16;
    let height = (area.height as f32 * 0.50) as u16;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, popup_area);

    let mut content: Vec<Line> = vec![
        Line::from(Span::styled(
            format!(" MCP sampling from `{}` ", ps.server),
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Server wants the agent LLM to complete a message:",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::raw("")),
    ];
    for l in ps.preview.lines().take(14) {
        content.push(Line::from(Span::styled(
            format!(" {l}"),
            Style::default().fg(Color::White),
        )));
    }
    if ps.preview.lines().count() > 14 {
        content.push(Line::from(Span::styled(
            " …",
            Style::default().fg(Color::DarkGray),
        )));
    }
    content.push(Line::from(Span::raw("")));
    content.push(Line::from(vec![
        Span::styled(
            " y",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" allow   "),
        Span::styled(
            "n/Esc",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" deny"),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(Span::styled(
            " MCP sampling approval ",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
    f.render_widget(
        Paragraph::new(content)
            .block(block)
            .wrap(Wrap { trim: false }),
        popup_area,
    );
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
    fn compute_chat_items_event_role_and_busy_only() {
        let mut st = empty_state();
        st.chat.push(super::super::ChatMessage {
            role: "event".into(),
            content: "→ shell".into(),
            ts: Instant::now(),
        });
        assert_eq!(compute_chat_items(&st), 1);

        st.busy = true;
        st.streaming.clear();
        // event 1 + busy streaming row (1 header + max(0 lines, 1) = 2) => 3
        assert_eq!(compute_chat_items(&st), 3);
    }

    #[test]
    fn compute_chat_items_from_direct() {
        assert_eq!(compute_chat_items_from([], "", false), 0);
        assert_eq!(
            compute_chat_items_from([("user", "a\nb")], "", false),
            1 + 2 + 1
        );
        assert_eq!(compute_chat_items_from([("event", "x")], "y", false), 1 + 2);
    }

    #[test]
    fn wrap_text_width_and_wrapping() {
        assert_eq!(wrap_text("hello", 0), vec!["hello".to_string()]);
        assert_eq!(wrap_text("hello world", 20), vec!["hello world".to_string()]);
        assert_eq!(
            wrap_text("one two three four", 8),
            vec!["one two".to_string(), "three".to_string(), "four".to_string()]
        );
        assert_eq!(
            wrap_text("line1\nline2", 80),
            vec!["line1".to_string(), "line2".to_string()]
        );
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

    #[test]
    fn event_line_kind_classifies_prefixes() {
        assert_eq!(event_line_kind("→ shell"), EventLineKind::ToolIn);
        assert_eq!(event_line_kind("← read_file: ok"), EventLineKind::ToolOut);
        assert_eq!(event_line_kind("⚠ error: boom"), EventLineKind::Error);
        assert_eq!(event_line_kind("error: x"), EventLineKind::Error);
        assert_eq!(event_line_kind("memory: recalled 2"), EventLineKind::Dim);
        assert_eq!(event_line_kind("cache write=1"), EventLineKind::Dim);
        assert_eq!(event_line_kind("swarm ↓ task"), EventLineKind::Swarm);
        assert_eq!(event_line_kind("[plan] ok"), EventLineKind::Default);
    }
}
