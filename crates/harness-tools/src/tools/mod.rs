pub mod agent;
pub mod apply_patch;
pub mod computer;
/// SQLite database queries (read-only by default).
pub mod database;
/// Allowlisted Docker CLI (read-heavy; config-gated).
pub mod docker;
/// File read/write/list/patch tools.
pub mod filesystem;
pub mod gh;
pub mod git;
/// Jupyter `.ipynb` cell read/edit.
pub mod notebook;
/// Regex code search over the workspace.
pub mod search;
pub mod selfdev;
/// Shell command execution.
pub mod shell;
pub mod swarm_tool;
pub mod test_runner;

pub use agent::SpawnAgentTool;
pub use apply_patch::ApplyPatchTool;
pub use computer::ComputerUseTool;
pub use database::{DatabaseTool, DatabaseToolConfig};
pub use docker::{DockerTool, DockerToolConfig};
pub use filesystem::{ListDirTool, PatchFileTool, ReadFileTool, WriteFileTool};
pub use gh::GhTool;
pub use git::GitTool;
pub use notebook::NotebookTool;
pub use search::SearchCodeTool;
pub use selfdev::{RebuildSelfTool, ReloadSelfTool};
pub use shell::{ShellConfig, ShellTool};
pub use swarm_tool::{SpawnSwarmTool, SwarmEnqueueRunner};
pub use test_runner::TestRunnerTool;
