//! Apply agent events to TUI state. Pure state transitions — no I/O beyond
//! the cost-DB write and notification dispatch that were already inline.
//!
//! Extracted from `tui/mod.rs` (May 2026) as part of the god-file decomposition.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;

use crate::cost;
use crate::cost_db;
use crate::events::AgentEvent;

use super::{AppState, ChatMessage};

pub(crate) fn apply_agent_event(state: &Arc<Mutex<AppState>>, event: AgentEvent) {
    let mut st = state.lock();
    match event {
        AgentEvent::TextChunk(chunk) => {
            st.streaming.push_str(&chunk);
        }
        AgentEvent::ToolStart { name, .. } => {
            st.tool_start = Some(Instant::now());
            st.push_event(format!("→ {name}"));
        }
        AgentEvent::ToolResult { name, result, .. } => {
            let preview = result
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(100)
                .collect::<String>();
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
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                    .unwrap_or_default();
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
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

                let (daily_pct, monthly_pct) =
                    cost_db::check_budget(db, st.budget_daily_usd, st.budget_monthly_usd);
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
                st.chat.push(ChatMessage {
                    role: "assistant".into(),
                    content: text,
                    ts: Instant::now(),
                });
            }
            st.scroll_to_bottom();
        }
        AgentEvent::Error(msg) => {
            st.push_event(format!("⚠ error: {msg}"));
            st.chat.push(ChatMessage {
                role: "error".into(),
                content: msg.clone(),
                ts: Instant::now(),
            });
            st.status = format!("Error: {}", msg.chars().take(60).collect::<String>());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::AgentEvent;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn arc_state() -> Arc<Mutex<AppState>> {
        Arc::new(Mutex::new(AppState::new("test-model")))
    }

    #[test]
    fn text_chunk_appends_to_streaming_buffer() {
        let st = arc_state();
        apply_agent_event(&st, AgentEvent::TextChunk("hel".into()));
        apply_agent_event(&st, AgentEvent::TextChunk("lo".into()));
        assert_eq!(st.lock().streaming, "hello");
    }

    #[test]
    fn done_finalizes_streaming_into_chat_message() {
        let st = arc_state();
        apply_agent_event(&st, AgentEvent::TextChunk("answer".into()));
        apply_agent_event(&st, AgentEvent::Done);
        let s = st.lock();
        assert!(s.streaming.is_empty(), "streaming buffer must drain");
        assert_eq!(s.chat.len(), 1);
        assert_eq!(s.chat[0].role, "assistant");
        assert_eq!(s.chat[0].content, "answer");
    }

    #[test]
    fn done_with_empty_streaming_does_not_append_blank_message() {
        let st = arc_state();
        apply_agent_event(&st, AgentEvent::Done);
        assert!(st.lock().chat.is_empty());
    }

    #[test]
    fn tool_start_records_event_and_timestamp() {
        let st = arc_state();
        apply_agent_event(
            &st,
            AgentEvent::ToolStart {
                name: "shell".into(),
                id: "call-1".into(),
            },
        );
        let s = st.lock();
        assert_eq!(s.event_log.len(), 1);
        assert_eq!(s.event_log[0], "→ shell");
        assert!(s.tool_start.is_some());
    }

    #[test]
    fn tool_result_truncates_preview_and_stores_full_text() {
        let st = arc_state();
        let long = "x".repeat(500);
        apply_agent_event(
            &st,
            AgentEvent::ToolResult {
                name: "read_file".into(),
                id: "call-2".into(),
                result: long.clone(),
            },
        );
        let s = st.lock();
        assert_eq!(s.event_log.len(), 1);
        // Preview is the first 100 chars; verify our event log line is bounded.
        let line = &s.event_log[0];
        assert!(line.starts_with("← read_file: "));
        assert!(
            line.len() < 200,
            "preview must be truncated, got {} chars",
            line.len()
        );
        assert_eq!(s.expanded_event_text, long);
    }

    #[test]
    fn token_usage_accumulates_counts() {
        let st = arc_state();
        apply_agent_event(
            &st,
            AgentEvent::TokenUsage {
                input: 100,
                output: 50,
            },
        );
        apply_agent_event(
            &st,
            AgentEvent::TokenUsage {
                input: 25,
                output: 10,
            },
        );
        let s = st.lock();
        assert_eq!(s.tokens_in, 125);
        assert_eq!(s.tokens_out, 60);
    }

    #[test]
    fn cache_usage_accumulates_and_emits_event_only_on_read() {
        let st = arc_state();
        apply_agent_event(
            &st,
            AgentEvent::CacheUsage {
                creation: 100,
                read: 0,
            },
        );
        // No event when read is zero (only cache writes — boring).
        assert_eq!(st.lock().event_log.len(), 0);

        apply_agent_event(
            &st,
            AgentEvent::CacheUsage {
                creation: 0,
                read: 200,
            },
        );
        let s = st.lock();
        assert_eq!(s.cache_creation_tokens, 100);
        assert_eq!(s.cache_read_tokens, 200);
        assert_eq!(s.event_log.len(), 1);
        assert!(s.event_log[0].contains("cache write=0 read=200"));
    }

    #[test]
    fn error_event_appends_chat_error_and_status() {
        let st = arc_state();
        apply_agent_event(&st, AgentEvent::Error("provider exploded".into()));
        let s = st.lock();
        assert_eq!(s.chat.len(), 1);
        assert_eq!(s.chat[0].role, "error");
        assert_eq!(s.chat[0].content, "provider exploded");
        assert!(s.status.starts_with("Error: "));
        assert_eq!(s.event_log.len(), 1);
        assert!(s.event_log[0].starts_with("⚠ error:"));
    }

    #[test]
    fn memory_recall_emits_event_with_count() {
        let st = arc_state();
        apply_agent_event(&st, AgentEvent::MemoryRecall { count: 7 });
        let s = st.lock();
        assert_eq!(s.event_log.len(), 1);
        assert!(s.event_log[0].contains("recalled 7 entries"));
    }
}
