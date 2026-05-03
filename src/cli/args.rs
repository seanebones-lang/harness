//! CLI argument definitions (clap).

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "harness",
    about = "Harness — multi-provider AI coding agent (Claude · GPT · Grok · Qwen)",
    long_about = "Harness is a Rust-native AI coding agent supporting Anthropic Claude 4.x, OpenAI GPT-5.x, xAI Grok 4.x, and Ollama Qwen3-Coder. Set ANTHROPIC_API_KEY, OPENAI_API_KEY, or XAI_API_KEY and run `harness` to start.",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Prompt to run in non-interactive mode.
    pub prompt: Option<String>,

    /// Resume a session by id prefix or name.
    #[arg(long, short)]
    pub resume: Option<String>,

    /// Config file path (default: ~/.harness/config.toml or .harness/config.toml).
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Model override (e.g. grok-4.3, grok-4.1-fast-reasoning, claude-opus-4-7).
    #[arg(long, short)]
    pub model: Option<String>,

    /// Disable semantic memory recall for this run.
    #[arg(long)]
    pub no_memory: bool,

    /// Enable browser tool (requires Chrome with --remote-debugging-port=9222).
    #[arg(long)]
    pub browser: bool,

    /// Chrome DevTools remote URL (default: http://localhost:9222).
    #[arg(long, default_value = "http://localhost:9222")]
    pub browser_url: String,

    /// Verbose logging.
    #[arg(long, short)]
    pub verbose: bool,

    /// Plan mode: preview file writes, patches, and shell commands before they execute.
    /// In TUI, press Enter to approve or Esc to skip each change.
    #[arg(long)]
    pub plan: bool,

    /// Attach an image file to the initial prompt (PNG, JPEG, GIF, WEBP).
    #[arg(long)]
    pub image: Option<PathBuf>,

    /// Enable extended thinking with a token budget.
    /// Example: --think 10000. Use without value for adaptive thinking (Opus 4.7 only).
    #[arg(long, value_name = "BUDGET")]
    pub think: Option<u32>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List recent sessions.
    Sessions,
    /// Manage linked projects in a local registry.
    #[command(visible_alias = "proj")]
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// Run a single prompt non-interactively.
    Run { prompt: String },
    /// Start the harness HTTP server.
    Serve {
        /// Address to listen on.
        #[arg(long, default_value = "127.0.0.1:8787")]
        addr: String,
    },
    /// Connect to a running harness server and chat via SSE.
    Connect {
        /// Server base URL.
        #[arg(default_value = "http://127.0.0.1:8787")]
        url: String,
        /// Prompt to send.
        prompt: String,
        /// Existing session id to continue.
        #[arg(long)]
        session: Option<String>,
    },
    /// Run harness in self-development mode: the agent can edit its own source
    /// and trigger rebuilds via the rebuild_self and reload_self tools.
    SelfDev {
        /// Directory containing harness source (defaults to current dir).
        #[arg(long)]
        src: Option<PathBuf>,
        /// Model for self-dev (default: same as main session model, e.g. claude-sonnet-4-6).
        #[arg(long)]
        model: Option<String>,
    },
    /// Export a session as Markdown.
    Export {
        /// Session id prefix or name.
        id: String,
        /// Output file path (defaults to stdout).
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// Delete a session by id prefix or full id.
    Delete {
        /// Session id prefix or full id.
        id: String,
    },
    /// Start the harness daemon (long-lived process over ~/.harness/daemon.sock).
    /// The daemon holds provider clients, SQLite, LSP servers, and ambient memory.
    /// Other harness processes auto-connect to the daemon when it's running.
    Daemon,
    /// Check if the harness daemon is running and print its status.
    DaemonStatus,
    /// Run a prompt as a background agent (detached process).
    /// Output is streamed to ~/.harness/runs/<id>/output.log.
    RunBg {
        /// Prompt to run in the background.
        prompt: String,
    },
    /// List recent background runs.
    Runs,
    /// Add a tool auto-approval rule (skip confirmation for matching calls).
    /// Example: harness trust shell "cargo check"
    Trust {
        /// Tool name (e.g. shell, write_file, git, *).
        tool: String,
        /// Pattern to match in the first argument (use * for all).
        pattern: String,
    },
    /// Remove a previously added trust rule.
    Untrust { tool: String, pattern: String },
    /// List all trust rules.
    TrustList,
    /// Set up harness for the first time (writes ~/.harness/config.toml).
    /// Pass --project to also write a project-level .harness/config.toml in CWD.
    Init {
        /// Also create a project-local .harness/config.toml in the current directory.
        #[arg(long)]
        project: bool,
        /// Overwrite existing config files without prompting.
        #[arg(long)]
        force: bool,
    },
    /// Show harness configuration and environment status.
    Status,
    /// Restore the most recent harness checkpoint stash (undo last agent turn).
    Undo,
    /// Manage harness checkpoint stashes.
    Checkpoint {
        #[command(subcommand)]
        action: CheckpointAction,
    },
    /// List available providers and models, with an interactive picker to change defaults.
    Models {
        /// Set the default model (writes to .harness/config.toml). Format: "provider:model" or just "model".
        #[arg(long)]
        set: Option<String>,
    },
    /// Sync Harness state across machines via an encrypted git repository.
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    /// Show cost and usage statistics from the cost database.
    Cost {
        #[command(subcommand)]
        action: CostAction,
    },
    /// Open a PR review session pre-loaded with PR context (diff, comments, CI status).
    /// Requires gh CLI to be installed and authenticated.
    Pr {
        /// PR number.
        number: u64,
        /// Post a review comment on the PR and exit (does not open an agent session).
        #[arg(long)]
        comment: Option<String>,
    },
    /// Store a project memory fact in .harness/memory/<topic>.md.
    /// These are automatically injected into the system prompt each session.
    Memorize {
        /// Topic name (used as filename, e.g. "architecture").
        topic: String,
        /// Fact to remember.
        fact: String,
    },
    /// Remove a project memory topic.
    Forget {
        /// Topic to remove.
        topic: String,
    },
    /// List all project memory topics.
    Memories,
    /// Record audio and transcribe via Whisper.
    /// Requires sox (brew install sox) for recording.
    Voice {
        /// Duration to record in seconds (default: 5).
        #[arg(long, short, default_value = "5")]
        duration: u64,
        /// Send transcript as a prompt to the agent instead of just printing it.
        #[arg(long)]
        send: bool,
        /// Use OpenAI Realtime API for duplex voice conversation (requires OPENAI_API_KEY).
        #[arg(long)]
        realtime: bool,
    },
    /// Manage parallel sub-agent swarm tasks.
    Swarm {
        #[command(subcommand)]
        action: SwarmAction,
    },
    /// Export observability traces.
    Trace {
        /// Trace ID to export (omit for last trace).
        id: Option<String>,
    },
    /// Run health checks: API keys, tools, config, daemon, MCP, LSP, and more.
    Doctor,
    /// Generate shell completions (bash, zsh, fish, powershell, elvish).
    Completions {
        /// Shell type.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
pub enum CheckpointAction {
    /// List all harness checkpoint stashes.
    List,
}

#[derive(Subcommand)]
pub enum SyncAction {
    /// Initialise sync with a remote git repository.
    Init {
        /// Git remote URL (e.g. git@github.com:user/harness-state.git).
        git_url: String,
    },
    /// Encrypt and push state to the remote.
    Push,
    /// Pull and decrypt state from the remote.
    Pull,
    /// Show sync status.
    Status,
    /// Show/set the sync passphrase.
    Auth,
}

#[derive(Subcommand)]
pub enum SwarmAction {
    /// Run one or more agent tasks in the background (tracked in swarm.db).
    Run {
        /// Task prompt.
        prompt: String,
        /// Override model for swarm workers (defaults to the main session model).
        #[arg(long)]
        model: Option<String>,
        /// Number of parallel tasks (default 1).
        #[arg(long)]
        count: Option<usize>,
    },
    /// List recent swarm tasks.
    List,
    /// Show status of a specific task.
    Status {
        /// Task ID.
        id: String,
    },
    /// Show result of a completed task.
    Result {
        /// Task ID.
        id: String,
    },
}

#[derive(Subcommand)]
pub enum CostAction {
    /// Show cost for today.
    Today,
    /// Show cost for the past 7 days.
    Week,
    /// Show cost for the past 30 days.
    Month,
    /// Show all-time cost.
    All,
    /// Show cost broken down by model.
    ByModel,
    /// Show cost broken down by project.
    ByProject,
    /// Tail recent usage rows live.
    Watch,
}

#[derive(Subcommand)]
pub enum ProjectAction {
    /// Create a new local git project and link it.
    #[command(visible_alias = "new")]
    Init {
        /// Project name.
        name: String,
        /// Parent folder to create the project in (defaults to current directory).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Initial branch name (default: main).
        #[arg(long = "default-branch", default_value = "main")]
        default_branch: String,
    },
    /// Add a project to ~/.harness/projects.json.
    #[command(visible_alias = "link")]
    Add {
        /// Optional project nickname (defaults to folder name).
        #[arg(long)]
        name: Option<String>,
        /// Project path (defaults to current directory).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Optional git remote URL override.
        #[arg(long)]
        remote: Option<String>,
        /// Optional default branch override.
        #[arg(long = "default-branch")]
        default_branch: Option<String>,
    },
    /// Clone a repo and link it in the project registry.
    #[command(visible_alias = "cl")]
    Clone {
        /// Repository URL/path to clone.
        repo: String,
        /// Optional project nickname (defaults to cloned folder name).
        #[arg(long)]
        name: Option<String>,
        /// Optional clone directory (defaults to repo-derived folder name).
        #[arg(long)]
        directory: Option<PathBuf>,
        /// Optional default branch to store in registry.
        #[arg(long = "default-branch")]
        default_branch: Option<String>,
    },
    /// List all linked projects.
    #[command(visible_alias = "ls")]
    List,
    /// Show a one-screen health summary for all linked projects.
    #[command(visible_alias = "dash")]
    Dashboard,
    /// Remove a linked project by name or path.
    #[command(visible_alias = "rm")]
    Remove {
        /// Project name (from `project list`) or absolute path.
        target: String,
    },
    /// Fetch + fast-forward pull for a linked project.
    #[command(visible_alias = "up")]
    Sync {
        /// Project name (from `project list`) or absolute path.
        target: Option<String>,
        /// Sync every linked project.
        #[arg(long, conflicts_with = "target")]
        all: bool,
    },
    /// Push the current branch for a linked project.
    #[command(visible_alias = "pub")]
    Push {
        /// Project name (from `project list`) or absolute path.
        target: String,
        /// Optional remote name (default: origin).
        #[arg(long, default_value = "origin")]
        remote: String,
        /// Optional branch override (defaults to current branch).
        #[arg(long)]
        branch: Option<String>,
        /// Force push with lease (blocked for main/master).
        #[arg(long)]
        force: bool,
    },
    /// Show git health for a linked project.
    #[command(visible_alias = "st")]
    Status {
        /// Project name (from `project list`) or absolute path.
        target: String,
    },
    /// Import local git repos into the linked project registry.
    #[command(visible_alias = "scan")]
    Import {
        /// Root folder to scan (defaults to current directory).
        #[arg(long)]
        root: Option<PathBuf>,
        /// Recursively scan nested folders.
        #[arg(long)]
        recursive: bool,
    },
    /// Remove linked projects whose paths no longer exist.
    #[command(visible_alias = "clean")]
    Prune,
    /// Run a command inside a linked project directory.
    #[command(visible_alias = "run")]
    Exec {
        /// Project name (from `project list`) or absolute path.
        target: String,
        /// Command to run (use `--` before command).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true, num_args = 1..)]
        command: Vec<String>,
    },
    /// Publish a linked project to GitHub using gh CLI.
    #[command(visible_alias = "ship")]
    Publish {
        /// Project name (from `project list`) or absolute path.
        target: String,
        /// GitHub repo name (owner/name or name). Defaults to project name.
        #[arg(long)]
        repo: Option<String>,
        /// Remote name to configure (default: origin).
        #[arg(long, default_value = "origin")]
        remote: String,
        /// Create as public repository.
        #[arg(long, conflicts_with = "private")]
        public: bool,
        /// Create as private repository (default).
        #[arg(long, default_value_t = true)]
        private: bool,
        /// Push current branch after creating the remote.
        #[arg(long, default_value_t = true)]
        push: bool,
    },
    /// Resolve and print a linked project path.
    Open {
        /// Project name (from `project list`) or absolute path.
        target: String,
        /// Launch harness in the project directory after resolving it.
        #[arg(long)]
        run: bool,
    },
}
