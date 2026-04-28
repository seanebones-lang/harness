//! Minimal ratatui TUI — interactive REPL with session history.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use harness_memory::{Session, SessionStore};
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

use crate::agent::drive_agent;
use crate::config::Config;

struct AppState {
    input: String,
    cursor: usize,
    messages: Vec<(String, String)>, // (role, content)
    status: String,
    scroll: u16,
}

impl AppState {
    fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            messages: Vec::new(),
            status: "Ready — Type a message and press Enter. Ctrl+C to quit.".into(),
            scroll: 0,
        }
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.input[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    fn take_input(&mut self) -> String {
        let s = self.input.clone();
        self.input.clear();
        self.cursor = 0;
        s
    }
}

pub async fn run(
    provider: XaiProvider,
    store: SessionStore,
    tools: ToolExecutor,
    model: String,
    cfg: Config,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut session = Session::new(&model);
    let system_prompt = cfg.agent.system_prompt.clone();
    let state = Arc::new(Mutex::new(AppState::new()));

    // We need to share provider/tools/session across the async boundary.
    // For the TUI loop we process events synchronously and spawn tasks for
    // agent calls, updating state via the Arc<Mutex<>>.
    let result = run_loop(
        &mut terminal,
        &provider,
        &store,
        &tools,
        &mut session,
        system_prompt.as_deref(),
        state,
    )
    .await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    provider: &XaiProvider,
    store: &SessionStore,
    tools: &ToolExecutor,
    session: &mut Session,
    system_prompt: Option<&str>,
    state: Arc<Mutex<AppState>>,
) -> Result<()> {
    loop {
        {
            let st = state.lock().unwrap();
            terminal.draw(|f| draw(f, &st))?;
        }

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Char('q'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Enter, _) => {
                        let prompt = {
                            let mut st = state.lock().unwrap();
                            st.take_input()
                        };
                        if prompt.trim().is_empty() {
                            continue;
                        }

                        {
                            let mut st = state.lock().unwrap();
                            st.messages.push(("user".into(), prompt.clone()));
                            st.status = "Thinking…".into();
                        }

                        use harness_provider_core::Message;
                        session.push(Message::user(&prompt));

                        // Collect streamed response synchronously (TUI refresh happens via poll loop)
                        let result =
                            drive_agent(provider, tools, session, system_prompt).await;

                        match result {
                            Ok(text) => {
                                let mut st = state.lock().unwrap();
                                st.messages.push(("assistant".into(), text));
                                st.status = format!("Session {}", &session.id[..8]);
                            }
                            Err(e) => {
                                let mut st = state.lock().unwrap();
                                st.messages.push(("error".into(), e.to_string()));
                                st.status = "Error — see above".into();
                            }
                        }

                        store.save(session)?;
                    }
                    (KeyCode::Backspace, _) => {
                        state.lock().unwrap().backspace();
                    }
                    (KeyCode::Up, _) => {
                        let mut st = state.lock().unwrap();
                        st.scroll = st.scroll.saturating_add(1);
                    }
                    (KeyCode::Down, _) => {
                        let mut st = state.lock().unwrap();
                        st.scroll = st.scroll.saturating_sub(1);
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

fn draw(f: &mut ratatui::Frame, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // message history
            Constraint::Length(3), // input box
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    // Message history
    let items: Vec<ListItem> = state
        .messages
        .iter()
        .map(|(role, content)| {
            let color = match role.as_str() {
                "user" => Color::Cyan,
                "assistant" => Color::Green,
                _ => Color::Red,
            };
            let label = format!("[{role}] ");
            let line = Line::from(vec![
                Span::styled(label, Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::raw(content.clone()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let history = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" harness — Grok Agent "))
        .style(Style::default().fg(Color::White));
    f.render_widget(history, chunks[0]);

    // Input box
    let input_display = format!("{}_", state.input);
    let input = Paragraph::new(input_display)
        .block(Block::default().borders(Borders::ALL).title(" message "))
        .wrap(Wrap { trim: false });
    f.render_widget(input, chunks[1]);

    // Status bar
    let status = Paragraph::new(state.status.as_str())
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(status, chunks[2]);
}
