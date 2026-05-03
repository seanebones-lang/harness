//! CLI plumbing extracted from `main.rs` (zero behavior change).

pub mod args;
pub mod wiring;

pub use args::{
    CheckpointAction, Cli, Commands, CostAction, ProjectAction, SwarmAction, SyncAction,
};
pub use wiring::{build_tools, connect_to_server, graceful_ambient_shutdown};
