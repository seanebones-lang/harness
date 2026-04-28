//! Two-panel TUI: chat history (left) + tool/event log (right), input box + status bar.
//! Agent runs in a background tokio task, streaming events via mpsc channel.
//! Code blocks in assistant messages are syntax-highlighted via syntect.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::Message;
use harness_provider_xai::XaiProvider;
use harness_tools::ToolExecutor;
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
    tokens_in: u32,
    tokens_out: u32,
}

impl AppState {
    fn new(_model: &str) -> Self {
        Self {
            input: String::new(),
            cursor_pos: 0,
            chat: Vec::new(),
            streaming: String::new(),
            event_log: Vec::new(),
            status: "Ready — Enter to send · Ctrl+C to quit · ↑↓ to scroll".into(),
            busy: false,
            chat_scroll: 0,
            event_scroll: 0,
            session_id: String::new(),
            tokens_in: 0,
            tokens_out: 0,
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

// ── Public entry point ────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run(
    provider: XaiProvider,
    session_store: SessionStore,
    memory_store: Option<MemoryStore>,
    embed_model: Option<String>,
    tools: ToolExecutor,
    model: String,
    cfg: Config,
    resume_id: Option<&str>,
    ambient_shutdown: Option<watch::Sender<()>>,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let state = Arc::new(Mutex::new(AppState::new(&model)));
    let mut session = match resume_id {
        Some(id) => session_store
            .find(id)?
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?,
        None => Session::new(&model),
    };
    if !session.messages.is_empty() {
        let mut st = state.lock().unwrap();
        st.session_id = session.short_id().to_string();
        st.status = format!(
            "Resumed {} · {} · {} turns",
            session.short_id(),
            model,
            session.messages.len()
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
    )
    .await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

// ── Main event loop ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: Arc<Mutex<AppState>>,
    session: &mut Session,
    provider: &XaiProvider,
    session_store: &SessionStore,
    memory_store: Option<&MemoryStore>,
    embed_model: Option<&str>,
    tools: &ToolExecutor,
    model: &str,
    system_prompt: &str,
    ambient_shutdown: Option<watch::Sender<()>>,
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
            let tok_str = if st.tokens_in > 0 || st.tokens_out > 0 {
                format!(" · ↑{}↓{} tok", st.tokens_in, st.tokens_out)
            } else {
                String::new()
            };
            st.status = format!(
                "Session {} · {} · {} turns{}",
                &session.id[..8],
                model,
                session.messages.len(),
                tok_str,
            );
        }

        // Handle terminal input events
        if event::poll(std::time::Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('c'), KeyModifiers::CONTROL)
                    | (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                        if let Some(tx) = &ambient_shutdown {
                            let _ = tx.send(());
                        }
                        break;
                    }

                    (KeyCode::Enter, _) => {
                        let busy = state.lock().unwrap().busy;
                        if busy {
                            continue;
                        }
                        let prompt = {
                            let mut st = state.lock().unwrap();
                            st.take_input()
                        };
                        if prompt.trim().is_empty() {
                            continue;
                        }

                        // Record user message in state
                        {
                            let mut st = state.lock().unwrap();
                            st.chat.push(ChatMessage { role: "user".into(), content: prompt.clone() });
                            st.busy = true;
                            st.streaming.clear();
                            st.status = "Thinking…".into();
                            st.event_log.clear();
                        }

                        session.push(Message::user(&prompt));

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

                    (KeyCode::Backspace, _) => {
                        state.lock().unwrap().backspace();
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
            st.tokens_in += input;
            st.tokens_out += output;
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
    let input = Paragraph::new(display)
        .block(Block::default().borders(Borders::ALL).title(" Message "))
        .style(Style::default().fg(if state.busy { Color::DarkGray } else { Color::White }))
        .wrap(Wrap { trim: false });
    f.render_widget(input, area);
}

fn draw_status(f: &mut ratatui::Frame, state: &AppState, area: ratatui::layout::Rect) {
    let style = if state.busy {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let p = Paragraph::new(format!(" {}", state.status)).style(style);
    f.render_widget(p, area);
}
