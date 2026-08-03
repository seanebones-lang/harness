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
pub use drive::{
    drive_agent, drive_agent_full, drive_agent_with_options, drive_agent_with_schema,
};
pub use memory::store_turn_memory;
pub use naming::suggest_session_name;
pub use run_once::{run_once, RunOnceOptions};
pub use system::{load_project_instructions, DEFAULT_SYSTEM};

#[cfg(test)]
mod tests;
