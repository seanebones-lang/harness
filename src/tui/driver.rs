//! Crossterm + ratatui main loop — draw UI, drain agent events, fork session helpers.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::{ArcProvider, Message};
use harness_tools::{ConfirmRequest, ConfirmResult, ToolExecutor};
use parking_lot::Mutex;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::{mpsc, watch};

use crate::agent;
use crate::events::{try_emit, AgentEvent};
use crate::highlight::Highlighter;

use super::confirm_flow::{approve_all_hunks, decide_hunk, move_hunk, reject_all_hunks};
use super::events;
use super::input::{
    approve_confirm, finish_confirm, handle_char, handle_mouse, handle_search_key,
    handle_slash_command, handle_voice, show_help,
};
use super::render;
use super::slash::{at_file_completions, expand_at_files};
use super::{mark_welcomed, AppState, ChatMessage, PendingConfirm, PendingSampling};

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
    _model: &str,
    system_prompt: &str,
    native_web_search: bool,
    native_code_execution: bool,
    native_x_search: bool,
    ambient_shutdown: Option<watch::Sender<()>>,
    mut confirm_rx: Option<mpsc::Receiver<ConfirmRequest>>,
    mut sampling_rx: Option<mpsc::UnboundedReceiver<harness_mcp::SamplingApprovalRequest>>,
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
            st.maybe_refresh_swarm(false);
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
                    let file_diff = req.args.as_ref().and_then(|args| {
                        crate::diff_review::file_diff_from_tool(&req.tool_name, args)
                    });
                    if let Some(ref diff) = file_diff {
                        let trust = crate::diff_review::AutoTrustPatterns::load();
                        if trust.should_auto_accept(&diff.path) {
                            let _ = req.reply.send(ConfirmResult::Approve);
                            continue;
                        }
                        if trust.should_auto_reject(&diff.path) {
                            let _ = req.reply.send(ConfirmResult::Deny);
                            continue;
                        }
                    }
                    let hunk_index = file_diff
                        .as_ref()
                        .and_then(|d| crate::diff_review::next_pending_hunk(d, 0))
                        .unwrap_or(0);
                    let has_diff = file_diff.is_some();
                    let mut st = state.lock();
                    st.pending_confirm = Some(PendingConfirm {
                        tool_name: req.tool_name,
                        preview: req.preview,
                        file_diff,
                        hunk_index,
                        reply: req.reply,
                    });
                    st.status = if has_diff {
                        "DIFF REVIEW — y/n hunk · [/] nav · Enter approve all · Esc skip".into()
                    } else {
                        let label = st.confirm_bar_label.as_deref().unwrap_or("PLAN");
                        format!("{label} MODE — y approve · n skip · a always allow")
                    };
                }
            }
        }

        // Poll MCP sampling approval requests
        if state.lock().pending_sampling.is_none() {
            if let Some(rx) = &mut sampling_rx {
                if let Ok(req) = rx.try_recv() {
                    let mut st = state.lock();
                    st.push_event(format!("[mcp sampling] request from `{}`", req.server));
                    st.pending_sampling = Some(PendingSampling {
                        server: req.server,
                        preview: req.preview,
                        reply: req.reply,
                    });
                    st.status =
                        "MCP SAMPLING — y allow LLM call · n deny (default deny if ignored)".into();
                }
            }
        }

        // Finished session
        if let Ok(finished) = done_rx.try_recv() {
            let mut to_save = finished.clone();
            if let Some(title) = agent::suggest_session_name(provider, &to_save).await {
                let _ = session_store.set_name_if_missing(&to_save.id, &title);
                to_save.name = Some(title);
            }
            *session = to_save.clone();
            session_store.save(session)?;
            {
                let p2 = provider.clone();
                let store2 = session_store.clone();
                let mem_owned = memory_store.cloned();
                let em_owned = embed_model.map(|s| s.to_string());
                let mem_pair = mem_owned.zip(em_owned);
                let sess2 = to_save;
                tokio::spawn(async move {
                    if let Some((mem, em)) = mem_pair {
                        agent::store_turn_memory(&p2, &mem, &em, &sess2).await;
                    }
                    let _ = store2.save(&sess2);
                });
            }
            let mut st = state.lock();
            st.busy = false;
            st.tool_start = None;
            st.session_id = session.id[..8].to_string();
            st.status = "Done".to_string();
            let turns = super::resume::count_user_turns(session).max(1);
            st.status_right = st.format_status_right(&session.id[..8], turns);
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

                    // ── Ctrl+] / Ctrl+[ — single-panel layout (no-op) ────────────────
                    (KeyCode::Char(']'), KeyModifiers::CONTROL)
                    | (KeyCode::Char('['), KeyModifiers::CONTROL) => {
                        state.lock().status =
                            "Single-panel layout (Hermes-style) — nothing to resize".into();
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
                        if let Some(ps) = state.lock().pending_sampling.take() {
                            let _ = ps.reply.send(false);
                            let mut st = state.lock();
                            st.push_event(format!("[mcp sampling] denied `{}`", ps.server));
                            st.status = "MCP sampling denied.".into();
                            continue;
                        }
                        let confirm = state.lock().pending_confirm.take();
                        if let Some(pc) = confirm {
                            let tool = pc.tool_name.clone();
                            let result = reject_all_hunks(&pc);
                            let _ = pc.reply.send(result);
                            let mut st = state.lock();
                            st.push_event(format!("[plan] skipped: {tool}"));
                            st.status = "Skipped.".to_string();
                        }
                    }

                    // ── Y — approve confirm ────────────────────────────────────
                    (KeyCode::Char('y'), KeyModifiers::NONE) => {
                        if let Some(ps) = state.lock().pending_sampling.take() {
                            let _ = ps.reply.send(true);
                            let mut st = state.lock();
                            st.push_event(format!("[mcp sampling] approved `{}`", ps.server));
                            st.status = "MCP sampling approved.".into();
                            continue;
                        }
                        let pc = state.lock().pending_confirm.take();
                        if let Some(mut pc) = pc {
                            if pc.file_diff.is_some() {
                                if let Some(result) = decide_hunk(&mut pc, true) {
                                    finish_confirm(&state, pc, result, "approved");
                                } else {
                                    state.lock().pending_confirm = Some(pc);
                                }
                                continue;
                            }
                            approve_confirm(&state, pc);
                            continue;
                        }
                        handle_char(&state, 'y');
                    }

                    (KeyCode::Char('n'), KeyModifiers::NONE) => {
                        if let Some(ps) = state.lock().pending_sampling.take() {
                            let _ = ps.reply.send(false);
                            let mut st = state.lock();
                            st.push_event(format!("[mcp sampling] denied `{}`", ps.server));
                            st.status = "MCP sampling denied.".into();
                            continue;
                        }
                        if let Some(mut pc) = state.lock().pending_confirm.take() {
                            if pc.file_diff.is_some() {
                                if let Some(result) = decide_hunk(&mut pc, false) {
                                    finish_confirm(&state, pc, result, "reviewed");
                                } else {
                                    state.lock().pending_confirm = Some(pc);
                                }
                                continue;
                            }
                            let tool = pc.tool_name.clone();
                            let _ = pc.reply.send(ConfirmResult::Deny);
                            let mut st = state.lock();
                            st.push_event(format!("[plan] denied: {tool}"));
                            st.status = "Denied.".to_string();
                            continue;
                        }
                        handle_char(&state, 'n');
                    }

                    (KeyCode::Char('['), KeyModifiers::NONE)
                        if state.lock().pending_confirm.is_some() =>
                    {
                        let mut st = state.lock();
                        if let Some(pc) = st.pending_confirm.as_mut() {
                            move_hunk(pc, -1);
                        }
                        continue;
                    }

                    (KeyCode::Char(']'), KeyModifiers::NONE)
                        if state.lock().pending_confirm.is_some() =>
                    {
                        let mut st = state.lock();
                        if let Some(pc) = st.pending_confirm.as_mut() {
                            move_hunk(pc, 1);
                        }
                        continue;
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
                            if let Some(mut pc) = state.lock().pending_confirm.take() {
                                if pc.file_diff.is_some() {
                                    let result = approve_all_hunks(&mut pc);
                                    finish_confirm(&state, pc, result, "approved all hunks");
                                } else {
                                    approve_confirm(&state, pc);
                                }
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
                            let res = agent::drive_agent_full(
                                &p2,
                                &t2,
                                mem2.as_ref(),
                                em2.as_deref(),
                                &mut sess_clone,
                                &sys,
                                Some(&atx),
                                think_budget,
                                native_web_search,
                                native_code_execution,
                                native_x_search,
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

                    // ── PageUp/Down — scroll transcript ──────────────────────────────
                    (KeyCode::PageUp, _) => {
                        state.lock().scroll_chat_up(8);
                    }
                    (KeyCode::PageDown, _) => {
                        state.lock().scroll_chat_down(8);
                    }

                    // ── F1 — help ─────────────────────────────────────────────
                    (KeyCode::F(1), _) => {
                        show_help(&state);
                    }
                    // ── F2 — dump swarm into transcript ───────────────────────
                    (KeyCode::F(2), _) => {
                        state.lock().toggle_swarm_panel();
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

/// Parse a task id from a swarm line (`*swabcdef01 status prompt…`).
#[allow(dead_code)] // used when peeking swarm results from transcript dumps
fn extract_swarm_task_id(line: &str) -> Option<String> {
    let s = line.trim_start_matches(['*', '!', ' ']);
    let id = s.split_whitespace().next()?;
    if id.starts_with("sw") && id.len() >= 4 {
        Some(id.to_string())
    } else {
        None
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use harness_provider_core::Message;

    #[test]
    fn extract_swarm_task_id_parses_markers() {
        assert_eq!(
            extract_swarm_task_id("*swabcdef01 done prompt here"),
            Some("swabcdef01".into())
        );
        assert_eq!(
            extract_swarm_task_id("  !sw12 running x"),
            Some("sw12".into())
        );
        assert_eq!(extract_swarm_task_id("swab"), Some("swab".into()));
        assert_eq!(extract_swarm_task_id("sw"), None); // too short
        assert_eq!(extract_swarm_task_id("*task01 done"), None);
        assert_eq!(extract_swarm_task_id(""), None);
    }

    #[test]
    fn extract_swarm_task_id_edge_markers_and_lengths() {
        // stacked markers
        assert_eq!(
            extract_swarm_task_id("!!!***swzzzz status"),
            Some("swzzzz".into())
        );
        assert_eq!(
            extract_swarm_task_id("*! swabcd01 queued"),
            Some("swabcd01".into())
        );
        // minimum accepted length is 4 ("sw" + 2)
        assert_eq!(extract_swarm_task_id("sw1"), None);
        assert_eq!(extract_swarm_task_id("sw12"), Some("sw12".into()));
        // leading tab is not in trim set, but split_whitespace still yields "swab"
        assert_eq!(extract_swarm_task_id("\tswab"), Some("swab".into()));
        // other leading punctuation stays glued to the token → reject
        assert_eq!(extract_swarm_task_id("-swab"), None);
        assert_eq!(extract_swarm_task_id("#swab"), None);
        // first whitespace-delimited token only
        assert_eq!(
            extract_swarm_task_id("swfirst swsecond"),
            Some("swfirst".into())
        );
        // case-sensitive prefix
        assert_eq!(extract_swarm_task_id("SWab"), None);
        assert_eq!(extract_swarm_task_id("Swab"), None);
        // whitespace-only
        assert_eq!(extract_swarm_task_id("   "), None);
        assert_eq!(extract_swarm_task_id("***"), None);
        // id alone with trailing spaces still parses (split_whitespace)
        assert_eq!(extract_swarm_task_id("swxy  "), Some("swxy".into()));
    }

    #[test]
    fn count_user_turns_filters_roles() {
        let msgs = vec![
            Message::user("a"),
            Message::assistant("b"),
            Message::user("c"),
            Message::assistant("d"),
        ];
        assert_eq!(count_user_turns(&msgs), 2);
        assert_eq!(count_user_turns(&[]), 0);
    }

    #[test]
    fn count_user_turns_ignores_system_tool_and_assistant_only() {
        let mixed = vec![
            Message::system("sys"),
            Message::user("u1"),
            Message::assistant("a1"),
            Message::tool_result("tc1", "ok"),
            Message::user("u2"),
            Message::tool_result("tc2", "ok2"),
        ];
        assert_eq!(count_user_turns(&mixed), 2);

        let no_user = vec![
            Message::system("s"),
            Message::assistant("a"),
            Message::tool_result("t", "r"),
        ];
        assert_eq!(count_user_turns(&no_user), 0);

        let only_users = vec![Message::user("1"), Message::user("2"), Message::user("3")];
        assert_eq!(count_user_turns(&only_users), 3);
    }

    #[test]
    fn fork_session_at_stops_after_nth_user_turn() {
        let mut original = harness_memory::Session::new("grok-4.5");
        original.name = Some("main".into());
        original.messages.push(Message::user("u1"));
        original.messages.push(Message::assistant("a1"));
        original.messages.push(Message::user("u2"));
        original.messages.push(Message::assistant("a2"));
        original.messages.push(Message::user("u3"));

        let fork = fork_session_at(&original, 2);
        assert_eq!(fork.name.as_deref(), Some("main (fork@2)"));
        assert_eq!(fork.model, "grok-4.5");
        assert_eq!(fork.messages.len(), 3); // u1, a1, u2
        assert_eq!(count_user_turns(&fork.messages), 2);
        assert_ne!(fork.id, original.id);

        let early = fork_session_at(&original, 1);
        assert_eq!(early.messages.len(), 1);
        assert_eq!(count_user_turns(&early.messages), 1);

        let unnamed = harness_memory::Session::new("m");
        let f2 = fork_session_at(&unnamed, 1);
        assert!(f2.name.is_none());
        assert!(f2.messages.is_empty());
    }

    #[test]
    fn fork_session_at_beyond_available_and_preserves_prefix() {
        let mut original = harness_memory::Session::new("m1");
        original.name = Some("s".into());
        original.messages.push(Message::user("u1"));
        original.messages.push(Message::assistant("a1"));
        original.messages.push(Message::tool_result("c1", "r1"));
        original.messages.push(Message::user("u2"));

        // turn beyond available user turns → keep whole transcript
        let all = fork_session_at(&original, 99);
        assert_eq!(all.messages.len(), original.messages.len());
        assert_eq!(count_user_turns(&all.messages), 2);
        assert_eq!(all.name.as_deref(), Some("s (fork@99)"));
        assert_ne!(all.id, original.id);

        // fork at last user includes trailing non-user? stops when user_count hits n
        // after u2 is pushed user_count=2 → break, so no messages after u2 (none anyway)
        let at2 = fork_session_at(&original, 2);
        assert_eq!(at2.messages.len(), 4);
        assert_eq!(count_user_turns(&at2.messages), 2);

        // includes tool result that sits between user turns when n covers later user
        let at1 = fork_session_at(&original, 1);
        assert_eq!(at1.messages.len(), 1); // only first user
    }

    #[test]
    fn fork_session_at_turn_zero_and_leading_non_user() {
        let mut original = harness_memory::Session::new("m");
        original.messages.push(Message::system("sys"));
        original.messages.push(Message::user("u1"));
        original.messages.push(Message::assistant("a1"));

        // turn_n=0: after first message user_count(0) >= 0 → break with 1 msg
        let z = fork_session_at(&original, 0);
        assert_eq!(z.messages.len(), 1);
        assert_eq!(count_user_turns(&z.messages), 0);
        assert!(z.name.is_none()); // original had no name

        // leading non-user then stop at first user
        let f = fork_session_at(&original, 1);
        assert_eq!(f.messages.len(), 2); // system + user
        assert_eq!(count_user_turns(&f.messages), 1);
    }

    #[test]
    fn fork_session_at_empty_and_model_copy() {
        let empty = harness_memory::Session::new("only-model");
        let f = fork_session_at(&empty, 5);
        assert_eq!(f.model, "only-model");
        assert!(f.messages.is_empty());
        assert!(f.name.is_none());
        assert_ne!(f.id, empty.id);

        let mut named = harness_memory::Session::new("x");
        named.name = Some("alpha".into());
        let f2 = fork_session_at(&named, 0);
        assert_eq!(f2.name.as_deref(), Some("alpha (fork@0)"));
    }
}
