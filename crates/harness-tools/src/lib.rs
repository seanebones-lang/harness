//! Built-in agent tools and the shared `Tool` trait / executor.
#![deny(missing_docs)]

pub mod confirm;
/// Tool execution loop with confirm gate and post-write hooks.
pub mod executor;
/// Shared destructive-tool policy (checkpoints + plan mode).
pub mod policy;
/// Tool trait and registry.
pub mod registry;
/// Built-in tool implementations.
pub mod tools;
/// Workspace path sandboxing for filesystem tools.
pub mod workspace_root;

pub use confirm::{ConfirmGate, ConfirmRequest, ConfirmResult};
pub use executor::ToolExecutor;
pub use policy::{tool_requires_checkpoint, tool_requires_confirmation, BUILTIN_DESTRUCTIVE_TOOLS};
pub use registry::ToolRegistry;
pub use workspace_root::{ArcWorkspace, SandboxMode, WorkspaceRoot};
