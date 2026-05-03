//! Per-subcommand handlers extracted from `main.rs` for readability and unit testing.
//!
//! Each submodule owns one top-level CLI command (or one CLI subcommand group) and
//! exposes a single entry point that takes the parsed `Action` enum from `cli::args`.

pub mod doctor;
pub mod init;
pub mod models;
pub mod project;
pub mod prompt;
pub mod self_dev;
pub mod sessions;
pub mod status;

pub use doctor::handle_doctor_command;
pub use init::run_init;
pub use models::handle_models_command;
pub use project::handle_project_command;
pub use prompt::build_prompt_with_image;
pub use self_dev::run_self_dev;
pub use sessions::{delete_session, export_session, list_sessions};
pub use status::run_status;
