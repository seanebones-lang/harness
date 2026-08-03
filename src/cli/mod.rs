//! CLI plumbing extracted from `main.rs` (zero behavior change).

pub mod args;
pub mod commands;
pub mod lightweight;
pub mod wiring;

pub use args::{
    CheckpointAction, Cli, Commands, CostAction, McpAction, ProjectAction, SwarmAction, SyncAction,
};
pub use commands::{
    build_prompt_with_image, command_needs_agent_runtime, delete_session, export_session,
    handle_doctor_command, handle_models_command, handle_project_command, list_sessions,
    maybe_run_first_time_wizard, run_init, run_self_dev, run_setup_interactive, run_status,
    run_update,
};
pub use lightweight::dispatch_lightweight;
pub use wiring::{build_tools, connect_to_server, graceful_ambient_shutdown};
