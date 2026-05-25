//! Two-panel TUI: chat history (left) + tool/event log (right), input box + status bar.
//! Agent runs in a background tokio task, streaming events via mpsc channel.
//! Code blocks in assistant messages are syntax-highlighted via syntect.
//!
//! E1 improvements (16hr/day daily driver):
//! - Scroll actually works (stateful List widgets)
//! - Input history (Up/Down when empty, Ctrl+R reverse search)
//! - Multi-line compose (Shift+Enter / Alt+Enter)
//! - Full readline bindings (Left/Right/Home/End/Del/Ctrl+A/E/W/U/K)
//! - Y/N/A confirm overlay
//! - Voice moved to Ctrl+S; Ctrl+V pastes as expected
//! - Slash command autocomplete popup
//! - Ctrl+F in-chat search
//! - Ctrl+Y copy last response
//! - Tool output expansion (Enter on event log item)
//! - /sessions browser
//! - Timestamps (/ts toggle)
//! - Resizable panel split (Ctrl+[ / Ctrl+])
//! - Mouse scroll support
//! - Theme system (theme.toml)
//! - Role label uses model name, not hardcoded "grok"
//! - Spinner during long operations
//! - Status bar: persistent cost/tokens, transient messages above

use std::time::Instant;

use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::ArcProvider;
use harness_tools::{ConfirmRequest, ToolExecutor};
use parking_lot::Mutex;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::watch;

use crate::agent::DEFAULT_SYSTEM;
use crate::config::Config;

mod confirm_flow;
mod driver;
mod events;
mod input;
mod render;
mod slash;
mod state;
mod theme;

pub(crate) use state::{mark_welcomed, AppState, ChatMessage, PendingConfirm};

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
    initial_thinking_budget: Option<u32>,
    ambient_shutdown: Option<watch::Sender<()>>,
    confirm_rx: Option<mpsc::Receiver<ConfirmRequest>>,
    confirm_bar_label: Option<&'static str>,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let has_confirm_gate = confirm_rx.is_some();
    // Labels + status must reflect the live provider model, not stale config text.
    let live_model = harness_provider_core::Provider::model(provider.as_ref()).to_string();
    let state = Arc::new(Mutex::new(AppState::new(&live_model)));
    {
        let mut st = state.lock();
        st.plan_mode = has_confirm_gate;
        st.confirm_bar_label = confirm_bar_label.map(str::to_string);
        st.computer_use_active = cfg.computer_use.is_enabled();
        st.budget_daily_usd = cfg.budget.daily_usd;
        st.budget_monthly_usd = cfg.budget.monthly_usd;
        st.notifications = cfg.notifications.clone();
        st.thinking_budget = initial_thinking_budget;
        if model != live_model {
            st.push_event(format!(
                "[model] config `{model}` — live provider uses `{live_model}`"
            ));
        }
    }
    let mut session = match resume_id {
        Some(id) => session_store
            .find(id)?
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?,
        None => Session::new(&live_model),
    };

    if let Some(id) = resume_id {
        let mut st = state.lock();
        st.session_id = id[..8.min(id.len())].to_string();
        st.session_id_full = id.to_string();
        st.model = live_model.clone();
        for msg in &session.messages {
            let role = match msg.role {
                harness_provider_core::Role::User => "user",
                harness_provider_core::Role::Assistant => "assistant",
                harness_provider_core::Role::Tool | harness_provider_core::Role::System => continue,
            };
            if let harness_provider_core::MessageContent::Text(text) = &msg.content {
                st.chat.push(ChatMessage {
                    role: role.to_string(),
                    content: text.clone(),
                    ts: Instant::now(),
                });
            }
        }
    }

    let system_prompt = {
        let loaded = crate::agent::load_project_instructions();
        let base = cfg.agent.system_prompt.as_deref().unwrap_or(DEFAULT_SYSTEM);
        if let Some(proj) = loaded {
            format!("{base}\n\n{proj}")
        } else {
            base.to_string()
        }
    };

    let result = driver::run_terminal_loop(
        &mut terminal,
        state,
        &mut session,
        &provider,
        &session_store,
        memory_store.as_ref(),
        embed_model.as_deref(),
        &tools,
        &model,
        &system_prompt,
        cfg.native_tools.web_search_enabled(),
        cfg.native_tools.code_execution_enabled(),
        cfg.native_tools.x_search_enabled(),
        ambient_shutdown,
        confirm_rx,
    )
    .await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    result
}
