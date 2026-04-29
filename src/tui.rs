//! Two-panel TUI: chat history (left) + tool/event log (right), input box + status bar.
//! Agent runs in a background tokio task, streaming events via mpsc channel.
//! Code blocks in assistant messages are syntax-highlighted via syntect.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    event::{DisableBracketedPaste, EnableBracketedPaste},
};
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::{ArcProvider, Message};
use harness_tools::{ConfirmRequest, ToolExecutor};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Terminal,
};
use std::io;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::watch;

use crate::agent::{self, DEFAULT_SYSTEM};
use crate::config::Config;
use crate::cost;
use crate::events::AgentEvent;
use crate::highlight::Highlighter;

// ── App state ─────────────────────────────────────────────────────────────────

struct ChatMessage {
    role: String,
    content: String,
}

struct AppState {
    input: String,
    cursor_pos: usize,
    /// Finalized chat messages shown in the left panel.
    chat: Vec<ChatMessage>,
    /// Current streaming assistant text (rendered live at bottom of chat).
    streaming: String,
    /// Event log shown in the right panel.
    event_log: Vec<String>,
    /// Status bar text.
    status: String,
    /// Is the agent currently running?
    busy: bool,
    /// Chat scroll offset (lines from bottom).
    chat_scroll: u16,
    /// Event log scroll.
    event_scroll: u16,
    /// Session id for display.
    session_id: String,
    /// Cumulative token counts for this session.
    tokens_in: u64,
    tokens_out: u64,
    /// Active model name (for cost calculation).
    model: String,
    /// Session start time for elapsed display.
    session_start: std::time::Instant,
    /// Plan mode: pending confirmation request shown as an overlay.
    pending_confirm: Option<PendingConfirm>,
    /// First-run welcome overlay: shown until user presses Enter.
    show_welcome: bool,
    /// Plan mode toggle (--plan flag or /plan command).
    plan_mode: bool,
    /// @file tab-completion candidates.
    tab_completions: Vec<String>,
    tab_completion_idx: usize,
    /// Fork mode: when Some(N), the status bar shows "Fork at turn N" and Enter forks.
    fork_mode: bool,
    /// Approval counts per (tool, first_arg) for learning trust suggestions.
    approval_counts: std::collections::HashMap<(String, String), usize>,
}

struct PendingConfirm {
    tool_name: String,
    preview: String,
    reply: tokio::sync::oneshot::Sender<bool>,
}

/// Returns `true` on the very first launch (before `~/.harness/.welcomed` exists).
fn is_first_run() -> bool {
    let marker = dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/.welcomed");
    !marker.exists()
}

/// Mark that the user has seen the welcome screen.
fn mark_welcomed() {
    if let Some(home) = dirs::home_dir() {
        let dir = home.join(".harness");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(".welcomed"), "1");
    }
}

impl AppState {
    fn new(model: &str) -> Self {
        let show_welcome = is_first_run();
        Self {
            input: String::new(),
            cursor_pos: 0,
            chat: Vec::new(),
            streaming: String::new(),
            event_log: Vec::new(),
            status: if show_welcome {
                "Welcome — press Enter to get started".into()
            } else {
                "Ready — Enter to send · / for commands · @file to pin · Ctrl+C to quit".into()
            },
            busy: false,
            chat_scroll: 0,
            event_scroll: 0,
            session_id: String::new(),
            tokens_in: 0,
            tokens_out: 0,
            model: model.to_string(),
            session_start: std::time::Instant::now(),
            pending_confirm: None,
            show_welcome,
            plan_mode: false,
            tab_completions: Vec::new(),
            tab_completion_idx: 0,
            fork_mode: false,
            approval_counts: std::collections::HashMap::new(),
        }
    }

    /// Return cost estimate string for status bar.
    fn cost_str(&self) -> String {
        if self.tokens_in == 0 && self.tokens_out == 0 {
            return String::new();
        }
        let in_str = cost::format_tokens(self.tokens_in);
        let out_str = cost::format_tokens(self.tokens_out);
        let cost_part = cost::price_for_model(&self.model)
            .map(|p| format!(" · {}", cost::format_cost(p.cost_usd(self.tokens_in, self.tokens_out))))
            .unwrap_or_default();
        format!(" · ↑{in_str} ↓{out_str} tok{cost_part}")
    }

    /// Elapsed time since session start as a short string.
    fn elapsed_str(&self) -> String {
        let secs = self.session_start.elapsed().as_secs();
        if secs < 60 {
            format!(" · {secs}s")
        } else {
            format!(" · {}m{}s", secs / 60, secs % 60)
        }
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self
                .input[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.drain(prev..self.cursor_pos);
            self.cursor_pos = prev;
        }
    }

    fn take_input(&mut self) -> String {
        let s = std::mem::take(&mut self.input);
        self.cursor_pos = 0;
        s
    }

    fn push_event(&mut self, msg: impl Into<String>) {
        let s = msg.into();
        self.event_log.push(s);
        // Keep at most 200 entries
        if self.event_log.len() > 200 {
            self.event_log.remove(0);
        }
    }
}

// ── Slash commands + @file helpers ───────────────────────────────────────────

/// Expand `@<path>` tokens in a prompt into inline file contents.
fn expand_at_files(prompt: &str) -> String {
    let mut result = String::new();
    let mut pinned = String::new();

    for part in prompt.split_whitespace() {
        if let Some(path) = part.strip_prefix('@') {
            match std::fs::read_to_string(path) {
                Ok(contents) => {
                    let ext = std::path::Path::new(path)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    pinned.push_str(&format!(
                        "<file path=\"{path}\">\n```{ext}\n{contents}\n```\n</file>\n"
                    ));
                }
                Err(e) => {
                    pinned.push_str(&format!("[could not read {path}: {e}]\n"));
                }
            }
        }
    }

    // Remove @tokens from the main prompt text.
    let clean: String = prompt
        .split_whitespace()
        .filter(|p| !p.starts_with('@'))
        .collect::<Vec<_>>()
        .join(" ");

    if !pinned.is_empty() {
        result.push_str(&pinned);
        result.push('\n');
    }
    result.push_str(&clean);
    result
}

/// Collect tab-completion candidates for an `@<partial>` suffix.
fn at_file_completions(partial: &str) -> Vec<String> {
    let (dir, prefix) = if let Some(slash) = partial.rfind('/') {
        (&partial[..=slash], &partial[slash + 1..])
    } else {
        ("./", partial)
    };

    let read_dir = std::fs::read_dir(dir).ok();
    let Some(rd) = read_dir else { return vec![] };

    let mut completions: Vec<String> = rd
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) {
                let full = format!("{dir}{name}");
                // Append '/' for directories.
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    Some(format!("{full}/"))
                } else {
                    Some(full)
                }
            } else {
                None
            }
        })
        .collect();

    completions.sort();
    completions.truncate(20);
    completions
}

// ── Public entry point ────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run(
    provider: ArcProvider,
    session_store: SessionStore,
    memory_store: Option<MemoryStore>,
    embed_model: Option<String>,
    tools: ToolExecutor,
    model: String,
    cfg: Config,
    resume_id: Option<&str>,
    ambient_shutdown: Option<watch::Sender<()>>,
    confirm_rx: Option<mpsc::UnboundedReceiver<ConfirmRequest>>,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let has_confirm_gate = confirm_rx.is_some();
    let state = Arc::new(Mutex::new(AppState::new(&model)));
    {
        let mut st = state.lock().unwrap();
        st.plan_mode = has_confirm_gate;
    }
    let mut session = match resume_id {
        Some(id) => session_store
            .find(id)?
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?,
        None => Session::new(&model),
    };
    if !session.messages.is_empty() {
        let mut st = state.lock().unwrap();
        st.session_id = session.short_id().to_string();

        // Replay prior turns into the chat panel so the user can see history.
        for msg in &session.messages {
            use harness_provider_core::Role;
            let content = msg.content.as_str();
            match msg.role {
                Role::System | Role::Tool => continue,
                Role::User => {
                    st.chat.push(ChatMessage {
                        role: "you".to_string(),
                        content: content.to_string(),
                    });
                }
                Role::Assistant => {
                    // Skip internal tool-call blobs
                    if content.starts_with("__tool_calls__:") {
                        continue;
                    }
                    st.chat.push(ChatMessage {
                        role: "grok".to_string(),
                        content: content.to_string(),
                    });
                }
            }
        }

        let turn_count = st.chat.len();
        st.status = format!(
            "Resumed {} · {} · {} turns — scroll ↑ to see history",
            session.short_id(),
            model,
            turn_count
        );
    }

    let result = event_loop(
        &mut terminal,
        state,
        &mut session,
        &provider,
        &session_store,
        memory_store.as_ref(),
        embed_model.as_deref(),
        &tools,
        &model,
        cfg.agent.system_prompt.as_deref().unwrap_or(DEFAULT_SYSTEM),
        ambient_shutdown,
        confirm_rx,
    )
    .await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableBracketedPaste, LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

// ── Main event loop ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn event_loop(
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
    mut confirm_rx: Option<mpsc::UnboundedReceiver<ConfirmRequest>>,
) -> Result<()> {
    // Built once — loads syntect syntax/theme sets (a few hundred ms).
    let highlighter = Highlighter::new();

    // Channel for receiving AgentEvents from the background agent task.
    let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AgentEvent>();
    // Channel for sending the finished session back.
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<harness_memory::Session>();

    loop {
        // Draw current state
        {
            let st = state.lock().unwrap();
            terminal.draw(|f| draw(f, &st, &highlighter))?;
        }

        // Drain any agent events (non-blocking)
        loop {
            match agent_rx.try_recv() {
                Ok(event) => apply_agent_event(&state, event),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => break,
            }
        }

        // Poll for a pending confirmation request (plan mode)
        if state.lock().unwrap().pending_confirm.is_none() {
            if let Some(rx) = &mut confirm_rx {
                if let Ok(req) = rx.try_recv() {
                    let mut st = state.lock().unwrap();
                    st.pending_confirm = Some(PendingConfirm {
                        tool_name: req.tool_name,
                        preview: req.preview,
                        reply: req.reply,
                    });
                    st.status = "PLAN MODE — Enter to approve · Esc to skip".into();
                }
            }
        }

        // Check for a completed session from the background task
        if let Ok(finished) = done_rx.try_recv() {
            *session = finished.clone();
            // Immediate save (no name yet).
            session_store.save(session)?;

            // Auto-name + memory store + re-save in background.
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

            let mut st = state.lock().unwrap();
            st.busy = false;
            st.session_id = session.id[..8].to_string();
            let cost_str = st.cost_str();
            let elapsed = st.elapsed_str();
            st.status = format!(
                "Session {} · {} · {} turns{}{}",
                &session.id[..8],
                model,
                session.messages.len(),
                cost_str,
                elapsed,
            );
        }

        // Handle terminal input events
        if event::poll(std::time::Duration::from_millis(16))? {
            let ev = event::read()?;

            // Bracketed paste: insert text at cursor (or detect image file paths).
            if let Event::Paste(pasted) = &ev {
                let mut st = state.lock().unwrap();
                // If pasted content looks like a file path to an image, convert to @file reference.
                let trimmed = pasted.trim();
                let is_image_path = {
                    let lower = trimmed.to_lowercase();
                    (lower.ends_with(".png") || lower.ends_with(".jpg")
                        || lower.ends_with(".jpeg") || lower.ends_with(".gif")
                        || lower.ends_with(".webp"))
                        && std::path::Path::new(trimmed).exists()
                };
                if is_image_path {
                    st.push_event(format!("[paste] image detected: {trimmed} — appending as @path"));
                    let at_ref = format!("@{trimmed}");
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
                match (key.code, key.modifiers) {
                    (KeyCode::Char('c'), KeyModifiers::CONTROL)
                    | (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                        if let Some(tx) = &ambient_shutdown {
                            let _ = tx.send(());
                        }
                        break;
                    }

                    // Ctrl+E — enter fork mode (edit/fork a past turn)
                    (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                        let mut st = state.lock().unwrap();
                        if st.busy {
                            st.push_event("[fork] agent is running, please wait.".to_string());
                        } else {
                            st.fork_mode = !st.fork_mode;
                            if st.fork_mode {
                                let turns = count_user_turns(&session.messages);
                                st.status = format!("FORK MODE — type turn number (1-{turns}) + Enter to fork, Esc to cancel");
                                st.input.clear();
                                st.cursor_pos = 0;
                            } else {
                                st.status = "Ready — Enter to send · / for commands · @file to pin · Ctrl+C to quit".into();
                            }
                        }
                    }

                    (KeyCode::Esc, _) => {
                        let mut st = state.lock().unwrap();
                        // Cancel fork mode
                        if st.fork_mode {
                            st.fork_mode = false;
                            st.input.clear();
                            st.cursor_pos = 0;
                            st.status = "Fork cancelled.".into();
                        }
                        drop(st);
                        // Deny a pending confirmation
                        let confirm = state.lock().unwrap().pending_confirm.take();
                        if let Some(pc) = confirm {
                            let _ = pc.reply.send(false);
                            let mut st = state.lock().unwrap();
                            st.push_event(format!("[plan] skipped: {}", pc.tool_name));
                            st.status = "Skipped — agent will continue.".into();
                        }
                    }

                    (KeyCode::Enter, _) => {
                        // Dismiss first-run welcome overlay
                        {
                            let mut st = state.lock().unwrap();
                            if st.show_welcome {
                                st.show_welcome = false;
                                st.status =
                                    "Ready — Enter to send · Ctrl+C to quit · ↑↓ to scroll"
                                        .into();
                                mark_welcomed();
                                continue;
                            }
                        }

                        // Fork mode: truncate session to the requested turn.
                        {
                            let fork_active = state.lock().unwrap().fork_mode;
                            if fork_active {
                                let input = state.lock().unwrap().input.trim().to_string();
                                if let Ok(turn_n) = input.parse::<usize>() {
                                    let new_session = fork_session_at(session, turn_n);
                                    *session = new_session;
                                    session_store.save(session)?;
                                    let mut st = state.lock().unwrap();
                                    let new_short_id = session.id[..8.min(session.id.len())].to_string();
                                    st.fork_mode = false;
                                    st.input.clear();
                                    st.cursor_pos = 0;
                                    st.chat.clear();
                                    st.event_log.clear();
                                    st.session_id = new_short_id.clone();
                                    st.push_event(format!("[fork] new session {new_short_id}: forked at turn {turn_n}"));
                                    st.status = format!("Forked at turn {turn_n} — continue from here.");
                                } else {
                                    let mut st = state.lock().unwrap();
                                    st.status = "Fork: enter a valid turn number.".into();
                                }
                                continue;
                            }
                        }

                        // Approve a pending confirmation (plan mode)
                        {
                            let confirm = state.lock().unwrap().pending_confirm.take();
                            if let Some(pc) = confirm {
                                let _ = pc.reply.send(true);
                                let mut st = state.lock().unwrap();
                                st.push_event(format!("[plan] approved: {}", pc.tool_name));
                                st.status = "Approved — agent continuing…".into();

                                // Track approvals; suggest trust after 3 identical approvals.
                                let first_arg = pc.preview.lines().next()
                                    .unwrap_or("").to_string();
                                let key = (pc.tool_name.clone(), first_arg.clone());
                                let count = st.approval_counts.entry(key).or_insert(0);
                                *count += 1;
                                if *count == 3 {
                                    st.push_event(format!(
                                        "[trust] You've approved '{}' 3 times. \
                                         Run: harness trust {} \"{}\" to skip future confirmations.",
                                        pc.tool_name, pc.tool_name, first_arg
                                    ));
                                }
                                continue;
                            }
                        }

                        let busy = state.lock().unwrap().busy;
                        if busy {
                            continue;
                        }
                        let prompt = {
                            let mut st = state.lock().unwrap();
                            st.tab_completions.clear();
                            st.take_input()
                        };
                        if prompt.trim().is_empty() {
                            continue;
                        }

                        // ── Slash commands ────────────────────────────────────────
                        if prompt.trim_start().starts_with('/') {
                            let cmd = prompt.trim();
                            handle_slash_command(
                                cmd,
                                &state,
                                session,
                                provider,
                                session_store,
                                &agent_tx,
                                &done_tx,
                                tools,
                                memory_store,
                                embed_model,
                                system_prompt,
                                model,
                            )
                            .await;
                            continue;
                        }

                        // Expand @file tokens.
                        let expanded = expand_at_files(&prompt);

                        // Record user message in state
                        {
                            let mut st = state.lock().unwrap();
                            st.chat.push(ChatMessage { role: "user".into(), content: prompt.clone() });
                            st.busy = true;
                            st.streaming.clear();
                            st.status = "Thinking…".into();
                            st.event_log.clear();
                        }

                        let send_prompt = if expanded != prompt { expanded } else { prompt.clone() };
                        session.push(Message::user(&send_prompt));

                        // Spawn agent task
                        let p2 = provider.clone();
                        let t2 = tools.clone();
                        let mem2 = memory_store.cloned();
                        let em2 = embed_model.map(|s| s.to_string());
                        let sys = system_prompt.to_string();
                        let atx = agent_tx.clone();
                        let dtx = done_tx.clone();
                        let mut sess_clone = session.clone();

                        tokio::spawn(async move {
                            let _ = agent::drive_agent(
                                &p2,
                                &t2,
                                mem2.as_ref(),
                                em2.as_deref(),
                                &mut sess_clone,
                                &sys,
                                Some(&atx),
                            )
                            .await;
                            let _ = dtx.send(sess_clone);
                        });
                    }

                    (KeyCode::Tab, _) => {
                        // @file tab completion
                        let (input_snap, cursor_snap) = {
                            let st = state.lock().unwrap();
                            (st.input.clone(), st.cursor_pos)
                        };
                        // Find the last @<partial> token before the cursor.
                        let before_cursor = &input_snap[..cursor_snap];
                        if let Some(at_pos) = before_cursor.rfind('@') {
                            let partial = &before_cursor[at_pos + 1..];
                            let mut st = state.lock().unwrap();
                            if st.tab_completions.is_empty() {
                                st.tab_completions = at_file_completions(partial);
                                st.tab_completion_idx = 0;
                            } else {
                                st.tab_completion_idx =
                                    (st.tab_completion_idx + 1) % st.tab_completions.len().max(1);
                            }
                            if let Some(completion) = st.tab_completions.get(st.tab_completion_idx).cloned() {
                                // Replace @<partial> with @<completion>
                                let new_input = format!(
                                    "{}@{}{}",
                                    &input_snap[..at_pos],
                                    completion,
                                    &input_snap[cursor_snap..]
                                );
                                let new_cursor = at_pos + 1 + completion.len();
                                st.input = new_input;
                                st.cursor_pos = new_cursor;
                            }
                        }
                    }

                    (KeyCode::Backspace, _) => {
                        let mut st = state.lock().unwrap();
                        st.tab_completions.clear();
                        st.backspace();
                    }

                    // Scroll chat panel
                    (KeyCode::Up, _) => {
                        let mut st = state.lock().unwrap();
                        st.chat_scroll = st.chat_scroll.saturating_add(1);
                    }
                    (KeyCode::Down, _) => {
                        let mut st = state.lock().unwrap();
                        st.chat_scroll = st.chat_scroll.saturating_sub(1);
                    }

                    // Scroll event log
                    (KeyCode::PageUp, _) => {
                        let mut st = state.lock().unwrap();
                        st.event_scroll = st.event_scroll.saturating_add(5);
                    }
                    (KeyCode::PageDown, _) => {
                        let mut st = state.lock().unwrap();
                        st.event_scroll = st.event_scroll.saturating_sub(5);
                    }

                    (KeyCode::Char(c), _) => {
                        state.lock().unwrap().insert_char(c);
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

// ── Slash command dispatcher ──────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_slash_command(
    cmd: &str,
    state: &Arc<Mutex<AppState>>,
    session: &mut harness_memory::Session,
    provider: &ArcProvider,
    session_store: &harness_memory::SessionStore,
    agent_tx: &mpsc::UnboundedSender<AgentEvent>,
    done_tx: &mpsc::UnboundedSender<harness_memory::Session>,
    tools: &ToolExecutor,
    memory_store: Option<&harness_memory::MemoryStore>,
    embed_model: Option<&str>,
    system_prompt: &str,
    model: &str,
) {
    let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
    let command = parts[0];

    match command {
        "/clear" => {
            let mut st = state.lock().unwrap();
            st.chat.clear();
            st.event_log.clear();
            st.streaming.clear();
            st.status = "Chat cleared.".into();
        }

        "/undo" => {
            let mut st = state.lock().unwrap();
            match crate::checkpoint::undo() {
                Ok(msg) => {
                    st.push_event(format!("[undo] {msg}"));
                    st.status = "Undo complete — files restored from last checkpoint.".into();
                }
                Err(e) => {
                    st.push_event(format!("[undo] {e}"));
                    st.status = format!("Undo failed: {e}");
                }
            }
        }

        "/diff" => {
            {
                let mut st = state.lock().unwrap();
                st.push_event("[diff] running git diff…".to_string());
            }
            match tokio::process::Command::new("git")
                .args(["diff", "--stat", "HEAD"])
                .output()
                .await
            {
                Ok(out) => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let mut st = state.lock().unwrap();
                    for line in text.lines().take(40) {
                        st.push_event(format!("  {line}"));
                    }
                    if text.trim().is_empty() {
                        st.push_event("  (no changes)".to_string());
                    }
                    st.status = "git diff shown in event log.".to_string();
                }
                Err(e) => {
                    state.lock().unwrap().push_event(format!("[diff error] {e}"));
                }
            }
        }

        "/test" => {
            let busy = state.lock().unwrap().busy;
            if busy {
                state.lock().unwrap().push_event("[test] agent is running, please wait.".to_string());
                return;
            }
            let test_cmd = detect_test_command();
            {
                let mut st = state.lock().unwrap();
                st.busy = true;
                st.status = format!("Running: {test_cmd}…");
                st.push_event(format!("[test] {test_cmd}"));
            }
            let atx = agent_tx.clone();
            let cmd_str = test_cmd.clone();
            let state2 = state.clone();
            tokio::spawn(async move {
                let out = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd_str)
                    .output()
                    .await;
                let mut st = state2.lock().unwrap();
                st.busy = false;
                match out {
                    Ok(o) => {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        for line in stdout.lines().chain(stderr.lines()).take(50) {
                            st.push_event(format!("  {line}"));
                        }
                        let status = if o.status.success() { "passed" } else { "FAILED" };
                        st.status = format!("Tests {status}.");
                        let _ = atx.send(AgentEvent::ToolResult {
                            name: "test".to_string(),
                            id: "test".to_string(),
                            result: format!("{stdout}{stderr}"),
                        });
                    }
                    Err(e) => {
                        st.push_event(format!("[test error] {e}"));
                        st.status = format!("Test error: {e}");
                    }
                }
            });
        }

        "/cost" => {
            let st = state.lock().unwrap();
            let in_tok = st.tokens_in;
            let out_tok = st.tokens_out;
            let model_name = st.model.clone();
            drop(st);
            let cost_line = if let Some(price) = cost::price_for_model(&model_name) {
                let usd = price.cost_usd(in_tok, out_tok);
                format!(
                    "Cost: {} ({} in + {} out tokens @ {} model)",
                    cost::format_cost(usd),
                    cost::format_tokens(in_tok),
                    cost::format_tokens(out_tok),
                    model_name
                )
            } else {
                format!(
                    "Tokens: {} in + {} out (no pricing data for {model_name})",
                    cost::format_tokens(in_tok),
                    cost::format_tokens(out_tok)
                )
            };
            let mut st = state.lock().unwrap();
            st.push_event(cost_line.clone());
            st.status = cost_line;
        }

        "/plan" => {
            let mut st = state.lock().unwrap();
            // Toggle plan mode display — actual gate requires restart with --plan flag.
            st.plan_mode = !st.plan_mode;
            if st.plan_mode {
                st.status = "Plan mode ON (restart with --plan to gate tool calls).".to_string();
                st.push_event("[plan] enabled — restart with --plan to fully activate.".to_string());
            } else {
                st.status = "Plan mode OFF.".to_string();
                st.push_event("[plan] disabled.".to_string());
            }
        }

        "/model" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                let st = state.lock().unwrap();
                let msg = format!("[model] current: {}", st.model);
                drop(st);
                state.lock().unwrap().push_event(msg);
            } else {
                let mut st = state.lock().unwrap();
                st.model = name.to_string();
                st.push_event(format!("[model] switched to {name} (applies to new turns)"));
                st.status = format!("Model: {name}");
            }
        }

        "/runs" => {
            match crate::background::list(10) {
                Ok(runs) if runs.is_empty() => {
                    state.lock().unwrap().push_event("[runs] No background runs yet. Use `harness run-bg <prompt>`.".to_string());
                }
                Ok(runs) => {
                    let mut st = state.lock().unwrap();
                    st.push_event(format!("[runs] {} background run(s):", runs.len()));
                    for run in &runs {
                        let prompt_preview = if run.prompt.len() > 50 {
                            format!("{}…", &run.prompt[..50])
                        } else {
                            run.prompt.clone()
                        };
                        st.push_event(format!("  {} [{}] {}", run.id, run.status, prompt_preview));
                    }
                }
                Err(e) => {
                    state.lock().unwrap().push_event(format!("[runs] error: {e}"));
                }
            }
        }

        "/compact" => {
            let busy = state.lock().unwrap().busy;
            if busy {
                state.lock().unwrap().push_event("[compact] agent is running, please wait.".to_string());
                return;
            }
            {
                let mut st = state.lock().unwrap();
                st.push_event("[compact] compacting context…".to_string());
                st.status = "Compacting context…".to_string();
            }
            crate::agent::compact_context(provider, session).await;
            let remaining = session.messages.len();
            let mut st = state.lock().unwrap();
            st.push_event(format!("[compact] done — {remaining} messages remain."));
            st.status = format!("Context compacted ({remaining} messages).");
        }

        "/fork" => {
            state.lock().unwrap().push_event(
                "[fork] Edit/fork past turns coming in Phase C. Use harness --resume in a new terminal for now.".to_string()
            );
        }

        "/help" | "/?" => {
            let mut st = state.lock().unwrap();
            for line in &[
                "/clear    — clear chat panel",
                "/undo     — restore last git checkpoint",
                "/diff     — show git diff in event log",
                "/test     — run test suite",
                "/compact  — compact context (summarise old messages)",
                "/runs     — list background runs (harness run-bg <prompt> to spawn)",
                "/cost     — show token usage + cost estimate",
                "/plan     — toggle plan mode (restart with --plan to gate)",
                "/model X  — switch model for new turns",
                "/fork N   — fork session at turn N (Phase C)",
                "/help     — show this list",
                "",
                "@path     — pin file contents into next message",
                "Tab       — autocomplete @file paths",
            ] {
                st.push_event(line.to_string());
            }
            st.status = "Commands listed in event log →".into();
        }

        _ => {
            state.lock().unwrap().push_event(format!("[unknown command] {cmd}  — type /help for commands"));
        }
    }

    // Suppress "unused" warnings for params only used in some branches.
    let _ = (session, provider, session_store, done_tx, tools, memory_store, embed_model, system_prompt, model);
}

/// Detect the test command for the current project.
fn detect_test_command() -> String {
    if std::path::Path::new("Cargo.toml").exists() {
        "cargo test 2>&1".to_string()
    } else if std::path::Path::new("package.json").exists() {
        "npm test 2>&1".to_string()
    } else if std::path::Path::new("pyproject.toml").exists() || std::path::Path::new("setup.py").exists() {
        "python -m pytest 2>&1".to_string()
    } else if std::path::Path::new("go.mod").exists() {
        "go test ./... 2>&1".to_string()
    } else {
        "make test 2>&1".to_string()
    }
}

// ── Apply incoming agent events to AppState ───────────────────────────────────

fn apply_agent_event(state: &Arc<Mutex<AppState>>, event: AgentEvent) {
    let mut st = state.lock().unwrap();
    match event {
        AgentEvent::TextChunk(chunk) => {
            st.streaming.push_str(&chunk);
        }
        AgentEvent::ToolStart { name, .. } => {
            st.push_event(format!("→ {name}"));
        }
        AgentEvent::ToolResult { name, result, .. } => {
            let preview = result.lines().next().unwrap_or("").chars().take(80).collect::<String>();
            st.push_event(format!("← {name}: {preview}"));
        }
        AgentEvent::MemoryRecall { count } => {
            st.push_event(format!("memory: recalled {count} entries"));
        }
        AgentEvent::SubAgentSpawned { task } => {
            let preview: String = task.chars().take(60).collect();
            st.push_event(format!("swarm ↓ {preview}…"));
        }
        AgentEvent::SubAgentDone { task, .. } => {
            let preview: String = task.chars().take(60).collect();
            st.push_event(format!("swarm ↑ done: {preview}"));
        }
        AgentEvent::TokenUsage { input, output } => {
            st.tokens_in += input as u64;
            st.tokens_out += output as u64;
            st.push_event(format!("tokens in={input} out={output}"));
        }
        AgentEvent::Done => {
            if !st.streaming.is_empty() {
                let text = std::mem::take(&mut st.streaming);
                st.chat.push(ChatMessage { role: "assistant".into(), content: text });
            }
            st.chat_scroll = 0;
            st.event_scroll = 0;
        }
        AgentEvent::Error(msg) => {
            st.push_event(format!("error: {msg}"));
            st.chat.push(ChatMessage { role: "error".into(), content: msg });
        }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn draw(f: &mut ratatui::Frame, state: &AppState, hl: &Highlighter) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),    // main panels
            Constraint::Length(3), // input box
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    // Split main area into chat (62%) and event log (38%)
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(root[0]);

    draw_chat(f, state, main[0], hl);
    draw_event_log(f, state, main[1]);
    draw_input(f, state, root[1]);
    draw_status(f, state, root[2]);

    // First-run welcome overlay (shown before anything else)
    if state.show_welcome {
        draw_welcome_overlay(f);
        return;
    }

    // Plan mode: confirmation overlay on top of everything
    if let Some(pc) = &state.pending_confirm {
        draw_confirm_overlay(f, pc);
    }
}

fn draw_chat(f: &mut ratatui::Frame, state: &AppState, area: ratatui::layout::Rect, hl: &Highlighter) {
    let mut items: Vec<ListItem> = Vec::new();

    for msg in &state.chat {
        let (color, label) = match msg.role.as_str() {
            "user"      => (Color::Cyan,  "you"),
            "assistant" => (Color::Green, "grok"),
            _           => (Color::Red,   "err"),
        };

        // Speaker label
        items.push(ListItem::new(Line::from(Span::styled(
            format!("┌ [{label}]"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))));

        if msg.role == "assistant" {
            // Syntax-highlighted rendering for assistant messages
            let rendered = hl.render_message(
                &msg.content,
                Style::default().fg(Color::White),
            );
            for line in rendered {
                let prefixed = prefix_line(line, "│ ");
                items.push(ListItem::new(prefixed));
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

    // Live streaming text (plain — no highlighting until turn completes)
    if !state.streaming.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "┌ [grok] ●",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ))));
        for line in state.streaming.lines() {
            items.push(ListItem::new(Line::from(Span::styled(
                format!("│ {line}"),
                Style::default().fg(Color::Yellow),
            ))));
        }
    }

    let title = if state.busy { " Chat ● " } else { " Chat " };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .style(Style::default().fg(Color::White));
    f.render_widget(list, area);
}

/// Prepend a margin prefix to every span in a line.
fn prefix_line(line: Line<'static>, prefix: &'static str) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix)];
    spans.extend(line.spans);
    Line::from(spans)
}

fn draw_event_log(f: &mut ratatui::Frame, state: &AppState, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = state
        .event_log
        .iter()
        .map(|line| {
            let color = if line.starts_with('→') {
                Color::Magenta
            } else if line.starts_with('←') {
                Color::Blue
            } else if line.starts_with("error") {
                Color::Red
            } else if line.starts_with("memory") {
                Color::DarkGray
            } else {
                Color::Gray
            };
            ListItem::new(Line::from(Span::styled(line.as_str(), Style::default().fg(color))))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Tools & Events "))
        .style(Style::default().fg(Color::White));
    f.render_widget(list, area);
}

fn draw_input(f: &mut ratatui::Frame, state: &AppState, area: ratatui::layout::Rect) {
    let display = if state.busy {
        "  (agent running…)".to_string()
    } else {
        format!("  {}_", state.input)
    };
    let title = if !state.tab_completions.is_empty() {
        let cur = state.tab_completions.get(state.tab_completion_idx)
            .map(|s| s.as_str())
            .unwrap_or("");
        format!(" Message  [Tab: {cur}] ")
    } else {
        " Message  [/help for commands · @file to pin] ".to_string()
    };
    let input = Paragraph::new(display)
        .block(Block::default().borders(Borders::ALL).title(title))
        .style(Style::default().fg(if state.busy { Color::DarkGray } else { Color::White }))
        .wrap(Wrap { trim: false });
    f.render_widget(input, area);
}

fn draw_status(f: &mut ratatui::Frame, state: &AppState, area: ratatui::layout::Rect) {
    let style = if state.pending_confirm.is_some() {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if state.busy {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let plan_indicator = if state.plan_mode { " [PLAN]" } else { "" };
    let text = format!("{plan_indicator} {}", state.status);
    let p = Paragraph::new(text).style(style);
    f.render_widget(p, area);
}

fn draw_welcome_overlay(f: &mut ratatui::Frame) {
    use ratatui::{layout::Rect, widgets::Clear};

    let area = f.area();
    let width = (area.width as f32 * 0.65).min(72.0) as u16;
    let height = 18u16;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    f.render_widget(Clear, popup_area);

    let lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " Welcome to Harness",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Harness is your AI coding assistant in the terminal.",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Try one of these first prompts:", Style::default().fg(Color::Gray))),
        Line::from(Span::styled(
            "   Read README.md and summarize the project.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "   Run the tests and show me which are failing.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "   Explain what src/main.rs does.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Keybindings:", Style::default().fg(Color::Gray))),
        Line::from(vec![
            Span::styled("   Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" send  "),
            Span::styled("↑/↓", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" scroll  "),
            Span::styled("PgUp/PgDn", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" event log  "),
            Span::styled("Ctrl+C", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" quit"),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Type /help for slash commands · @path to pin files · Tab to complete.",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Press Enter to get started",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " harness — first run ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, popup_area);
}

fn draw_confirm_overlay(f: &mut ratatui::Frame, pc: &PendingConfirm) {
    use ratatui::{layout::Rect, widgets::Clear};

    let area = f.area();
    // Centre a box: 70% wide, up to 20 lines tall
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
        Span::styled(" Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" = approve   "),
        Span::styled("Esc", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw(" = skip"),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));

    let para = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, popup_area);
}

// ── Fork session helpers ──────────────────────────────────────────────────────

/// Count the number of user turns in a message list.
fn count_user_turns(messages: &[harness_provider_core::Message]) -> usize {
    messages.iter().filter(|m| matches!(m.role, harness_provider_core::Role::User)).count()
}

/// Fork a session by creating a new session with messages truncated at the Nth user turn.
/// The new session inherits the model and gets a fresh ID.
fn fork_session_at(original: &harness_memory::Session, turn_n: usize) -> harness_memory::Session {
    use harness_provider_core::Role;
    let mut new_session = harness_memory::Session::new(&original.model);
    // Give it a fork-derived name.
    if let Some(name) = &original.name {
        new_session.name = Some(format!("{name} (fork@{turn_n})"));
    }

    // Copy messages up to and including the Nth user turn.
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
