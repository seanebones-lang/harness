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

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
        MouseEvent, MouseEventKind,
        DisableBracketedPaste, EnableBracketedPaste,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::{ArcProvider, Message};
use harness_tools::{ConfirmRequest, ToolExecutor};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::io;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::watch;

use crate::agent::{self, DEFAULT_SYSTEM};
use crate::config::Config;
use crate::cost;
use crate::cost_db::{self, CostDb};
use crate::events::AgentEvent;
use crate::highlight::Highlighter;

// ── Constants ──────────────────────────────────────────────────────────────────

const HISTORY_MAX: usize = 1000;
const SPINNER_CHARS: &[char] = &['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
const DEFAULT_RIGHT_PCT: u8 = 38;

// ── App state ─────────────────────────────────────────────────────────────────

struct ChatMessage {
    role: String,
    content: String,
    ts: Instant,
}

#[derive(Clone)]
struct Theme {
    user_color: Color,
    assistant_color: Color,
    streaming_color: Color,
    error_color: Color,
    tool_in_color: Color,
    tool_out_color: Color,
    dim_color: Color,
    border_color: Color,
    accent_color: Color,
    search_hl_color: Color,
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
    fn load() -> Self {
        let path = dirs::home_dir().unwrap_or_default().join(".harness/theme.toml");
        if !path.exists() {
            return Self::default();
        }
        let Ok(text) = std::fs::read_to_string(&path) else { return Self::default(); };
        let Ok(val) = text.parse::<toml::Value>() else { return Self::default(); };
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

    fn assistant_label<'a>(&self, model: &str) -> &'a str {
        // Return a short provider-friendly label for the model
        if model.contains("claude") { "claude" }
        else if model.contains("grok") { "grok" }
        else if model.contains("gpt") { "gpt" }
        else if model.contains("qwen") { "qwen" }
        else { "ai" }
    }
}

fn parse_color(s: &str) -> Option<Color> {
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

#[allow(clippy::struct_excessive_bools)]
struct AppState {
    input: String,
    cursor_pos: usize,
    /// Finalized chat messages shown in the left panel.
    chat: Vec<ChatMessage>,
    /// Current streaming assistant text (rendered live at bottom of chat).
    streaming: String,
    /// Event log shown in the right panel.
    event_log: Vec<String>,
    /// Status bar text (transient, ephemeral message).
    status: String,
    /// Persistent info shown on the right of the status bar.
    status_right: String,
    /// Is the agent currently running?
    busy: bool,
    /// Scroll state for chat list.
    chat_scroll: ListState,
    /// Scroll state for event log.
    event_scroll: ListState,
    /// Total rendered chat item count (updated on draw for scroll bounds).
    chat_items_len: usize,
    /// Total event log item count.
    event_items_len: usize,
    /// Session id for display.
    session_id: String,
    /// Cumulative token counts for this session.
    tokens_in: u64,
    tokens_out: u64,
    /// Anthropic prompt-cache stats.
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    /// Active model name.
    model: String,
    /// Session start time.
    session_start: Instant,
    /// Plan mode: pending confirmation request.
    pending_confirm: Option<PendingConfirm>,
    /// First-run welcome overlay.
    show_welcome: bool,
    /// Plan mode toggle.
    plan_mode: bool,
    /// @file tab-completion candidates.
    tab_completions: Vec<String>,
    tab_completion_idx: usize,
    /// Fork mode.
    fork_mode: bool,
    /// Extended thinking budget.
    thinking_budget: Option<u32>,
    /// Is a voice recording in progress?
    recording_voice: bool,
    /// Is computer use active?
    computer_use_active: bool,
    /// Cost database handle.
    cost_db: Option<CostDb>,
    /// Full session ID.
    session_id_full: String,
    /// Budget limits.
    budget_daily_usd: Option<f64>,
    budget_monthly_usd: Option<f64>,
    /// Approval counts for trust suggestions.
    approval_counts: std::collections::HashMap<(String, String), usize>,
    /// Notifications config.
    notifications: crate::config::NotificationsConfig,
    // ── E1 new fields ─────────────────────────────────────────────────────────
    /// Input history (most recent at index 0).
    input_history: VecDeque<String>,
    /// Current history navigation position (None = live input).
    history_idx: Option<usize>,
    /// Saved live input when navigating history.
    history_saved: String,
    /// Search mode active.
    search_mode: bool,
    /// Current search query.
    search_query: String,
    /// Chat indices (into chat vec) that match current search.
    search_matches: Vec<usize>,
    /// Current match position (index into search_matches).
    search_match_pos: usize,
    /// Slash command autocomplete: matching commands.
    slash_suggestions: Vec<String>,
    /// Which suggestion is selected.
    slash_suggest_idx: usize,
    /// Right panel percentage (default 38).
    right_panel_pct: u8,
    /// Timestamps visible.
    timestamps_visible: bool,
    /// Spinner frame.
    spinner_frame: usize,
    /// Last spinner tick.
    spinner_tick: Instant,
    /// When the current tool started (for elapsed display).
    tool_start: Option<Instant>,
    /// Expanded event log item index (None = none expanded).
    _expanded_event: Option<usize>,
    /// Full text for expanded event.
    expanded_event_text: String,
    /// Focus mode: silence notifications until this time.
    focus_until: Option<Instant>,
    /// Theme.
    theme: Theme,
    /// Active response schema for strict JSON output (set via /schema).
    response_schema: Option<harness_provider_core::ResponseSchema>,
}

struct PendingConfirm {
    tool_name: String,
    preview: String,
    reply: tokio::sync::oneshot::Sender<bool>,
}

fn is_first_run() -> bool {
    let marker = dirs::home_dir().unwrap_or_default().join(".harness/.welcomed");
    !marker.exists()
}

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
        let mut chat_scroll = ListState::default();
        chat_scroll.select(None);
        let mut event_scroll = ListState::default();
        event_scroll.select(None);

        Self {
            input: String::new(),
            cursor_pos: 0,
            chat: Vec::new(),
            streaming: String::new(),
            event_log: Vec::new(),
            status: if show_welcome {
                "Welcome — press Enter to get started".into()
            } else {
                "Ready".into()
            },
            status_right: String::new(),
            busy: false,
            chat_scroll,
            event_scroll,
            chat_items_len: 0,
            event_items_len: 0,
            session_id: String::new(),
            tokens_in: 0,
            tokens_out: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: model.to_string(),
            session_start: Instant::now(),
            pending_confirm: None,
            show_welcome,
            plan_mode: false,
            tab_completions: Vec::new(),
            tab_completion_idx: 0,
            fork_mode: false,
            thinking_budget: None,
            recording_voice: false,
            computer_use_active: false,
            cost_db: CostDb::open().ok(),
            session_id_full: String::new(),
            budget_daily_usd: None,
            budget_monthly_usd: None,
            approval_counts: std::collections::HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            input_history: VecDeque::with_capacity(HISTORY_MAX),
            history_idx: None,
            history_saved: String::new(),
            search_mode: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_match_pos: 0,
            slash_suggestions: Vec::new(),
            slash_suggest_idx: 0,
            right_panel_pct: DEFAULT_RIGHT_PCT,
            timestamps_visible: false,
            spinner_frame: 0,
            spinner_tick: Instant::now(),
            tool_start: None,
            _expanded_event: None,
            expanded_event_text: String::new(),
            focus_until: None,
            theme: Theme::load(),
            response_schema: None,
        }
    }

    fn cost_str(&self) -> String {
        if self.tokens_in == 0 && self.tokens_out == 0 {
            return String::new();
        }
        let in_str = cost::format_tokens(self.tokens_in);
        let out_str = cost::format_tokens(self.tokens_out);
        let cost_part = cost::price_for_model(&self.model)
            .map(|p| {
                let usd = p.cost_with_cache(self.tokens_in, self.cache_read_tokens, self.tokens_out);
                format!(" {}", cost::format_cost(usd))
            })
            .unwrap_or_default();
        let cache_part = if self.cache_read_tokens > 0 {
            let pct = self.cache_read_tokens * 100 / self.tokens_in.max(1);
            format!(" cache:{pct}%")
        } else {
            String::new()
        };
        format!("↑{in_str} ↓{out_str}{cost_part}{cache_part}")
    }

    fn elapsed_str(&self) -> String {
        let secs = self.session_start.elapsed().as_secs();
        if secs < 60 { format!("{secs}s") } else { format!("{}m{}s", secs / 60, secs % 60) }
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
        self.update_slash_suggestions();
    }

    fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.input[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.drain(prev..self.cursor_pos);
            self.cursor_pos = prev;
            self.update_slash_suggestions();
        }
    }

    fn delete_forward(&mut self) {
        if self.cursor_pos < self.input.len() {
            let next = self.input[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.input.len());
            self.input.drain(self.cursor_pos..next);
        }
    }

    fn move_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.input[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    fn move_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos += self.input[self.cursor_pos..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
        }
    }

    fn move_word_left(&mut self) {
        let s = &self.input[..self.cursor_pos];
        let trimmed = s.trim_end();
        let new_pos = trimmed.rfind(|c: char| c == ' ' || c == '/' || c == '.').map(|i| i + 1).unwrap_or(0);
        self.cursor_pos = new_pos;
    }

    fn move_word_right(&mut self) {
        let s = &self.input[self.cursor_pos..];
        let trimmed = s.trim_start();
        let skip = s.len() - trimmed.len();
        let word_end = trimmed.find(|c: char| c == ' ' || c == '/' || c == '.').map(|i| i + skip + 1).unwrap_or(s.len());
        self.cursor_pos += word_end;
    }

    fn kill_word_back(&mut self) {
        let original = self.cursor_pos;
        self.move_word_left();
        let new_pos = self.cursor_pos;
        self.input.drain(new_pos..original);
        self.cursor_pos = new_pos;
    }

    fn kill_to_end(&mut self) {
        self.input.truncate(self.cursor_pos);
    }

    fn kill_line(&mut self) {
        self.input.clear();
        self.cursor_pos = 0;
        self.update_slash_suggestions();
    }

    fn take_input(&mut self) -> String {
        let s = std::mem::take(&mut self.input);
        self.cursor_pos = 0;
        self.slash_suggestions.clear();
        if !s.trim().is_empty() {
            // Push to history (dedup adjacent)
            if self.input_history.front().map(|f| f != &s).unwrap_or(true) {
                self.input_history.push_front(s.clone());
                if self.input_history.len() > HISTORY_MAX {
                    self.input_history.pop_back();
                }
            }
        }
        self.history_idx = None;
        self.history_saved.clear();
        s
    }

    fn history_up(&mut self) {
        if self.input_history.is_empty() { return; }
        let new_idx = match self.history_idx {
            None => {
                self.history_saved = self.input.clone();
                0
            }
            Some(i) if i + 1 < self.input_history.len() => i + 1,
            Some(i) => i,
        };
        self.history_idx = Some(new_idx);
        self.input = self.input_history[new_idx].clone();
        self.cursor_pos = self.input.len();
    }

    fn history_down(&mut self) {
        match self.history_idx {
            None => {}
            Some(0) => {
                self.history_idx = None;
                self.input = std::mem::take(&mut self.history_saved);
                self.cursor_pos = self.input.len();
            }
            Some(i) => {
                let new_idx = i - 1;
                self.history_idx = Some(new_idx);
                self.input = self.input_history[new_idx].clone();
                self.cursor_pos = self.input.len();
            }
        }
    }

    fn push_event(&mut self, msg: impl Into<String>) {
        let s = msg.into();
        self.event_log.push(s);
        if self.event_log.len() > 500 {
            self.event_log.remove(0);
        }
        // Auto-scroll event log to bottom
        if self.event_items_len > 0 {
            let last = self.event_items_len.saturating_sub(1);
            self.event_scroll.select(Some(last));
        }
    }

    fn scroll_chat_up(&mut self, n: usize) {
        let cur = self.chat_scroll.selected().unwrap_or(self.chat_items_len.saturating_sub(1));
        let new = cur.saturating_sub(n);
        self.chat_scroll.select(Some(new));
    }

    fn scroll_chat_down(&mut self, n: usize) {
        let cur = self.chat_scroll.selected().unwrap_or(0);
        let max = self.chat_items_len.saturating_sub(1);
        let new = (cur + n).min(max);
        self.chat_scroll.select(Some(new));
    }

    fn scroll_event_up(&mut self, n: usize) {
        let cur = self.event_scroll.selected().unwrap_or(self.event_items_len.saturating_sub(1));
        let new = cur.saturating_sub(n);
        self.event_scroll.select(Some(new));
    }

    fn scroll_event_down(&mut self, n: usize) {
        let cur = self.event_scroll.selected().unwrap_or(0);
        let max = self.event_items_len.saturating_sub(1);
        let new = (cur + n).min(max);
        self.event_scroll.select(Some(new));
    }

    fn scroll_to_bottom(&mut self) {
        let max = self.chat_items_len.saturating_sub(1);
        self.chat_scroll.select(Some(max));
        let emax = self.event_items_len.saturating_sub(1);
        self.event_scroll.select(Some(emax));
    }

    fn update_slash_suggestions(&mut self) {
        const ALL_COMMANDS: &[(&str, &str)] = &[
            ("/clear", "clear chat panel"),
            ("/undo", "restore last checkpoint"),
            ("/diff", "show git diff"),
            ("/test", "run test suite"),
            ("/compact", "compact context"),
            ("/runs", "list background runs"),
            ("/cost", "show cost estimate"),
            ("/plan", "toggle plan mode"),
            ("/model", "switch model (e.g. /model claude-haiku-4-5)"),
            ("/think", "set thinking budget (e.g. /think 10000)"),
            ("/remember", "store fact (e.g. /remember arch: monorepo)"),
            ("/forget", "delete memory topic"),
            ("/memories", "list memory topics"),
            ("/sessions", "browse previous sessions"),
            ("/pr", "list PRs or load PR #N"),
            ("/issues", "list GitHub issues"),
            ("/ci", "show CI workflow runs"),
            ("/notify", "test desktop notification"),
            ("/ts", "toggle message timestamps"),
            ("/focus", "silence notifications for N minutes"),
            ("/obsidian", "save to Obsidian vault"),
            ("/trace", "show/replay last turn trace"),
            ("/schema", "set structured output JSON schema"),
            ("/help", "show all commands"),
        ];
        let trimmed = self.input.trim_start();
        if !trimmed.starts_with('/') || trimmed.contains(' ') {
            self.slash_suggestions.clear();
            return;
        }
        let partial = trimmed.to_lowercase();
        self.slash_suggestions = ALL_COMMANDS
            .iter()
            .filter(|(cmd, _)| cmd.to_lowercase().starts_with(&partial))
            .map(|(cmd, desc)| format!("{cmd}  — {desc}"))
            .collect();
        self.slash_suggest_idx = 0;
    }

    fn tick_spinner(&mut self) {
        if self.spinner_tick.elapsed() >= Duration::from_millis(120) {
            self.spinner_frame = (self.spinner_frame + 1) % SPINNER_CHARS.len();
            self.spinner_tick = Instant::now();
        }
    }

    fn spinner_char(&self) -> char {
        SPINNER_CHARS[self.spinner_frame]
    }

    fn focus_active(&self) -> bool {
        self.focus_until.map(|t| Instant::now() < t).unwrap_or(false)
    }

    fn focus_mins_remaining(&self) -> u64 {
        self.focus_until
            .map(|t| t.saturating_duration_since(Instant::now()).as_secs() / 60 + 1)
            .unwrap_or(0)
    }
}

// ── Slash commands + @file helpers ────────────────────────────────────────────

fn expand_at_files(prompt: &str) -> String {
    let mut result = String::new();
    let mut pinned = String::new();
    let mut text_parts = Vec::new();

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
        } else {
            text_parts.push(part);
        }
    }

    result.push_str(&text_parts.join(" "));
    if !pinned.is_empty() {
        result.push_str("\n\n");
        result.push_str(&pinned);
    }
    result
}

fn at_file_completions(partial: &str) -> Vec<String> {
    let dir = if let Some(slash) = partial.rfind('/') {
        partial[..=slash].to_string()
    } else {
        String::new()
    };
    let file_prefix = if let Some(slash) = partial.rfind('/') {
        partial[slash + 1..].to_string()
    } else {
        partial.to_string()
    };

    let search_dir = if dir.is_empty() { ".".to_string() } else { dir.clone() };
    let Ok(entries) = std::fs::read_dir(&search_dir) else { return vec![] };

    let mut results: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&file_prefix) {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let full = format!("{}{}{}", dir, name, if is_dir { "/" } else { "" });
                Some(full)
            } else {
                None
            }
        })
        .collect();
    results.sort();
    results.truncate(20);
    results
}

// ── Entry point ───────────────────────────────────────────────────────────────

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
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let has_confirm_gate = confirm_rx.is_some();
    let state = Arc::new(Mutex::new(AppState::new(&model)));
    {
        let mut st = state.lock().unwrap();
        st.plan_mode = has_confirm_gate;
        st.computer_use_active = cfg.computer_use.is_enabled();
        st.budget_daily_usd = cfg.budget.daily_usd;
        st.budget_monthly_usd = cfg.budget.monthly_usd;
        st.notifications = cfg.notifications.clone();
    }
    let mut session = match resume_id {
        Some(id) => session_store
            .find(id)?
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?,
        None => Session::new(&model),
    };

    if let Some(id) = resume_id {
        let mut st = state.lock().unwrap();
        st.session_id = id[..8.min(id.len())].to_string();
        st.session_id_full = id.to_string();
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
        &system_prompt,
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

// ── Main event loop ────────────────────────────────────────────────────────────

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
    let highlighter = Highlighter::new();
    let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AgentEvent>();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<harness_memory::Session>();

    loop {
        // Spinner tick
        {
            let mut st = state.lock().unwrap();
            if st.busy { st.tick_spinner(); }
        }

        // Draw
        {
            let mut st = state.lock().unwrap();
            let hl = &highlighter;
            let theme = st.theme.clone();
            terminal.draw(|f| draw_all(f, &mut st, hl, &theme))?;
        }

        // Drain agent events
        loop {
            match agent_rx.try_recv() {
                Ok(ev) => apply_agent_event(&state, ev),
                Err(_) => break,
            }
        }

        // Poll for confirmation requests
        if state.lock().unwrap().pending_confirm.is_none() {
            if let Some(rx) = &mut confirm_rx {
                if let Ok(req) = rx.try_recv() {
                    let mut st = state.lock().unwrap();
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
            let mut st = state.lock().unwrap();
            st.busy = false;
            st.tool_start = None;
            st.session_id = session.id[..8].to_string();
            let cost_str = st.cost_str();
            let elapsed = st.elapsed_str();
            let turns = session.messages.len();
            st.status = "Done".to_string();
            st.status_right = format!("{} · {} · {} turns · {} · {}", &session.id[..8], model, turns, cost_str, elapsed);
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
                let mut st = state.lock().unwrap();
                let trimmed = pasted.trim();
                let is_image_path = {
                    let lower = trimmed.to_lowercase();
                    (lower.ends_with(".png") || lower.ends_with(".jpg")
                        || lower.ends_with(".jpeg") || lower.ends_with(".gif")
                        || lower.ends_with(".webp"))
                        && std::path::Path::new(trimmed).exists()
                };
                if is_image_path {
                    st.push_event(format!("[paste] image → {trimmed}"));
                    let at_ref = format!("@{trimmed} ");
                    for c in at_ref.chars() { st.insert_char(c); }
                } else {
                    for c in pasted.chars() { st.insert_char(c); }
                }
                continue;
            }

            if let Event::Key(key) = ev {
                // Search mode intercept
                {
                    let search = state.lock().unwrap().search_mode;
                    if search {
                        if handle_search_key(&state, key) { continue; }
                    }
                }

                match (key.code, key.modifiers) {
                    // ── Quit ─────────────────────────────────────────────────
                    (KeyCode::Char('c'), KeyModifiers::CONTROL)
                    | (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                        if let Some(tx) = &ambient_shutdown { let _ = tx.send(()); }
                        break;
                    }

                    // ── Voice (moved from Ctrl+V to Ctrl+S) ──────────────────
                    (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                        handle_voice(&state);
                    }

                    // ── Ctrl+F / forward-slash focus → search ─────────────────
                    (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                        let mut st = state.lock().unwrap();
                        st.search_mode = true;
                        st.search_query.clear();
                        st.search_matches.clear();
                        st.status = "Search: ".to_string();
                    }

                    // ── Ctrl+Y — copy last response ───────────────────────────
                    (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                        let last = state.lock().unwrap()
                            .chat.iter().rev()
                            .find(|m| m.role == "assistant")
                            .map(|m| m.content.clone());
                        if let Some(text) = last {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(&text);
                                state.lock().unwrap().status = "Copied last response.".to_string();
                            }
                        }
                    }

                    // ── Ctrl+E — fork mode ────────────────────────────────────
                    (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                        let mut st = state.lock().unwrap();
                        if st.busy {
                            st.push_event("[fork] agent running, please wait.");
                        } else {
                            st.fork_mode = !st.fork_mode;
                            if st.fork_mode {
                                let turns = count_user_turns(&session.messages);
                                st.status = format!("FORK MODE — enter turn (1-{turns}) + Enter to fork, Esc to cancel");
                                st.input.clear(); st.cursor_pos = 0;
                            } else {
                                st.status = "Ready".to_string();
                            }
                        }
                    }

                    // ── Ctrl+] / Ctrl+[ — resize panels ───────────────────────
                    (KeyCode::Char(']'), KeyModifiers::CONTROL) => {
                        let mut st = state.lock().unwrap();
                        st.right_panel_pct = st.right_panel_pct.saturating_add(5).min(70);
                        st.status = format!("Right panel: {}%", st.right_panel_pct);
                    }
                    (KeyCode::Char('['), KeyModifiers::CONTROL) => {
                        let mut st = state.lock().unwrap();
                        st.right_panel_pct = st.right_panel_pct.saturating_sub(5).max(20);
                        st.status = format!("Right panel: {}%", st.right_panel_pct);
                    }

                    // ── Ctrl+L — scroll to bottom ─────────────────────────────
                    (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                        state.lock().unwrap().scroll_to_bottom();
                    }

                    // ── Esc ───────────────────────────────────────────────────
                    (KeyCode::Esc, _) => {
                        let mut st = state.lock().unwrap();
                        if st.fork_mode {
                            st.fork_mode = false;
                            st.input.clear(); st.cursor_pos = 0;
                            st.status = "Fork cancelled.".to_string();
                        }
                        drop(st);
                        let confirm = state.lock().unwrap().pending_confirm.take();
                        if let Some(pc) = confirm {
                            let _ = pc.reply.send(false);
                            let mut st = state.lock().unwrap();
                            st.push_event(format!("[plan] skipped: {}", pc.tool_name));
                            st.status = "Skipped.".to_string();
                        }
                    }

                    // ── Y — approve confirm ────────────────────────────────────
                    (KeyCode::Char('y'), KeyModifiers::NONE) => {
                        let confirm = state.lock().unwrap().pending_confirm.take();
                        if let Some(pc) = confirm {
                            approve_confirm(&state, pc);
                            continue;
                        }
                        // Otherwise insert 'y' normally
                        handle_char(&state, 'y');
                    }

                    // ── N — deny confirm ───────────────────────────────────────
                    (KeyCode::Char('n'), KeyModifiers::NONE) => {
                        let confirm = state.lock().unwrap().pending_confirm.take();
                        if let Some(pc) = confirm {
                            let _ = pc.reply.send(false);
                            let mut st = state.lock().unwrap();
                            st.push_event(format!("[plan] denied: {}", pc.tool_name));
                            st.status = "Denied.".to_string();
                            continue;
                        }
                        handle_char(&state, 'n');
                    }

                    // ── A — always allow ──────────────────────────────────────
                    (KeyCode::Char('a'), KeyModifiers::NONE) => {
                        let has_confirm = state.lock().unwrap().pending_confirm.is_some();
                        if has_confirm {
                            let confirm = state.lock().unwrap().pending_confirm.take();
                            if let Some(pc) = confirm {
                                let tool = pc.tool_name.clone();
                                let first_arg = pc.preview.lines().next().unwrap_or("").to_string();
                                approve_confirm(&state, pc);
                                // Emit trust suggestion
                                state.lock().unwrap().push_event(
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
                            state.lock().unwrap().insert_char('\n');
                            continue;
                        }

                        // Welcome dismiss
                        {
                            let mut st = state.lock().unwrap();
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
                            let fork_active = state.lock().unwrap().fork_mode;
                            if fork_active {
                                let input = state.lock().unwrap().input.trim().to_string();
                                if let Ok(turn_n) = input.parse::<usize>() {
                                    let new_session = fork_session_at(session, turn_n);
                                    *session = new_session;
                                    session_store.save(session)?;
                                    let mut st = state.lock().unwrap();
                                    let short = session.id[..8.min(session.id.len())].to_string();
                                    st.fork_mode = false;
                                    st.input.clear(); st.cursor_pos = 0;
                                    st.chat.clear(); st.event_log.clear();
                                    st.session_id = short.clone();
                                    st.push_event(format!("[fork] session {short} forked at turn {turn_n}"));
                                    st.status = format!("Forked at turn {turn_n} — continue here.");
                                } else {
                                    state.lock().unwrap().status = "Fork: enter a valid turn number.".to_string();
                                }
                                continue;
                            }
                        }

                        // Approve pending confirm
                        {
                            let confirm = state.lock().unwrap().pending_confirm.take();
                            if let Some(pc) = confirm {
                                approve_confirm(&state, pc);
                                continue;
                            }
                        }

                        let busy = state.lock().unwrap().busy;
                        if busy { continue; }

                        let prompt = {
                            let mut st = state.lock().unwrap();
                            st.tab_completions.clear();
                            st.slash_suggestions.clear();
                            st.take_input()
                        };
                        if prompt.trim().is_empty() { continue; }

                        // Slash commands
                        if prompt.trim_start().starts_with('/') {
                            let cmd = prompt.trim();
                            handle_slash_command(
                                cmd, &state, session, provider, session_store,
                                &agent_tx, &done_tx, tools, memory_store, embed_model,
                                system_prompt, model,
                            ).await;
                            continue;
                        }

                        // Expand @file tokens
                        let expanded = expand_at_files(&prompt);

                        {
                            let mut st = state.lock().unwrap();
                            let label = if prompt.len() > 100 {
                                format!("{}…", &prompt[..100])
                            } else {
                                prompt.clone()
                            };
                            st.chat.push(ChatMessage { role: "user".into(), content: label, ts: Instant::now() });
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
                        let think_budget = state.lock().unwrap().thinking_budget;
                        let resp_schema = state.lock().unwrap().response_schema.clone();

                        tokio::spawn(async move {
                            let res = agent::drive_agent_with_schema(
                                &p2, &t2, mem2.as_ref(), em2.as_deref(),
                                &mut sess_clone, &sys, Some(&atx), think_budget, resp_schema,
                            ).await;
                            if let Err(e) = res {
                                let _ = atx.send(AgentEvent::Error(format!("Agent error: {e}")));
                            }
                            let _ = dtx.send(sess_clone);
                        });
                    }

                    // ── Tab — @file completion or slash completion ─────────────
                    (KeyCode::Tab, _) => {
                        // Slash suggestion completion
                        {
                            let has_slash = !state.lock().unwrap().slash_suggestions.is_empty();
                            if has_slash {
                                let mut st = state.lock().unwrap();
                                st.slash_suggest_idx = (st.slash_suggest_idx + 1) % st.slash_suggestions.len();
                                // Apply selected command to input (strip description)
                                let selected = st.slash_suggestions[st.slash_suggest_idx].clone();
                                let cmd = selected.split("  —").next().unwrap_or("").trim().to_string();
                                st.input = cmd.clone();
                                st.cursor_pos = cmd.len();
                                continue;
                            }
                        }
                        // @file completion
                        let (input_snap, cursor_snap) = {
                            let st = state.lock().unwrap();
                            (st.input.clone(), st.cursor_pos)
                        };
                        let before_cursor = &input_snap[..cursor_snap];
                        if let Some(at_pos) = before_cursor.rfind('@') {
                            let partial = &before_cursor[at_pos + 1..];
                            let mut st = state.lock().unwrap();
                            if st.tab_completions.is_empty() {
                                st.tab_completions = at_file_completions(partial);
                                st.tab_completion_idx = 0;
                            } else {
                                st.tab_completion_idx = (st.tab_completion_idx + 1) % st.tab_completions.len().max(1);
                            }
                            if let Some(c) = st.tab_completions.get(st.tab_completion_idx).cloned() {
                                let new_input = format!("{}@{}{}", &input_snap[..at_pos], c, &input_snap[cursor_snap..]);
                                let new_cursor = at_pos + 1 + c.len();
                                st.input = new_input;
                                st.cursor_pos = new_cursor;
                            }
                        }
                    }

                    // ── Backspace ─────────────────────────────────────────────
                    (KeyCode::Backspace, _) => {
                        let mut st = state.lock().unwrap();
                        st.tab_completions.clear();
                        st.backspace();
                    }

                    // ── Delete forward ────────────────────────────────────────
                    (KeyCode::Delete, _) => {
                        state.lock().unwrap().delete_forward();
                    }

                    // ── Left / Right cursor movement ─────────────────────────
                    (KeyCode::Left, m) if m.contains(KeyModifiers::ALT) => {
                        state.lock().unwrap().move_word_left();
                    }
                    (KeyCode::Left, _) => {
                        state.lock().unwrap().move_left();
                    }
                    (KeyCode::Right, m) if m.contains(KeyModifiers::ALT) => {
                        state.lock().unwrap().move_word_right();
                    }
                    (KeyCode::Right, _) => {
                        state.lock().unwrap().move_right();
                    }
                    (KeyCode::Home, _) => {
                        // Go to start of current line in input
                        let input = state.lock().unwrap().input.clone();
                        let cursor = state.lock().unwrap().cursor_pos;
                        let line_start = input[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
                        state.lock().unwrap().cursor_pos = line_start;
                    }
                    (KeyCode::End, _) => {
                        let input = state.lock().unwrap().input.clone();
                        let cursor = state.lock().unwrap().cursor_pos;
                        let line_end = input[cursor..].find('\n').map(|i| cursor + i).unwrap_or(input.len());
                        state.lock().unwrap().cursor_pos = line_end;
                    }

                    // ── Readline shortcuts ────────────────────────────────────
                    (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                        state.lock().unwrap().cursor_pos = 0;
                    }
                    // Note: Ctrl+E is fork mode (see above). Use End key for end-of-line.
                    (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                        state.lock().unwrap().kill_word_back();
                    }
                    (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                        state.lock().unwrap().kill_line();
                    }
                    (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                        state.lock().unwrap().kill_to_end();
                    }

                    // ── Scroll chat (Up/Down) or input history ────────────────
                    (KeyCode::Up, _) => {
                        let input_empty = state.lock().unwrap().input.is_empty();
                        if input_empty {
                            state.lock().unwrap().history_up();
                        } else {
                            state.lock().unwrap().scroll_chat_up(3);
                        }
                    }
                    (KeyCode::Down, _) => {
                        let at_history = state.lock().unwrap().history_idx.is_some();
                        if at_history {
                            state.lock().unwrap().history_down();
                        } else {
                            state.lock().unwrap().scroll_chat_down(3);
                        }
                    }

                    // ── Ctrl+Up/Down — scroll chat by half page ───────────────
                    (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                        state.lock().unwrap().scroll_chat_up(10);
                    }
                    (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                        let has_confirm = state.lock().unwrap().pending_confirm.is_some();
                        if !has_confirm {
                            state.lock().unwrap().scroll_chat_down(10);
                        }
                    }

                    // ── PageUp/Down — scroll event log ────────────────────────
                    (KeyCode::PageUp, _) => {
                        state.lock().unwrap().scroll_event_up(5);
                    }
                    (KeyCode::PageDown, _) => {
                        state.lock().unwrap().scroll_event_down(5);
                    }

                    // ── F1 — help ─────────────────────────────────────────────
                    (KeyCode::F(1), _) => {
                        show_help(&state);
                    }

                    // ── Regular char input ────────────────────────────────────
                    (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) => {
                        state.lock().unwrap().tab_completions.clear();
                        state.lock().unwrap().insert_char(c);
                    }

                    _ => {}
                }
            }
        }
    }

    Ok(())
}

fn handle_char(state: &Arc<Mutex<AppState>>, c: char) {
    state.lock().unwrap().insert_char(c);
}

fn handle_mouse(state: &Arc<Mutex<AppState>>, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            state.lock().unwrap().scroll_chat_up(3);
        }
        MouseEventKind::ScrollDown => {
            state.lock().unwrap().scroll_chat_down(3);
        }
        _ => {}
    }
}

fn handle_voice(state: &Arc<Mutex<AppState>>) {
    let busy = state.lock().unwrap().busy;
    let recording = state.lock().unwrap().recording_voice;
    if busy || recording {
        state.lock().unwrap().push_event("[voice] busy, please wait.");
        return;
    }
    {
        let mut st = state.lock().unwrap();
        st.recording_voice = true;
        st.status = "Recording… (5s) Ctrl+S to cancel".to_string();
        st.push_event("[voice] recording 5s…");
    }
    let state2 = state.clone();
    let openai_key = std::env::var("OPENAI_API_KEY").ok();
    tokio::spawn(async move {
        use harness_voice::{WhisperBackend, record_and_transcribe};
        let backend = WhisperBackend::detect(openai_key.as_deref());
        let result = record_and_transcribe(Duration::from_secs(5), &backend).await;
        let mut st = state2.lock().unwrap();
        st.recording_voice = false;
        match result {
            Ok(t) if !t.is_empty() => {
                st.input.push_str(&t);
                st.cursor_pos = st.input.len();
                st.status = "Transcribed — press Enter to send.".to_string();
                st.push_event(format!("[voice] {}", &t[..t.len().min(80)]));
            }
            Ok(_) => { st.status = "Voice: no speech detected.".to_string(); }
            Err(e) => {
                st.push_event(format!("[voice] error: {e}"));
                st.status = format!("Voice error: {e}");
            }
        }
    });
}

fn approve_confirm(state: &Arc<Mutex<AppState>>, pc: PendingConfirm) {
    let _ = pc.reply.send(true);
    let mut st = state.lock().unwrap();
    let first_arg = pc.preview.lines().next().unwrap_or("").to_string();
    let key = (pc.tool_name.clone(), first_arg.clone());
    let count = st.approval_counts.entry(key).or_insert(0);
    *count += 1;
    if *count == 3 {
        st.push_event(format!(
            "[trust] Approved 3x. Run: harness trust {} \"{}\"",
            pc.tool_name, first_arg
        ));
    }
    st.push_event(format!("[plan] approved: {}", pc.tool_name));
    st.status = "Approved — continuing…".to_string();
}

fn handle_search_key(state: &Arc<Mutex<AppState>>, key: crossterm::event::KeyEvent) -> bool {
    let code = key.code;
    let mods = key.modifiers;
    match (code, mods) {
        (KeyCode::Esc, _) | (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
            let mut st = state.lock().unwrap();
            st.search_mode = false;
            st.search_query.clear();
            st.search_matches.clear();
            st.status = "Ready".to_string();
            true
        }
        (KeyCode::Enter, _) => {
            let mut st = state.lock().unwrap();
            let nmatches = st.search_matches.len();
            if nmatches > 0 {
                st.search_match_pos = (st.search_match_pos + 1) % nmatches;
                let msg_idx = st.search_matches[st.search_match_pos];
                st.chat_scroll.select(Some(msg_idx));
                st.status = format!("Search: \"{}\" ({}/{})", st.search_query, st.search_match_pos + 1, nmatches);
            }
            true
        }
        (KeyCode::Char('n'), KeyModifiers::NONE) => {
            let mut st = state.lock().unwrap();
            let nmatches = st.search_matches.len();
            if nmatches > 0 {
                st.search_match_pos = (st.search_match_pos + 1) % nmatches;
                let msg_idx = st.search_matches[st.search_match_pos];
                st.chat_scroll.select(Some(msg_idx));
                st.status = format!("Search: \"{}\" ({}/{})", st.search_query, st.search_match_pos + 1, nmatches);
            }
            true
        }
        (KeyCode::Char('p'), KeyModifiers::NONE) => {
            let mut st = state.lock().unwrap();
            let nmatches = st.search_matches.len();
            if nmatches > 0 {
                st.search_match_pos = (st.search_match_pos + nmatches - 1) % nmatches;
                let msg_idx = st.search_matches[st.search_match_pos];
                st.chat_scroll.select(Some(msg_idx));
                st.status = format!("Search: \"{}\" ({}/{})", st.search_query, st.search_match_pos + 1, nmatches);
            }
            true
        }
        (KeyCode::Backspace, _) => {
            let mut st = state.lock().unwrap();
            st.search_query.pop();
            run_search(&mut st);
            true
        }
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
            let mut st = state.lock().unwrap();
            st.search_query.push(c);
            run_search(&mut st);
            true
        }
        _ => false,
    }
}

fn run_search(st: &mut AppState) {
    let q = st.search_query.to_lowercase();
    st.search_matches = st.chat.iter().enumerate()
        .filter(|(_, m)| m.content.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect();
    st.search_match_pos = 0;
    let nmatches = st.search_matches.len();
    if let Some(&first) = st.search_matches.first() {
        st.chat_scroll.select(Some(first));
    }
    if q.is_empty() {
        st.status = "Search: ".to_string();
    } else {
        st.status = format!("Search: \"{}\" — {nmatches} match{}", q, if nmatches == 1 { "" } else { "es" });
    }
}

fn show_help(state: &Arc<Mutex<AppState>>) {
    let mut st = state.lock().unwrap();
    for line in &[
        "━━━ HARNESS COMMANDS ━━━━━━━━━━━━━━━━━━━━━━━━━━━",
        "SLASH COMMANDS",
        " /clear            clear chat panel",
        " /undo             restore last checkpoint",
        " /diff             show git diff",
        " /test             run test suite",
        " /compact          compact context",
        " /cost             show cost/token breakdown",
        " /plan             toggle plan mode",
        " /model <name>     switch model (e.g. claude-haiku-4-5)",
        " /think [N]        thinking budget (off = adaptive)",
        " /remember t: f    store fact under topic t",
        " /forget t         delete memory topic",
        " /memories         list all topics",
        " /sessions         browse previous sessions",
        " /pr [N]           list PRs / load PR #N context",
        " /issues           list GitHub issues",
        " /ci               show CI runs",
        " /focus [N]        silence notifications N minutes",
        " /ts               toggle timestamps",
        " /notify test      test desktop notification",
        " /trace [last]     show last turn trace",
        " /obsidian save    save response to Obsidian",
        " /help  F1         this list",
        "KEYBINDINGS",
        " Enter             send message",
        " Shift+Enter       new line in input",
        " ↑/↓              scroll chat (or input history when empty)",
        " PgUp/PgDn         scroll event log",
        " Ctrl+S            voice record (5s, Whisper transcription)",
        " Ctrl+F            search chat",
        " Ctrl+Y            copy last response to clipboard",
        " Ctrl+E            fork session at past turn",
        " Ctrl+[ / Ctrl+]   resize panels",
        " Ctrl+L            scroll to bottom",
        " Ctrl+A/E          line start/end",
        " Ctrl+W            kill word back",
        " Ctrl+U            kill line",
        " Ctrl+K            kill to end",
        " Alt+Left/Right    word jump",
        " Tab               @file autocomplete / slash autocomplete",
        " y / n / a         plan overlay: approve / deny / always-allow",
        " Ctrl+C            quit",
    ] {
        st.push_event(line.to_string());
    }
    st.status = "Help listed in event log →".to_string();
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
            st.chat.clear(); st.event_log.clear(); st.streaming.clear();
            st.status = "Chat cleared.".to_string();
        }

        "/undo" => {
            let mut st = state.lock().unwrap();
            match crate::checkpoint::undo() {
                Ok(msg) => { st.push_event(format!("[undo] {msg}")); st.status = "Undo complete.".to_string(); }
                Err(e) => { st.push_event(format!("[undo] {e}")); st.status = format!("Undo failed: {e}"); }
            }
        }

        "/diff" => {
            state.lock().unwrap().push_event("[diff] running git diff…");
            match tokio::process::Command::new("git").args(["diff", "--stat", "HEAD"]).output().await {
                Ok(out) => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let mut st = state.lock().unwrap();
                    for line in text.lines().take(40) { st.push_event(format!("  {line}")); }
                    if text.trim().is_empty() { st.push_event("  (no changes)"); }
                    st.status = "git diff in event log.".to_string();
                }
                Err(e) => { state.lock().unwrap().push_event(format!("[diff] {e}")); }
            }
        }

        "/test" => {
            let busy = state.lock().unwrap().busy;
            if busy { state.lock().unwrap().push_event("[test] agent running."); return; }
            let test_cmd = detect_test_command();
            { let mut st = state.lock().unwrap(); st.busy = true; st.status = format!("Running: {test_cmd}…"); st.push_event(format!("[test] {test_cmd}")); }
            let atx = agent_tx.clone();
            let state2 = state.clone();
            let cmd_str = test_cmd.clone();
            tokio::spawn(async move {
                let out = tokio::process::Command::new("sh").arg("-c").arg(&cmd_str).output().await;
                let mut st = state2.lock().unwrap();
                st.busy = false;
                match out {
                    Ok(o) => {
                        let all = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                        for line in all.lines().take(60) { st.push_event(format!("  {line}")); }
                        let status = if o.status.success() { "passed ✓" } else { "FAILED ✗" };
                        st.status = format!("Tests {status}.");
                        let _ = atx.send(AgentEvent::ToolResult { name: "test".into(), id: "test".into(), result: all });
                    }
                    Err(e) => { st.push_event(format!("[test] {e}")); st.status = format!("Test error: {e}"); }
                }
            });
        }

        "/cost" => {
            let st = state.lock().unwrap();
            let (in_tok, out_tok, model_name) = (st.tokens_in, st.tokens_out, st.model.clone());
            drop(st);
            let cost_line = match cost::price_for_model(&model_name) {
                Some(p) => format!("Cost: {} (↑{} ↓{} @ {})", cost::format_cost(p.cost_usd(in_tok, out_tok)), cost::format_tokens(in_tok), cost::format_tokens(out_tok), model_name),
                None => format!("Tokens: ↑{} ↓{} (no pricing for {model_name})", cost::format_tokens(in_tok), cost::format_tokens(out_tok)),
            };
            let mut st = state.lock().unwrap();
            st.push_event(cost_line.clone()); st.status = cost_line;
        }

        "/plan" => {
            let mut st = state.lock().unwrap();
            st.plan_mode = !st.plan_mode;
            if st.plan_mode { st.status = "Plan mode ON (restart with --plan to fully gate).".to_string(); }
            else { st.status = "Plan mode OFF.".to_string(); }
        }

        "/model" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                let model_name = state.lock().unwrap().model.clone();
                state.lock().unwrap().push_event(format!("[model] current: {model_name}"));
            } else {
                let mut st = state.lock().unwrap();
                st.model = name.to_string();
                st.push_event(format!("[model] → {name}"));
                st.status = format!("Model: {name}");
            }
        }

        "/runs" => {
            match crate::background::list(10) {
                Ok(runs) if runs.is_empty() => state.lock().unwrap().push_event("[runs] No background runs. Use `harness run-bg <prompt>`."),
                Ok(runs) => {
                    let mut st = state.lock().unwrap();
                    st.push_event(format!("[runs] {} run(s):", runs.len()));
                    for run in &runs {
                        let p = if run.prompt.len() > 50 { format!("{}…", &run.prompt[..50]) } else { run.prompt.clone() };
                        st.push_event(format!("  {} [{}] {}", run.id, run.status, p));
                    }
                }
                Err(e) => state.lock().unwrap().push_event(format!("[runs] {e}")),
            }
        }

        "/sessions" => {
            match session_store.list(20) {
                Ok(sessions) if sessions.is_empty() => {
                    state.lock().unwrap().push_event("[sessions] No sessions yet.");
                }
                Ok(sessions) => {
                    let mut st = state.lock().unwrap();
                    st.push_event(format!("[sessions] {} session(s) — use `harness --resume <id>` to load:", sessions.len()));
                    for (id, name, updated) in &sessions {
                        let short = &id[..8.min(id.len())];
                        let n = name.as_deref().unwrap_or("(unnamed)");
                        st.push_event(format!("  {short}  {n}  {updated}"));
                    }
                    st.status = format!("{} sessions in event log →", sessions.len());
                }
                Err(e) => state.lock().unwrap().push_event(format!("[sessions] {e}")),
            }
        }

        "/compact" => {
            let busy = state.lock().unwrap().busy;
            if busy { state.lock().unwrap().push_event("[compact] agent running."); return; }
            state.lock().unwrap().push_event("[compact] compacting…");
            crate::agent::compact_context(provider, session).await;
            let remaining = session.messages.len();
            let mut st = state.lock().unwrap();
            st.push_event(format!("[compact] {remaining} messages remain."));
            st.status = format!("Compacted ({remaining} messages).");
        }

        "/fork" => {
            state.lock().unwrap().push_event("[fork] Use Ctrl+E to enter fork mode.");
        }

        "/ts" => {
            let mut st = state.lock().unwrap();
            st.timestamps_visible = !st.timestamps_visible;
            st.status = if st.timestamps_visible { "Timestamps ON".into() } else { "Timestamps OFF".into() };
        }

        "/think" => {
            let mut st = state.lock().unwrap();
            let arg = parts.get(1).copied().unwrap_or("").trim();
            if arg.is_empty() || arg == "off" {
                st.thinking_budget = None;
                st.push_event("[think] OFF — adaptive.");
                st.status = "Thinking: adaptive".to_string();
            } else if let Ok(b) = arg.parse::<u32>() {
                st.thinking_budget = Some(b);
                st.push_event(format!("[think] ON — budget: {b} tokens"));
                st.status = format!("Thinking: {b} tokens");
            } else {
                st.push_event("[think] Usage: /think [N | off]");
            }
        }

        "/focus" => {
            let mut st = state.lock().unwrap();
            let arg = parts.get(1).copied().unwrap_or("").trim();
            if arg == "off" {
                st.focus_until = None;
                st.status = "Focus mode OFF.".to_string();
            } else {
                let mins: u64 = arg.parse().unwrap_or(25);
                st.focus_until = Some(Instant::now() + Duration::from_secs(mins * 60));
                st.push_event(format!("[focus] {mins}min focus — notifications silenced."));
                st.status = format!("Focus: {mins}min");
            }
        }

        "/remember" => {
            let rest = parts[1..].join(" ");
            if let Some((topic, fact)) = rest.split_once(':') {
                match crate::memory_project::remember(topic.trim(), fact.trim()) {
                    Ok(path) => {
                        let mut st = state.lock().unwrap();
                        st.push_event(format!("[memory] saved → {}", path.display()));
                        st.status = format!("Remembered under '{}'", topic.trim());
                    }
                    Err(e) => state.lock().unwrap().push_event(format!("[memory] error: {e}")),
                }
            } else {
                state.lock().unwrap().push_event("[memory] Usage: /remember <topic>: <fact>");
            }
        }

        "/forget" => {
            let topic = parts.get(1).copied().unwrap_or("").trim();
            if topic.is_empty() { state.lock().unwrap().push_event("[memory] Usage: /forget <topic>"); }
            else {
                match crate::memory_project::forget(topic) {
                    Ok(true) => { let mut st = state.lock().unwrap(); st.push_event(format!("[memory] forgot '{topic}'")); st.status = format!("Forgot '{topic}'"); }
                    Ok(false) => state.lock().unwrap().push_event(format!("[memory] no memory for '{topic}'")),
                    Err(e) => state.lock().unwrap().push_event(format!("[memory] {e}")),
                }
            }
        }

        "/memories" => {
            let topics = crate::memory_project::list_topics();
            let mut st = state.lock().unwrap();
            if topics.is_empty() { st.push_event("[memory] no topics. Use /remember topic: fact"); }
            else { st.push_event(format!("[memory] {} topic(s):", topics.len())); for t in &topics { st.push_event(format!("  • {t}")); } }
            st.status = format!("{} topics", topics.len());
        }

        "/pr" => {
            let pr_num = parts.get(1).copied().unwrap_or("").trim();
            if pr_num.is_empty() {
                state.lock().unwrap().push_event("[pr] fetching PRs…");
                let state2 = state.clone();
                tokio::spawn(async move {
                    let msg = harness_tools::tools::gh::pr_list().await.unwrap_or_else(|e| format!("gh error: {e}"));
                    let mut st = state2.lock().unwrap();
                    for line in msg.lines().take(30) { st.push_event(format!("  {line}")); }
                    st.status = "PRs in event log →".to_string();
                });
            } else {
                let num = pr_num.to_string();
                let mut st = state.lock().unwrap();
                st.input = format!("Review PR #{num} — fetch diff, comments, and CI status. Summarize and suggest improvements.");
                st.cursor_pos = st.input.len();
                st.status = format!("PR #{num} loaded — press Enter to review");
            }
        }

        "/issues" => {
            state.lock().unwrap().push_event("[issues] fetching…");
            let state2 = state.clone();
            tokio::spawn(async move {
                let out = tokio::process::Command::new("gh").args(["issue", "list", "--limit", "20"]).output().await;
                let msg = match out { Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(), Err(e) => format!("gh error: {e}") };
                let mut st = state2.lock().unwrap();
                for line in msg.lines().take(40) { st.push_event(format!("  {line}")); }
                st.status = "Issues in event log →".to_string();
            });
        }

        "/ci" => {
            state.lock().unwrap().push_event("[ci] checking runs…");
            let state2 = state.clone();
            tokio::spawn(async move {
                let out = tokio::process::Command::new("gh").args(["run", "list", "--limit", "10"]).output().await;
                let msg = match out { Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(), Err(e) => format!("gh error: {e}") };
                let mut st = state2.lock().unwrap();
                for line in msg.lines().take(20) { st.push_event(format!("  {line}")); }
                st.status = "CI runs in event log →".to_string();
            });
        }

        "/notify" | "/notify test" => {
            let notif_cfg = state.lock().unwrap().notifications.clone();
            crate::notifications::test_notification(&notif_cfg);
            state.lock().unwrap().push_event("[notify] test notification sent");
        }

        "/obsidian" => {
            state.lock().unwrap().push_event("[obsidian] bridge coming in Phase E12.");
        }

        "/trace" => {
            state.lock().unwrap().push_event("[trace] observability coming in Phase E7.");
        }

        "/schema" => {
            // Usage: /schema <name> <json-schema>
            //        /schema clear
            let rest = cmd.trim_start_matches("/schema").trim();
            if rest == "clear" || rest.is_empty() {
                state.lock().unwrap().response_schema = None;
                state.lock().unwrap().push_event("[schema] structured output cleared.");
            } else {
                // Split into name + json
                let mut schema_parts = rest.splitn(2, ' ');
                let name = schema_parts.next().unwrap_or("response");
                let schema_str = schema_parts.next().unwrap_or("{}");
                match serde_json::from_str::<serde_json::Value>(schema_str) {
                    Ok(schema_val) => {
                        let rs = harness_provider_core::ResponseSchema::new(name, schema_val);
                        let msg = format!("[schema] set to '{}' — responses will be strict JSON.", rs.name);
                        state.lock().unwrap().response_schema = Some(rs);
                        state.lock().unwrap().push_event(msg);
                    }
                    Err(e) => {
                        state.lock().unwrap().push_event(format!("[schema] invalid JSON: {e}"));
                    }
                }
            }
        }

        "/help" | "/?" => {
            show_help(state);
        }

        _ => {
            state.lock().unwrap().push_event(format!("[unknown] {cmd} — type /help or press F1"));
        }
    }

    let _ = (session, provider, session_store, done_tx, tools, memory_store, embed_model, system_prompt, model);
}

fn detect_test_command() -> String {
    if std::path::Path::new("Cargo.toml").exists() { "cargo test 2>&1".into() }
    else if std::path::Path::new("package.json").exists() { "npm test 2>&1".into() }
    else if std::path::Path::new("pyproject.toml").exists() || std::path::Path::new("setup.py").exists() { "python -m pytest 2>&1".into() }
    else if std::path::Path::new("go.mod").exists() { "go test ./... 2>&1".into() }
    else { "make test 2>&1".into() }
}

// ── Apply incoming agent events ────────────────────────────────────────────────

fn apply_agent_event(state: &Arc<Mutex<AppState>>, event: AgentEvent) {
    let mut st = state.lock().unwrap();
    match event {
        AgentEvent::TextChunk(chunk) => {
            st.streaming.push_str(&chunk);
        }
        AgentEvent::ToolStart { name, .. } => {
            st.tool_start = Some(Instant::now());
            st.push_event(format!("→ {name}"));
        }
        AgentEvent::ToolResult { name, result, .. } => {
            let preview = result.lines().next().unwrap_or("").chars().take(100).collect::<String>();
            let full_entry = format!("← {name}: {preview}");
            st.push_event(full_entry);
            // Store full result for expansion (keyed as last event)
            st.expanded_event_text = result;
        }
        AgentEvent::MemoryRecall { count } => {
            st.push_event(format!("memory: recalled {count} entries"));
        }
        AgentEvent::SubAgentSpawned { task } => {
            let p: String = task.chars().take(60).collect();
            st.push_event(format!("swarm ↓ {p}…"));
        }
        AgentEvent::SubAgentDone { task, .. } => {
            let p: String = task.chars().take(60).collect();
            st.push_event(format!("swarm ↑ done: {p}"));
            // Notify (if not in focus mode and if enabled)
            if !st.focus_active() {
                let notif_cfg = st.notifications.clone();
                crate::notifications::background_done(&notif_cfg, &p, true);
            }
        }
        AgentEvent::TokenUsage { input, output } => {
            st.tokens_in += input as u64;
            st.tokens_out += output as u64;

            if let Some(ref db) = st.cost_db {
                let usd = cost::price_for_model(&st.model)
                    .map(|p| p.cost_with_cache(input as u64, st.cache_read_tokens, output as u64))
                    .unwrap_or(0.0);
                let project = std::env::current_dir()
                    .ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_default();
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
                let row = cost_db::UsageRow {
                    session_id: st.session_id_full.clone(),
                    project,
                    provider: "auto".to_string(),
                    model: st.model.clone(),
                    ts,
                    in_tok: input,
                    cached_in: st.cache_read_tokens as u32,
                    out_tok: output,
                    native_calls: 0,
                    usd,
                };
                let _ = db.record(&row);

                let (daily_pct, monthly_pct) = cost_db::check_budget(db, st.budget_daily_usd, st.budget_monthly_usd);
                for (opt_pct, period) in [(&daily_pct, "daily"), (&monthly_pct, "monthly")] {
                    if let Some(pct) = opt_pct {
                        if *pct >= 80.0 {
                            let msg = format!("⚠ BUDGET: {:.0}% of {period} limit", pct);
                            if !st.focus_active() {
                                crate::notifications::budget_alert(&st.notifications, &msg);
                            }
                            st.push_event(msg);
                        }
                    }
                }
            }
            // Update right-side status
            st.status_right = st.cost_str();
        }
        AgentEvent::CacheUsage { creation, read } => {
            st.cache_creation_tokens += creation as u64;
            st.cache_read_tokens += read as u64;
            if read > 0 {
                let total = st.tokens_in.max(1);
                let pct = st.cache_read_tokens * 100 / total;
                st.push_event(format!("cache write={creation} read={read} ({pct}% hit)"));
            }
        }
        AgentEvent::Done => {
            if !st.streaming.is_empty() {
                let text = std::mem::take(&mut st.streaming);
                st.chat.push(ChatMessage { role: "assistant".into(), content: text, ts: Instant::now() });
            }
            st.scroll_to_bottom();
        }
        AgentEvent::Error(msg) => {
            st.push_event(format!("⚠ error: {msg}"));
            st.chat.push(ChatMessage { role: "error".into(), content: msg.clone(), ts: Instant::now() });
            st.status = format!("Error: {}", msg.chars().take(60).collect::<String>());
        }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn draw_all(f: &mut ratatui::Frame, state: &mut AppState, hl: &Highlighter, theme: &Theme) {
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
        .constraints([Constraint::Percentage(left_pct), Constraint::Percentage(right_pct)])
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
    state.chat.iter().map(|m| {
        1 + m.content.lines().count() + 1
    }).sum::<usize>() + if !state.streaming.is_empty() { 1 + state.streaming.lines().count() } else { 0 }
}

fn draw_chat(f: &mut ratatui::Frame, state: &mut AppState, area: Rect, hl: &Highlighter, theme: &Theme) {
    let mut items: Vec<ListItem> = Vec::new();
    let search_q = if state.search_mode { state.search_query.to_lowercase() } else { String::new() };

    for (msg_idx, msg) in state.chat.iter().enumerate() {
        let is_search_match = !search_q.is_empty() && state.search_matches.contains(&msg_idx);
        let (color, label) = match msg.role.as_str() {
            "user" => (theme.user_color, "you"),
            "assistant" => (theme.assistant_color, theme.assistant_label(&state.model)),
            _ => (theme.error_color, "err"),
        };
        let ts_str = if state.timestamps_visible {
            let elapsed = msg.ts.elapsed();
            let secs = state.session_start.elapsed().as_secs().saturating_sub(elapsed.as_secs());
            format!(" +{secs}s")
        } else { String::new() };
        let header_style = if is_search_match {
            Style::default().fg(theme.search_hl_color).add_modifier(Modifier::BOLD)
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
            Style::default().fg(theme.streaming_color).add_modifier(Modifier::BOLD),
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
        let elapsed = state.tool_start.map(|t| format!(" {:.0}s", t.elapsed().as_secs_f32())).unwrap_or_default();
        format!(" Chat {}  {} ", state.spinner_char(), elapsed)
    } else {
        format!(" Chat [{} turns] ", state.chat.iter().filter(|m| m.role == "user").count())
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(theme.border_color)))
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
    let items: Vec<ListItem> = state.event_log.iter().map(|line| {
        let color = if line.starts_with('→') { theme.tool_in_color }
            else if line.starts_with('←') { theme.tool_out_color }
            else if line.starts_with('⚠') || line.starts_with("error") { theme.error_color }
            else if line.starts_with("memory") || line.starts_with("cache") { theme.dim_color }
            else if line.starts_with("swarm") { Color::LightCyan }
            else { theme.border_color };
        ListItem::new(Line::from(Span::styled(line.as_str(), Style::default().fg(color))))
    }).collect();

    let title = " Events ";
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(theme.border_color)))
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
        let cur = state.tab_completions.get(state.tab_completion_idx).map(|s| s.as_str()).unwrap_or("");
        format!(" Message  [Tab→{cur}] ")
    } else if state.search_mode {
        format!(" Search: {} ", state.search_query)
    } else if let Some(idx) = state.history_idx {
        format!(" History [{}/{}] — ↑↓ to navigate, Enter to send ", idx + 1, state.input_history.len())
    } else {
        " Message  [Enter send · Shift+Enter newline · /help · @file Tab] ".to_string()
    };

    let input_widget = Paragraph::new(input_with_cursor)
        .block(Block::default().borders(Borders::ALL).title(title).border_style(
            if state.busy {
                Style::default().fg(theme.dim_color)
            } else {
                Style::default().fg(theme.border_color)
            }
        ))
        .style(Style::default().fg(if state.busy { Color::DarkGray } else { Color::White }))
        .wrap(Wrap { trim: false });
    f.render_widget(input_widget, area);
}

fn draw_status(f: &mut ratatui::Frame, state: &AppState, area: Rect, theme: &Theme) {
    let style = if state.computer_use_active {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if state.pending_confirm.is_some() {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if state.busy {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(theme.dim_color)
    };

    let mut indicators = String::new();
    if state.computer_use_active { indicators.push_str("[⚠CU] "); }
    if state.plan_mode { indicators.push_str("[PLAN] "); }
    if state.recording_voice { indicators.push_str("[🎙REC] "); }
    if state.focus_active() { indicators.push_str(&format!("[FOCUS {}m] ", state.focus_mins_remaining())); }
    if state.search_mode { indicators.push_str("[SEARCH] "); }

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
    if suggestions.is_empty() { return; }

    let height = (suggestions.len() as u16).min(8) + 2;
    let width = suggestions.iter().map(|s| s.len()).max().unwrap_or(20).min(60) as u16 + 4;

    let x = input_area.x + 2;
    let y = input_area.y.saturating_sub(height);
    let popup = Rect::new(x, y, width.min(input_area.width.saturating_sub(4)), height);

    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = suggestions.iter().enumerate().map(|(i, s)| {
        let style = if i == state.slash_suggest_idx {
            Style::default().fg(Color::Black).bg(theme.accent_color)
        } else {
            Style::default().fg(Color::White)
        };
        ListItem::new(Line::from(Span::styled(format!(" {s} "), style)))
    }).collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Commands ").border_style(Style::default().fg(theme.accent_color)));
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
    let match_info = if nmatches > 0 { format!(" [{}/{nmatches}]", state.search_match_pos + 1) } else { String::new() };
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
        Line::from(Span::styled(" Welcome to Harness — April 2026", Style::default().fg(theme.accent_color).add_modifier(Modifier::BOLD))),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Your AI coding assistant for 16-hour days.", Style::default().fg(Color::White))),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Try these first prompts:", Style::default().fg(Color::Gray))),
        Line::from(Span::styled("   Read README.md and summarize this project.", Style::default().fg(Color::Yellow))),
        Line::from(Span::styled("   Run the tests and show me which are failing.", Style::default().fg(Color::Yellow))),
        Line::from(Span::styled("   Refactor src/main.rs to be cleaner.", Style::default().fg(Color::Yellow))),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Keybindings:", Style::default().fg(Color::Gray))),
        Line::from(vec![
            Span::styled("   Enter", Style::default().fg(theme.accent_color).add_modifier(Modifier::BOLD)),
            Span::raw(" send  "),
            Span::styled("Shift+Enter", Style::default().fg(theme.accent_color).add_modifier(Modifier::BOLD)),
            Span::raw(" newline  "),
            Span::styled("↑↓", Style::default().fg(theme.accent_color).add_modifier(Modifier::BOLD)),
            Span::raw(" scroll/history"),
        ]),
        Line::from(vec![
            Span::styled("   Ctrl+F", Style::default().fg(theme.accent_color).add_modifier(Modifier::BOLD)),
            Span::raw(" search  "),
            Span::styled("Ctrl+Y", Style::default().fg(theme.accent_color).add_modifier(Modifier::BOLD)),
            Span::raw(" copy  "),
            Span::styled("Ctrl+S", Style::default().fg(theme.accent_color).add_modifier(Modifier::BOLD)),
            Span::raw(" voice  "),
            Span::styled("F1", Style::default().fg(theme.accent_color).add_modifier(Modifier::BOLD)),
            Span::raw(" help"),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Type /help or press F1 for all commands.", Style::default().fg(Color::Gray))),
        Line::from(Span::styled(" Use @filename to pin files · Tab to autocomplete.", Style::default().fg(Color::Gray))),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Press Enter to get started", Style::default().fg(theme.accent_color).add_modifier(Modifier::BOLD))),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent_color))
        .title(Span::styled(" harness — first run ", Style::default().fg(theme.accent_color).add_modifier(Modifier::BOLD)));

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
    let preview_lines: Vec<Line> = pc.preview.lines().map(|l| {
        let color = if l.starts_with("+ ") { Color::Green }
            else if l.starts_with("- ") { Color::Red }
            else if l.starts_with("$ ") { Color::Yellow }
            else { Color::White };
        Line::from(Span::styled(format!(" {l}"), Style::default().fg(color)))
    }).collect();

    let mut content: Vec<Line> = preview_lines;
    content.push(Line::from(Span::raw("")));
    content.push(Line::from(vec![
        Span::styled(" y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" approve   "),
        Span::styled("n", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw(" deny   "),
        Span::styled("a", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" always allow   "),
        Span::styled("Esc", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw(" skip   "),
        Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" approve"),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));

    let para = Paragraph::new(content).block(block).wrap(Wrap { trim: false });
    f.render_widget(para, popup_area);
}

// ── Fork session helpers ──────────────────────────────────────────────────────

fn count_user_turns(messages: &[harness_provider_core::Message]) -> usize {
    messages.iter().filter(|m| matches!(m.role, harness_provider_core::Role::User)).count()
}

fn fork_session_at(original: &harness_memory::Session, turn_n: usize) -> harness_memory::Session {
    use harness_provider_core::Role;
    let mut new_session = harness_memory::Session::new(&original.model);
    if let Some(name) = &original.name {
        new_session.name = Some(format!("{name} (fork@{turn_n})"));
    }
    let mut user_count = 0;
    for msg in &original.messages {
        if matches!(msg.role, Role::User) { user_count += 1; }
        new_session.messages.push(msg.clone());
        if user_count >= turn_n { break; }
    }
    new_session
}
