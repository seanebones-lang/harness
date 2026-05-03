//! CLI plumbing extracted from `main.rs` (zero behavior change).

pub mod args;
pub mod commands;
pub mod wiring;

pub use args::{
    CheckpointAction, Cli, Commands, CostAction, ProjectAction, SwarmAction, SyncAction,
};
pub use commands::{
    build_prompt_with_image, delete_session, export_session, handle_doctor_command,
    handle_models_command, handle_project_command, list_sessions, run_init, run_self_dev,
    run_status,
};
pub use wiring::{build_tools, connect_to_server, graceful_ambient_shutdown};
