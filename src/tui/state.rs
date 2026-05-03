//! Chat/event/input state for the ratatui TUI (`AppState`, `ChatMessage`, `PendingConfirm`).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::cost;
use crate::cost_db::CostDb;
use ratatui::widgets::ListState;

use super::theme::Theme;

// ── Constants ──────────────────────────────────────────────────────────────────

const HISTORY_MAX: usize = 1000;
const SPINNER_CHARS: &[char] = &['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
const DEFAULT_RIGHT_PCT: u8 = 38;

// ── App state ─────────────────────────────────────────────────────────────────

pub(crate) struct ChatMessage {
    pub(crate) role: String,
    pub(crate) content: String,
    pub(crate) ts: Instant,
}

#[allow(clippy::struct_excessive_bools)]
pub(crate) struct AppState {
    pub(crate) input: String,
    pub(crate) cursor_pos: usize,
    /// Finalized chat messages shown in the left panel.
    pub(crate) chat: Vec<ChatMessage>,
    /// Current streaming assistant text (rendered live at bottom of chat).
    pub(crate) streaming: String,
    /// Event log shown in the right panel.
    pub(crate) event_log: Vec<String>,
    /// Status bar text (transient, ephemeral message).
    pub(crate) status: String,
    /// Persistent info shown on the right of the status bar.
    pub(crate) status_right: String,
    /// Is the agent currently running?
    pub(crate) busy: bool,
    /// Scroll state for chat list.
    pub(crate) chat_scroll: ListState,
    /// Scroll state for event log.
    pub(crate) event_scroll: ListState,
    /// Total rendered chat item count (updated on draw for scroll bounds).
    pub(crate) chat_items_len: usize,
    /// Total event log item count.
    pub(crate) event_items_len: usize,
    /// Session id for display.
    pub(crate) session_id: String,
    /// Cumulative token counts for this session.
    pub(crate) tokens_in: u64,
    pub(crate) tokens_out: u64,
    /// Anthropic prompt-cache stats.
    pub(crate) cache_read_tokens: u64,
    pub(crate) cache_creation_tokens: u64,
    /// Active model name.
    pub(crate) model: String,
    /// Session start time.
    pub(crate) session_start: Instant,
    /// Plan mode: pending confirmation request.
    pub(crate) pending_confirm: Option<PendingConfirm>,
    /// First-run welcome overlay.
    pub(crate) show_welcome: bool,
    /// Plan mode toggle.
    pub(crate) plan_mode: bool,
    /// @file tab-completion candidates.
    pub(crate) tab_completions: Vec<String>,
    pub(crate) tab_completion_idx: usize,
    /// Fork mode.
    pub(crate) fork_mode: bool,
    /// Extended thinking budget.
    pub(crate) thinking_budget: Option<u32>,
    /// Is a voice recording in progress?
    pub(crate) recording_voice: bool,
    /// Is computer use active?
    pub(crate) computer_use_active: bool,
    /// Cost database handle.
    pub(crate) cost_db: Option<CostDb>,
    /// Full session ID.
    pub(crate) session_id_full: String,
    /// Budget limits.
    pub(crate) budget_daily_usd: Option<f64>,
    pub(crate) budget_monthly_usd: Option<f64>,
    /// Approval counts for trust suggestions.
    pub(crate) approval_counts: std::collections::HashMap<(String, String), usize>,
    /// Notifications config.
    pub(crate) notifications: crate::config::NotificationsConfig,
    // ── E1 new fields ─────────────────────────────────────────────────────────
    /// Input history (most recent at index 0).
    pub(crate) input_history: VecDeque<String>,
    /// Current history navigation position (None = live input).
    pub(crate) history_idx: Option<usize>,
    /// Saved live input when navigating history.
    pub(crate) history_saved: String,
    /// Search mode active.
    pub(crate) search_mode: bool,
    /// Current search query.
    pub(crate) search_query: String,
    /// Chat indices (into chat vec) that match current search.
    pub(crate) search_matches: Vec<usize>,
    /// Current match position (index into search_matches).
    pub(crate) search_match_pos: usize,
    /// Slash command autocomplete: matching commands.
    pub(crate) slash_suggestions: Vec<String>,
    /// Which suggestion is selected.
    pub(crate) slash_suggest_idx: usize,
    /// Right panel percentage (default 38).
    pub(crate) right_panel_pct: u8,
    /// Timestamps visible.
    pub(crate) timestamps_visible: bool,
    /// Spinner frame.
    pub(crate) spinner_frame: usize,
    /// Last spinner tick.
    pub(crate) spinner_tick: Instant,
    /// When the current tool started (for elapsed display).
    pub(crate) tool_start: Option<Instant>,
    /// Expanded event log item index (None = none expanded).
    pub(crate) _expanded_event: Option<usize>,
    /// Full text for expanded event.
    pub(crate) expanded_event_text: String,
    /// Focus mode: silence notifications until this time.
    pub(crate) focus_until: Option<Instant>,
    /// Theme.
    pub(crate) theme: Theme,
    /// Active response schema for strict JSON output (set via /schema).
    pub(crate) response_schema: Option<harness_provider_core::ResponseSchema>,
}

pub(crate) struct PendingConfirm {
    pub(crate) tool_name: String,
    pub(crate) preview: String,
    pub(crate) reply: tokio::sync::oneshot::Sender<bool>,
}

fn is_first_run() -> bool {
    let marker = dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/.welcomed");
    !marker.exists()
}

pub(crate) fn mark_welcomed() {
    if let Some(home) = dirs::home_dir() {
        let dir = home.join(".harness");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(".welcomed"), "1");
    }
}

impl AppState {
    pub(crate) fn new(model: &str) -> Self {
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

    pub(crate) fn cost_str(&self) -> String {
        if self.tokens_in == 0 && self.tokens_out == 0 {
            return String::new();
        }
        let in_str = cost::format_tokens(self.tokens_in);
        let out_str = cost::format_tokens(self.tokens_out);
        let cost_part = cost::price_for_model(&self.model)
            .map(|p| {
                let usd =
                    p.cost_with_cache(self.tokens_in, self.cache_read_tokens, self.tokens_out);
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

    pub(crate) fn elapsed_str(&self) -> String {
        let secs = self.session_start.elapsed().as_secs();
        if secs < 60 {
            format!("{secs}s")
        } else {
            format!("{}m{}s", secs / 60, secs % 60)
        }
    }

    pub(crate) fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
        self.update_slash_suggestions();
    }

    pub(crate) fn backspace(&mut self) {
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

    pub(crate) fn delete_forward(&mut self) {
        if self.cursor_pos < self.input.len() {
            let next = self.input[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.input.len());
            self.input.drain(self.cursor_pos..next);
        }
    }

    pub(crate) fn move_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.input[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub(crate) fn move_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos += self.input[self.cursor_pos..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
        }
    }

    pub(crate) fn move_word_left(&mut self) {
        let s = &self.input[..self.cursor_pos];
        let trimmed = s.trim_end();
        let new_pos = trimmed.rfind([' ', '/', '.']).map(|i| i + 1).unwrap_or(0);
        self.cursor_pos = new_pos;
    }

    pub(crate) fn move_word_right(&mut self) {
        let s = &self.input[self.cursor_pos..];
        let trimmed = s.trim_start();
        let skip = s.len() - trimmed.len();
        let word_end = trimmed
            .find([' ', '/', '.'])
            .map(|i| i + skip + 1)
            .unwrap_or(s.len());
        self.cursor_pos += word_end;
    }

    pub(crate) fn kill_word_back(&mut self) {
        let original = self.cursor_pos;
        self.move_word_left();
        let new_pos = self.cursor_pos;
        self.input.drain(new_pos..original);
        self.cursor_pos = new_pos;
    }

    pub(crate) fn kill_to_end(&mut self) {
        self.input.truncate(self.cursor_pos);
    }

    pub(crate) fn kill_line(&mut self) {
        self.input.clear();
        self.cursor_pos = 0;
        self.update_slash_suggestions();
    }

    pub(crate) fn take_input(&mut self) -> String {
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

    pub(crate) fn history_up(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
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

    pub(crate) fn history_down(&mut self) {
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

    pub(crate) fn push_event(&mut self, msg: impl Into<String>) {
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

    pub(crate) fn scroll_chat_up(&mut self, n: usize) {
        let cur = self
            .chat_scroll
            .selected()
            .unwrap_or(self.chat_items_len.saturating_sub(1));
        let new = cur.saturating_sub(n);
        self.chat_scroll.select(Some(new));
    }

    pub(crate) fn scroll_chat_down(&mut self, n: usize) {
        let cur = self.chat_scroll.selected().unwrap_or(0);
        let max = self.chat_items_len.saturating_sub(1);
        let new = (cur + n).min(max);
        self.chat_scroll.select(Some(new));
    }

    pub(crate) fn scroll_event_up(&mut self, n: usize) {
        let cur = self
            .event_scroll
            .selected()
            .unwrap_or(self.event_items_len.saturating_sub(1));
        let new = cur.saturating_sub(n);
        self.event_scroll.select(Some(new));
    }

    pub(crate) fn scroll_event_down(&mut self, n: usize) {
        let cur = self.event_scroll.selected().unwrap_or(0);
        let max = self.event_items_len.saturating_sub(1);
        let new = (cur + n).min(max);
        self.event_scroll.select(Some(new));
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
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

    pub(crate) fn tick_spinner(&mut self) {
        if self.spinner_tick.elapsed() >= Duration::from_millis(120) {
            self.spinner_frame = (self.spinner_frame + 1) % SPINNER_CHARS.len();
            self.spinner_tick = Instant::now();
        }
    }

    pub(crate) fn spinner_char(&self) -> char {
        SPINNER_CHARS[self.spinner_frame]
    }

    pub(crate) fn focus_active(&self) -> bool {
        self.focus_until
            .map(|t| Instant::now() < t)
            .unwrap_or(false)
    }

    pub(crate) fn focus_mins_remaining(&self) -> u64 {
        self.focus_until
            .map(|t| t.saturating_duration_since(Instant::now()).as_secs() / 60 + 1)
            .unwrap_or(0)
    }
}
