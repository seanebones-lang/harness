//! Crossterm + ratatui main loop — draw UI, drain agent events, fork session helpers.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::{ArcProvider, Message};
use harness_tools::{ConfirmRequest, ToolExecutor};
use parking_lot::Mutex;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::{mpsc, watch};

use crate::agent;
use crate::events::{try_emit, AgentEvent};
use crate::highlight::Highlighter;

use super::events;
use super::input::{
    approve_confirm, handle_char, handle_mouse, handle_search_key, handle_slash_command,
    handle_voice, show_help,
};
use super::render;
use super::slash::{at_file_completions, expand_at_files};
use super::{mark_welcomed, AppState, ChatMessage, PendingConfirm};

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_terminal_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: Arc<Mutex<AppState>>,
    session: &mut Session,
    provider: &ArcProvider,
    session_store: &SessionStore,
    memory_store: Option<&MemoryStore>,
    embed_model: Option<&str>,
    tools: &ToolExecutor,
    model: &str,
    system_prompt: &str,
    ambient_shutdown: Option<watch::Sender<()>>,
    mut confirm_rx: Option<mpsc::Receiver<ConfirmRequest>>,
) -> Result<()> {
    let highlighter = Highlighter::new();
    let (agent_tx, mut agent_rx) = crate::events::channel();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<harness_memory::Session>();

    loop {
        // Spinner tick
        {
            let mut st = state.lock();
            if st.busy {
                st.tick_spinner();
            }
        }

        // Draw
        {
            let mut st = state.lock();
            let hl = &highlighter;
            let theme = st.theme.clone();
            terminal.draw(|f| render::draw_all(f, &mut st, hl, &theme))?;
        }

        // Drain agent events
        while let Ok(ev) = agent_rx.try_recv() {
            events::apply_agent_event(&state, ev);
        }

        // Poll for confirmation requests
        if state.lock().pending_confirm.is_none() {
            if let Some(rx) = &mut confirm_rx {
                if let Ok(req) = rx.try_recv() {
                    let mut st = state.lock();
                    st.pending_confirm = Some(PendingConfirm {
                        tool_name: req.tool_name,
                        preview: req.preview,
                        reply: req.reply,
                    });
                    st.status = "PLAN MODE — y approve · n skip · a always allow".to_string();
                }
            }
        }

        // Finished session
        if let Ok(finished) = done_rx.try_recv() {
            *session = finished.clone();
            session_store.save(session)?;
            {
                let p2 = provider.clone();
                let store2 = session_store.clone();
                let mem_owned = memory_store.cloned();
                let em_owned = embed_model.map(|s| s.to_string());
                let mem_pair = mem_owned.zip(em_owned);
                let mut sess2 = finished.clone();
                tokio::spawn(async move {
                    if let Some(title) = agent::suggest_session_name(&p2, &sess2).await {
                        let _ = store2.set_name_if_missing(&sess2.id, &title);
                        sess2.name = Some(title);
                    }
                    let _ = store2.save(&sess2);
                    if let Some((mem, em)) = mem_pair {
                        agent::store_turn_memory(&p2, &mem, &em, &sess2).await;
                    }
                });
            }
            let mut st = state.lock();
            st.busy = false;
            st.tool_start = None;
            st.session_id = session.id[..8].to_string();
            let cost_str = st.cost_str();
            let elapsed = st.elapsed_str();
            let turns = session.messages.len();
            st.status = "Done".to_string();
            st.status_right = format!(
                "{} · {} · {} turns · {} · {}",
                &session.id[..8],
                model,
                turns,
                cost_str,
                elapsed
            );
            st.scroll_to_bottom();
        }

        // Handle terminal events
        if event::poll(Duration::from_millis(16))? {
            let ev = event::read()?;

            // Mouse scroll
            if let Event::Mouse(mouse) = ev {
                handle_mouse(&state, mouse);
                continue;
            }

            // Bracketed paste
            if let Event::Paste(pasted) = &ev {
                let mut st = state.lock();
                let trimmed = pasted.trim();
                let is_image_path = {
                    let lower = trimmed.to_lowercase();
                    (lower.ends_with(".png")
                        || lower.ends_with(".jpg")
                        || lower.ends_with(".jpeg")
                        || lower.ends_with(".gif")
                        || lower.ends_with(".webp"))
                        && std::path::Path::new(trimmed).exists()
                };
                if is_image_path {
                    st.push_event(format!("[paste] image → {trimmed}"));
                    let at_ref = format!("@{trimmed} ");
                    for c in at_ref.chars() {
                        st.insert_char(c);
                    }
                } else {
                    for c in pasted.chars() {
                        st.insert_char(c);
                    }
                }
                continue;
            }

            if let Event::Key(key) = ev {
                // Search mode intercept
                {
                    let search = state.lock().search_mode;
                    if search && handle_search_key(&state, key) {
                        continue;
                    }
                }

                match (key.code, key.modifiers) {
                    // ── Quit ─────────────────────────────────────────────────
                    (KeyCode::Char('c'), KeyModifiers::CONTROL)
                    | (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                        if let Some(tx) = &ambient_shutdown {
                            let _ = tx.send(());
                        }
                        break;
                    }

                    // ── Voice (moved from Ctrl+V to Ctrl+S) ──────────────────
                    (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                        handle_voice(&state);
                    }

                    // ── Ctrl+F / forward-slash focus → search ─────────────────
                    (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                        let mut st = state.lock();
                        st.search_mode = true;
                        st.search_query.clear();
                        st.search_matches.clear();
                        st.status = "Search: ".to_string();
                    }

                    // ── Ctrl+Y — copy last response ───────────────────────────
                    (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                        let last = state
                            .lock()
                            .chat
                            .iter()
                            .rev()
                            .find(|m| m.role == "assistant")
                            .map(|m| m.content.clone());
                        if let Some(text) = last {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(&text);
                                state.lock().status = "Copied last response.".to_string();
                            }
                        }
                    }

                    // ── Ctrl+E — fork mode ────────────────────────────────────
                    (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                        let mut st = state.lock();
                        if st.busy {
                            st.push_event("[fork] agent running, please wait.");
                        } else {
                            st.fork_mode = !st.fork_mode;
                            if st.fork_mode {
                                let turns = count_user_turns(&session.messages);
                                st.status = format!("FORK MODE — enter turn (1-{turns}) + Enter to fork, Esc to cancel");
                                st.input.clear();
                                st.cursor_pos = 0;
                            } else {
                                st.status = "Ready".to_string();
                            }
                        }
                    }

                    // ── Ctrl+] / Ctrl+[ — resize panels ───────────────────────
                    (KeyCode::Char(']'), KeyModifiers::CONTROL) => {
                        let mut st = state.lock();
                        st.right_panel_pct = st.right_panel_pct.saturating_add(5).min(70);
                        st.status = format!("Right panel: {}%", st.right_panel_pct);
                    }
                    (KeyCode::Char('['), KeyModifiers::CONTROL) => {
                        let mut st = state.lock();
                        st.right_panel_pct = st.right_panel_pct.saturating_sub(5).max(20);
                        st.status = format!("Right panel: {}%", st.right_panel_pct);
                    }

                    // ── Ctrl+L — scroll to bottom ─────────────────────────────
                    (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                        state.lock().scroll_to_bottom();
                    }

                    // ── Esc ───────────────────────────────────────────────────
                    (KeyCode::Esc, _) => {
                        let mut st = state.lock();
                        if st.fork_mode {
                            st.fork_mode = false;
                            st.input.clear();
                            st.cursor_pos = 0;
                            st.status = "Fork cancelled.".to_string();
                        }
                        drop(st);
                        let confirm = state.lock().pending_confirm.take();
                        if let Some(pc) = confirm {
                            let _ = pc.reply.send(false);
                            let mut st = state.lock();
                            st.push_event(format!("[plan] skipped: {}", pc.tool_name));
                            st.status = "Skipped.".to_string();
                        }
                    }

                    // ── Y — approve confirm ────────────────────────────────────
                    (KeyCode::Char('y'), KeyModifiers::NONE) => {
                        let confirm = state.lock().pending_confirm.take();
                        if let Some(pc) = confirm {
                            approve_confirm(&state, pc);
                            continue;
                        }
                        // Otherwise insert 'y' normally
                        handle_char(&state, 'y');
                    }

                    // ── N — deny confirm ───────────────────────────────────────
                    (KeyCode::Char('n'), KeyModifiers::NONE) => {
                        let confirm = state.lock().pending_confirm.take();
                        if let Some(pc) = confirm {
                            let _ = pc.reply.send(false);
                            let mut st = state.lock();
                            st.push_event(format!("[plan] denied: {}", pc.tool_name));
                            st.status = "Denied.".to_string();
                            continue;
                        }
                        handle_char(&state, 'n');
                    }

                    // ── A — always allow ──────────────────────────────────────
                    (KeyCode::Char('a'), KeyModifiers::NONE) => {
                        let has_confirm = state.lock().pending_confirm.is_some();
                        if has_confirm {
                            let confirm = state.lock().pending_confirm.take();
                            if let Some(pc) = confirm {
                                let tool = pc.tool_name.clone();
                                let first_arg = pc.preview.lines().next().unwrap_or("").to_string();
                                approve_confirm(&state, pc);
                                // Emit trust suggestion
                                state.lock().push_event(
                                    format!("[trust] Run: harness trust {tool} \"{first_arg}\" to always allow.")
                                );
                            }
                            continue;
                        }
                        handle_char(&state, 'a');
                    }

                    // ── Enter ─────────────────────────────────────────────────
                    (KeyCode::Enter, m) => {
                        // Shift+Enter or Alt+Enter: insert newline
                        if m.contains(KeyModifiers::SHIFT) || m.contains(KeyModifiers::ALT) {
                            state.lock().insert_char('\n');
                            continue;
                        }

                        // Welcome dismiss
                        {
                            let mut st = state.lock();
                            if st.show_welcome {
                                st.show_welcome = false;
                                st.status = "Ready".to_string();
                                mark_welcomed();
                                continue;
                            }
                        }

                        // Slash autocomplete select with Tab (handled below), Enter sends
                        // (don't consume Enter for slash suggest — that sends the completed cmd)

                        // Fork mode
                        {
                            let fork_active = state.lock().fork_mode;
                            if fork_active {
                                let input = state.lock().input.trim().to_string();
                                if let Ok(turn_n) = input.parse::<usize>() {
                                    let new_session = fork_session_at(session, turn_n);
                                    *session = new_session;
                                    session_store.save(session)?;
                                    let mut st = state.lock();
                                    let short = session.id[..8.min(session.id.len())].to_string();
                                    st.fork_mode = false;
                                    st.input.clear();
                                    st.cursor_pos = 0;
                                    st.chat.clear();
                                    st.event_log.clear();
                                    st.session_id = short.clone();
                                    st.push_event(format!(
                                        "[fork] session {short} forked at turn {turn_n}"
                                    ));
                                    st.status = format!("Forked at turn {turn_n} — continue here.");
                                } else {
                                    state.lock().status =
                                        "Fork: enter a valid turn number.".to_string();
                                }
                                continue;
                            }
                        }

                        // Approve pending confirm
                        {
                            let confirm = state.lock().pending_confirm.take();
                            if let Some(pc) = confirm {
                                approve_confirm(&state, pc);
                                continue;
                            }
                        }

                        let busy = state.lock().busy;
                        if busy {
                            continue;
                        }

                        let prompt = {
                            let mut st = state.lock();
                            st.tab_completions.clear();
                            st.slash_suggestions.clear();
                            st.take_input()
                        };
                        if prompt.trim().is_empty() {
                            continue;
                        }

                        // Slash commands
                        if prompt.trim_start().starts_with('/') {
                            let cmd = prompt.trim();
                            handle_slash_command(
                                cmd,
                                &state,
                                session,
                                provider,
                                session_store,
                                &agent_tx,
                            )
                            .await;
                            continue;
                        }

                        // Expand @file tokens
                        let expanded = expand_at_files(&prompt);

                        {
                            let mut st = state.lock();
                            let label = if prompt.len() > 100 {
                                format!("{}…", &prompt[..100])
                            } else {
                                prompt.clone()
                            };
                            st.chat.push(ChatMessage {
                                role: "user".into(),
                                content: label,
                                ts: Instant::now(),
                            });
                            st.busy = true;
                            st.streaming.clear();
                            st.status = "Thinking…".to_string();
                            st.event_log.clear();
                            st.tool_start = Some(Instant::now());
                        }

                        let send_prompt = if expanded != prompt { expanded } else { prompt };
                        session.push(Message::user(&send_prompt));

                        let p2 = provider.clone();
                        let t2 = tools.clone();
                        let mem2 = memory_store.cloned();
                        let em2 = embed_model.map(|s| s.to_string());
                        let sys = system_prompt.to_string();
                        let atx = agent_tx.clone();
                        let dtx = done_tx.clone();
                        let mut sess_clone = session.clone();
                        let think_budget = state.lock().thinking_budget;
                        let resp_schema = state.lock().response_schema.clone();

                        tokio::spawn(async move {
                            let res = agent::drive_agent_with_schema(
                                &p2,
                                &t2,
                                mem2.as_ref(),
                                em2.as_deref(),
                                &mut sess_clone,
                                &sys,
                                Some(&atx),
                                think_budget,
                                resp_schema,
                            )
                            .await;
                            if let Err(e) = res {
                                try_emit(
                                    Some(&atx),
                                    AgentEvent::Error(format!("Agent error: {e}")),
                                );
                            }
                            let _ = dtx.send(sess_clone);
                        });
                    }

                    // ── Tab — @file completion or slash completion ─────────────
                    (KeyCode::Tab, _) => {
                        // Slash suggestion completion
                        {
                            let has_slash = !state.lock().slash_suggestions.is_empty();
                            if has_slash {
                                let mut st = state.lock();
                                st.slash_suggest_idx =
                                    (st.slash_suggest_idx + 1) % st.slash_suggestions.len();
                                // Apply selected command to input (strip description)
                                let selected = st.slash_suggestions[st.slash_suggest_idx].clone();
                                let cmd = selected
                                    .split("  —")
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                                    .to_string();
                                st.input = cmd.clone();
                                st.cursor_pos = cmd.len();
                                continue;
                            }
                        }
                        // @file completion
                        let (input_snap, cursor_snap) = {
                            let st = state.lock();
                            (st.input.clone(), st.cursor_pos)
                        };
                        let before_cursor = &input_snap[..cursor_snap];
                        if let Some(at_pos) = before_cursor.rfind('@') {
                            let partial = &before_cursor[at_pos + 1..];
                            let mut st = state.lock();
                            if st.tab_completions.is_empty() {
                                st.tab_completions = at_file_completions(partial);
                                st.tab_completion_idx = 0;
                            } else {
                                st.tab_completion_idx =
                                    (st.tab_completion_idx + 1) % st.tab_completions.len().max(1);
                            }
                            if let Some(c) = st.tab_completions.get(st.tab_completion_idx).cloned()
                            {
                                let new_input = format!(
                                    "{}@{}{}",
                                    &input_snap[..at_pos],
                                    c,
                                    &input_snap[cursor_snap..]
                                );
                                let new_cursor = at_pos + 1 + c.len();
                                st.input = new_input;
                                st.cursor_pos = new_cursor;
                            }
                        }
                    }

                    // ── Backspace ─────────────────────────────────────────────
                    (KeyCode::Backspace, _) => {
                        let mut st = state.lock();
                        st.tab_completions.clear();
                        st.backspace();
                    }

                    // ── Delete forward ────────────────────────────────────────
                    (KeyCode::Delete, _) => {
                        state.lock().delete_forward();
                    }

                    // ── Left / Right cursor movement ─────────────────────────
                    (KeyCode::Left, m) if m.contains(KeyModifiers::ALT) => {
                        state.lock().move_word_left();
                    }
                    (KeyCode::Left, _) => {
                        state.lock().move_left();
                    }
                    (KeyCode::Right, m) if m.contains(KeyModifiers::ALT) => {
                        state.lock().move_word_right();
                    }
                    (KeyCode::Right, _) => {
                        state.lock().move_right();
                    }
                    (KeyCode::Home, _) => {
                        // Go to start of current line in input
                        let input = state.lock().input.clone();
                        let cursor = state.lock().cursor_pos;
                        let line_start = input[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
                        state.lock().cursor_pos = line_start;
                    }
                    (KeyCode::End, _) => {
                        let input = state.lock().input.clone();
                        let cursor = state.lock().cursor_pos;
                        let line_end = input[cursor..]
                            .find('\n')
                            .map(|i| cursor + i)
                            .unwrap_or(input.len());
                        state.lock().cursor_pos = line_end;
                    }

                    // ── Readline shortcuts ────────────────────────────────────
                    (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                        state.lock().cursor_pos = 0;
                    }
                    // Note: Ctrl+E is fork mode (see above). Use End key for end-of-line.
                    (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                        state.lock().kill_word_back();
                    }
                    (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                        state.lock().kill_line();
                    }
                    (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                        state.lock().kill_to_end();
                    }

                    // ── Scroll chat (Up/Down) or input history ────────────────
                    (KeyCode::Up, _) => {
                        let input_empty = state.lock().input.is_empty();
                        if input_empty {
                            state.lock().history_up();
                        } else {
                            state.lock().scroll_chat_up(3);
                        }
                    }
                    (KeyCode::Down, _) => {
                        let at_history = state.lock().history_idx.is_some();
                        if at_history {
                            state.lock().history_down();
                        } else {
                            state.lock().scroll_chat_down(3);
                        }
                    }

                    // ── Ctrl+Up/Down — scroll chat by half page ───────────────
                    (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                        state.lock().scroll_chat_up(10);
                    }
                    (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                        let has_confirm = state.lock().pending_confirm.is_some();
                        if !has_confirm {
                            state.lock().scroll_chat_down(10);
                        }
                    }

                    // ── PageUp/Down — scroll event log ────────────────────────
                    (KeyCode::PageUp, _) => {
                        state.lock().scroll_event_up(5);
                    }
                    (KeyCode::PageDown, _) => {
                        state.lock().scroll_event_down(5);
                    }

                    // ── F1 — help ─────────────────────────────────────────────
                    (KeyCode::F(1), _) => {
                        show_help(&state);
                    }

                    // ── Regular char input ────────────────────────────────────
                    (KeyCode::Char(c), m)
                        if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
                    {
                        state.lock().tab_completions.clear();
                        state.lock().insert_char(c);
                    }

                    _ => {}
                }
            }
        }
    }

    Ok(())
}

fn count_user_turns(messages: &[harness_provider_core::Message]) -> usize {
    messages
        .iter()
        .filter(|m| matches!(m.role, harness_provider_core::Role::User))
        .count()
}

fn fork_session_at(original: &harness_memory::Session, turn_n: usize) -> harness_memory::Session {
    use harness_provider_core::Role;
    let mut new_session = harness_memory::Session::new(&original.model);
    if let Some(name) = &original.name {
        new_session.name = Some(format!("{name} (fork@{turn_n})"));
    }
    let mut user_count = 0;
    for msg in &original.messages {
        if matches!(msg.role, Role::User) {
            user_count += 1;
        }
        new_session.messages.push(msg.clone());
        if user_count >= turn_n {
            break;
        }
    }
    new_session
}
