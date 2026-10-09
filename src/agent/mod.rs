//! Core agent loop: send → stream → execute tools → repeat.
//! Emits AgentEvents so callers (TUI or CLI) can display progress.

mod compact;
mod drive;
mod memory;
mod naming;
mod run_once;
mod system;

// Re-export public API (stable paths used across the binary).
#[allow(unused_imports)]
pub use compact::{compact_context, context_limit_for_model, estimate_tokens, maybe_compact};
#[allow(unused_imports)]
pub use drive::{drive_agent, drive_agent_full, drive_agent_with_options, drive_agent_with_schema};
pub use memory::store_turn_memory;
pub use naming::suggest_session_name;
pub use run_once::{run_once, RunOnceOptions};
#[allow(unused_imports)]
pub use system::{load_project_instructions, load_project_instructions_in, DEFAULT_SYSTEM};

#[cfg(test)]
mod tests;

/// Restore a resumable tool ledger after interruption without assuming effects were rolled back.
pub(crate) fn complete_cancelled_tool_results(session: &mut harness_memory::Session) {
    let completed: std::collections::HashSet<&str> = session
        .messages
        .iter()
        .filter_map(|message| message.tool_call_id.as_deref())
        .collect();
    let unfinished: Vec<String> = session
        .messages
        .iter()
        .filter(|message| message.role == harness_provider_core::Role::Assistant)
        .filter_map(|message| message.content.as_str().strip_prefix("__tool_calls__:"))
        .filter_map(|json| serde_json::from_str::<Vec<harness_provider_core::ToolCall>>(json).ok())
        .flatten()
        .filter(|call| !completed.contains(call.id.as_str()))
        .map(|call| call.id.clone())
        .collect();
    for id in unfinished {
        session.push(harness_provider_core::Message::tool_result(&id,
            "Cancelled before a result was recorded. Effects may be partial; inspect the workspace before retrying."));
    }
}
