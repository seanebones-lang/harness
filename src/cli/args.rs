//! CLI argument definitions (clap).

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "harness",
    about = "NextEleven Harness — multi-provider AI coding agent (Claude · GPT · Grok · Qwen)",
    long_about = "NextEleven Harness is a Rust-native AI coding agent by NextEleven LLC. Supports Anthropic Claude 4.x, OpenAI GPT-5.x, xAI Grok 4.x, Gemini, Bedrock, Mistral, and Ollama Qwen3-Coder. Set ANTHROPIC_API_KEY, OPENAI_API_KEY, or XAI_API_KEY and run `harness` to start.",
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

    /// Model override (e.g. grok-4.3, grok-4.1-fast, claude-opus-4-7).
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
        #[arg(long, default_value = "http://127.0.0.1:8787")]
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
    /// Output is streamed to `~/.harness/runs/<id>/output.log`.
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
    /// Interactive provider and API key setup (same flow as the first-run wizard).
    Setup {
        /// Re-run setup even when keys are already configured.
        #[arg(long)]
        force: bool,
    },
    /// Print instructions to upgrade to the latest release.
    Update,
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
    /// Store a project memory fact in `.harness/memory/<topic>.md`.
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
    /// Write to Obsidian, Apple Notes, Calendar, or list GitHub Project items.
    Bridge {
        #[command(subcommand)]
        action: BridgeAction,
    },
    /// List MCP resources/roots or read a resource URI (from `.harness/mcp.json`).
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Export observability traces.
    Trace {
        /// Trace ID to export (omit for last trace).
        id: Option<String>,
    },
    /// Run health checks: API keys, tools, config, daemon, MCP, LSP, and more.
    Doctor,
    /// Run offline micro-benchmark pack (no API keys). See `demo/bench_tasks/`.
    Bench {
        /// Pack directory containing pack.json (default: demo/bench_tasks if present).
        #[arg(long)]
        pack: Option<PathBuf>,
        /// Emit JSON report.
        #[arg(long)]
        json: bool,
    },
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
        #[arg(long, visible_alias = "agents", short = 'n')]
        count: Option<usize>,
    },
    /// List recent swarm tasks.
    List,
    /// Show status of a specific task.
    Status {
        /// Task ID.
        id: String,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show result of a completed task.
    Result {
        /// Task ID.
        id: String,
        /// Emit machine-readable JSON (id, status, prompt, result, timestamps).
        #[arg(long)]
        json: bool,
    },
    /// Cancel a pending or running task (or all with `--all`).
    Cancel {
        /// Task ID (prefix ok). Required unless `--all`.
        id: Option<String>,
        /// Cancel every pending/running task.
        #[arg(long)]
        all: bool,
    },
    /// Wait until a task completes (or timeout).
    Wait {
        /// Task ID (prefix ok).
        id: String,
        /// Max seconds to wait (default 300).
        #[arg(long, default_value = "300")]
        timeout_secs: u64,
    },
    /// Reap orphan pending/running tasks and optionally purge old terminal rows.
    Gc {
        /// Mark non-live pending/running older than this many seconds as failed (default 3600).
        #[arg(long, default_value = "3600")]
        stale_secs: u64,
        /// Keep only the newest N terminal tasks (done/failed/cancelled); delete the rest.
        #[arg(long)]
        keep: Option<usize>,
        /// Delete terminal tasks completed more than this many seconds ago.
        #[arg(long)]
        older_than_secs: Option<u64>,
        /// Report what would change without writing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub enum BridgeAction {
    /// Write a note to Obsidian (`[bridges.obsidian]` must be enabled).
    Obsidian {
        title: String,
        /// Note body (or `-` to read from stdin).
        content: String,
    },
    /// Create a note in Apple Notes.
    Notes { title: String, content: String },
    /// List calendar events for a date (`YYYY-MM-DD`).
    CalendarList { date: String },
    /// Create a calendar event (`start`/`end` as AppleScript date strings).
    CalendarCreate {
        title: String,
        start: String,
        end: String,
    },
    /// List GitHub Project V2 items.
    GithubProject,
}

#[derive(Subcommand)]
pub enum McpAction {
    /// List resources across configured MCP servers.
    Resources {
        /// Only query this server name (from mcp.json).
        #[arg(long)]
        server: Option<String>,
    },
    /// Show workspace roots harness advertises to MCP servers.
    Roots,
    /// Read a resource by URI (tries servers that advertise resources).
    Read {
        /// Resource URI (e.g. `file:///…` or server-specific scheme).
        uri: String,
        /// Only query this server name (from mcp.json).
        #[arg(long)]
        server: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).unwrap_or_else(|e| panic!("parse should succeed: {e}"))
    }

    fn parse_err(args: &[&str]) -> clap::error::Error {
        match Cli::try_parse_from(args) {
            Ok(_) => panic!("parse should fail for {args:?}"),
            Err(e) => e,
        }
    }

    #[test]
    fn top_level_defaults_and_global_flags() {
        let cli = parse(&["harness"]);
        assert!(cli.command.is_none());
        assert!(cli.prompt.is_none());
        assert!(!cli.no_memory);
        assert!(!cli.browser);
        assert!(!cli.plan);
        assert!(!cli.verbose);
        assert_eq!(cli.browser_url, "http://localhost:9222");
        assert!(cli.think.is_none());
        assert!(cli.model.is_none());
        assert!(cli.resume.is_none());
        assert!(cli.image.is_none());

        let cli = parse(&[
            "harness",
            "--no-memory",
            "--browser",
            "--plan",
            "-v",
            "--model",
            "grok-4.5",
            "--think",
            "10000",
            "--browser-url",
            "http://127.0.0.1:9333",
            "--resume",
            "abc",
            "do the thing",
        ]);
        assert!(cli.no_memory);
        assert!(cli.browser);
        assert!(cli.plan);
        assert!(cli.verbose);
        assert_eq!(cli.model.as_deref(), Some("grok-4.5"));
        assert_eq!(cli.think, Some(10_000));
        assert_eq!(cli.browser_url, "http://127.0.0.1:9333");
        assert_eq!(cli.resume.as_deref(), Some("abc"));
        assert_eq!(cli.prompt.as_deref(), Some("do the thing"));
    }

    #[test]
    fn parse_run_serve_export_delete_and_init() {
        let cli = parse(&["harness", "run", "hello world"]);
        match cli.command {
            Some(Commands::Run { prompt }) => assert_eq!(prompt, "hello world"),
            _ => panic!("expected Run"),
        }

        let cli = parse(&["harness", "serve", "--addr", "0.0.0.0:9000"]);
        match cli.command {
            Some(Commands::Serve { addr }) => assert_eq!(addr, "0.0.0.0:9000"),
            _ => panic!("expected Serve"),
        }
        // default addr
        let cli = parse(&["harness", "serve"]);
        match cli.command {
            Some(Commands::Serve { addr }) => assert_eq!(addr, "127.0.0.1:8787"),
            _ => panic!("expected Serve default"),
        }

        let cli = parse(&["harness", "export", "sess01", "-o", "out.md"]);
        match cli.command {
            Some(Commands::Export { id, output }) => {
                assert_eq!(id, "sess01");
                assert_eq!(output.as_deref(), Some(std::path::Path::new("out.md")));
            }
            _ => panic!("expected Export"),
        }

        let cli = parse(&["harness", "delete", "deadbeef"]);
        match cli.command {
            Some(Commands::Delete { id }) => assert_eq!(id, "deadbeef"),
            _ => panic!("expected Delete"),
        }

        let cli = parse(&["harness", "init", "--project", "--force"]);
        match cli.command {
            Some(Commands::Init { project, force }) => {
                assert!(project);
                assert!(force);
            }
            _ => panic!("expected Init"),
        }
    }

    #[test]
    fn parse_swarm_run_aliases_status_json_cancel_all_gc() {
        // -n and visible_alias --agents
        let cli = parse(&[
            "harness",
            "swarm",
            "run",
            "task-a",
            "-n",
            "3",
            "--model",
            "ollama:qwen",
        ]);
        match cli.command {
            Some(Commands::Swarm {
                action:
                    SwarmAction::Run {
                        prompt,
                        model,
                        count,
                    },
            }) => {
                assert_eq!(prompt, "task-a");
                assert_eq!(model.as_deref(), Some("ollama:qwen"));
                assert_eq!(count, Some(3));
            }
            _ => panic!("expected swarm run -n"),
        }

        let cli = parse(&["harness", "swarm", "run", "task-b", "--agents", "2"]);
        match cli.command {
            Some(Commands::Swarm {
                action: SwarmAction::Run { count, .. },
            }) => assert_eq!(count, Some(2)),
            _ => panic!("expected swarm run --agents"),
        }

        // defaults: count None when omitted
        let cli = parse(&["harness", "swarm", "run", "solo"]);
        match cli.command {
            Some(Commands::Swarm {
                action:
                    SwarmAction::Run {
                        prompt,
                        model,
                        count,
                    },
            }) => {
                assert_eq!(prompt, "solo");
                assert!(model.is_none());
                assert!(count.is_none());
            }
            _ => panic!("expected swarm run defaults"),
        }

        let cli = parse(&["harness", "swarm", "status", "tid", "--json"]);
        match cli.command {
            Some(Commands::Swarm {
                action: SwarmAction::Status { id, json },
            }) => {
                assert_eq!(id, "tid");
                assert!(json);
            }
            _ => panic!("expected swarm status --json"),
        }

        let cli = parse(&["harness", "swarm", "result", "tid2", "--json"]);
        match cli.command {
            Some(Commands::Swarm {
                action: SwarmAction::Result { id, json },
            }) => {
                assert_eq!(id, "tid2");
                assert!(json);
            }
            _ => panic!("expected swarm result --json"),
        }

        let cli = parse(&["harness", "swarm", "cancel", "--all"]);
        match cli.command {
            Some(Commands::Swarm {
                action: SwarmAction::Cancel { id, all },
            }) => {
                assert!(id.is_none());
                assert!(all);
            }
            _ => panic!("expected cancel --all"),
        }

        let cli = parse(&["harness", "swarm", "cancel", "abc123"]);
        match cli.command {
            Some(Commands::Swarm {
                action: SwarmAction::Cancel { id, all },
            }) => {
                assert_eq!(id.as_deref(), Some("abc123"));
                assert!(!all);
            }
            _ => panic!("expected cancel id"),
        }

        let cli = parse(&["harness", "swarm", "wait", "w1"]);
        match cli.command {
            Some(Commands::Swarm {
                action: SwarmAction::Wait { id, timeout_secs },
            }) => {
                assert_eq!(id, "w1");
                assert_eq!(timeout_secs, 300);
            }
            _ => panic!("expected wait default timeout"),
        }

        let cli = parse(&[
            "harness",
            "swarm",
            "gc",
            "--stale-secs",
            "60",
            "--keep",
            "5",
            "--older-than-secs",
            "86400",
            "--dry-run",
        ]);
        match cli.command {
            Some(Commands::Swarm {
                action:
                    SwarmAction::Gc {
                        stale_secs,
                        keep,
                        older_than_secs,
                        dry_run,
                    },
            }) => {
                assert_eq!(stale_secs, 60);
                assert_eq!(keep, Some(5));
                assert_eq!(older_than_secs, Some(86_400));
                assert!(dry_run);
            }
            _ => panic!("expected gc flags"),
        }

        // list has no flags
        let cli = parse(&["harness", "swarm", "list"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Swarm {
                action: SwarmAction::List
            })
        ));
    }

    #[test]
    fn parse_mcp_bridge_cost_project_aliases_and_bench() {
        let cli = parse(&["harness", "mcp", "resources", "--server", "fs"]);
        match cli.command {
            Some(Commands::Mcp {
                action: McpAction::Resources { server },
            }) => assert_eq!(server.as_deref(), Some("fs")),
            _ => panic!("expected mcp resources"),
        }

        let cli = parse(&["harness", "mcp", "read", "file:///tmp/x", "--server", "fs"]);
        match cli.command {
            Some(Commands::Mcp {
                action: McpAction::Read { uri, server },
            }) => {
                assert_eq!(uri, "file:///tmp/x");
                assert_eq!(server.as_deref(), Some("fs"));
            }
            _ => panic!("expected mcp read"),
        }

        assert!(matches!(
            parse(&["harness", "mcp", "roots"]).command,
            Some(Commands::Mcp {
                action: McpAction::Roots
            })
        ));

        let cli = parse(&["harness", "bridge", "obsidian", "T", "body"]);
        match cli.command {
            Some(Commands::Bridge {
                action: BridgeAction::Obsidian { title, content },
            }) => {
                assert_eq!(title, "T");
                assert_eq!(content, "body");
            }
            _ => panic!("expected bridge obsidian"),
        }

        let cli = parse(&["harness", "cost", "by-model"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Cost {
                action: CostAction::ByModel
            })
        ));

        // project visible aliases
        let cli = parse(&["harness", "proj", "ls"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Project {
                action: ProjectAction::List
            })
        ));

        let cli = parse(&[
            "harness",
            "project",
            "new",
            "demo",
            "--default-branch",
            "develop",
        ]);
        match cli.command {
            Some(Commands::Project {
                action:
                    ProjectAction::Init {
                        name,
                        path,
                        default_branch,
                    },
            }) => {
                assert_eq!(name, "demo");
                assert!(path.is_none());
                assert_eq!(default_branch, "develop");
            }
            _ => panic!("expected project init alias new"),
        }

        let cli = parse(&["harness", "bench", "--json", "--pack", "demo/bench_tasks"]);
        match cli.command {
            Some(Commands::Bench { pack, json }) => {
                assert!(json);
                assert_eq!(
                    pack.as_deref(),
                    Some(std::path::Path::new("demo/bench_tasks"))
                );
            }
            _ => panic!("expected bench"),
        }

        let cli = parse(&["harness", "voice", "-d", "12"]);
        match cli.command {
            Some(Commands::Voice {
                duration,
                send,
                realtime,
            }) => {
                assert_eq!(duration, 12);
                assert!(!send);
                assert!(!realtime);
            }
            _ => panic!("expected voice"),
        }

        // default voice duration
        let cli = parse(&["harness", "voice"]);
        match cli.command {
            Some(Commands::Voice { duration, .. }) => assert_eq!(duration, 5),
            _ => panic!("expected voice default"),
        }

        let cli = parse(&["harness", "pr", "42", "--comment", "lgtm"]);
        match cli.command {
            Some(Commands::Pr { number, comment }) => {
                assert_eq!(number, 42);
                assert_eq!(comment.as_deref(), Some("lgtm"));
            }
            _ => panic!("expected pr"),
        }

        let cli = parse(&[
            "harness",
            "connect",
            "--url",
            "http://x:1",
            "hi",
            "--session",
            "s1",
        ]);
        match cli.command {
            Some(Commands::Connect {
                url,
                prompt,
                session,
            }) => {
                assert_eq!(url, "http://x:1");
                assert_eq!(prompt, "hi");
                assert_eq!(session.as_deref(), Some("s1"));
            }
            _ => panic!("expected connect"),
        }

        let cli = parse(&["harness", "connect", "hello"]);
        match cli.command {
            Some(Commands::Connect { url, prompt, .. }) => {
                assert_eq!(url, "http://127.0.0.1:8787");
                assert_eq!(prompt, "hello");
            }
            _ => panic!("expected connect default url"),
        }
    }

    #[test]
    fn parse_rejects_unknown_subcommand_and_missing_required() {
        // Bare unknown token becomes top-level `prompt` (not an error) — document that.
        let cli = parse(&["harness", "not-a-real-command"]);
        assert!(cli.command.is_none());
        assert_eq!(cli.prompt.as_deref(), Some("not-a-real-command"));

        // Force a subcommand context for a real unknown action:
        let err = parse_err(&["harness", "swarm", "not-a-swarm-action"]);
        let _ = err;

        let err = parse_err(&["harness", "swarm", "status"]); // missing id
        assert!(
            err.to_string().to_lowercase().contains("required")
                || err.kind() != clap::error::ErrorKind::DisplayHelp
        );

        let _ = parse_err(&["harness", "export"]); // missing id
        let _ = parse_err(&["harness", "mcp", "read"]); // missing uri

        // project sync --all conflicts with target
        let err = parse_err(&["harness", "project", "sync", "foo", "--all"]);
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("cannot be used with")
                || msg.contains("conflict")
                || err.kind() == clap::error::ErrorKind::ArgumentConflict,
            "expected conflict error, got: {err}"
        );
    }

    #[test]
    fn parse_checkpoint_sync_trust_trace_and_completions_shell() {
        assert!(matches!(
            parse(&["harness", "checkpoint", "list"]).command,
            Some(Commands::Checkpoint {
                action: CheckpointAction::List
            })
        ));

        let cli = parse(&["harness", "sync", "init", "git@example.com:r.git"]);
        match cli.command {
            Some(Commands::Sync {
                action: SyncAction::Init { git_url },
            }) => assert_eq!(git_url, "git@example.com:r.git"),
            _ => panic!("expected sync init"),
        }

        let cli = parse(&["harness", "trust", "shell", "cargo test"]);
        match cli.command {
            Some(Commands::Trust { tool, pattern }) => {
                assert_eq!(tool, "shell");
                assert_eq!(pattern, "cargo test");
            }
            _ => panic!("expected trust"),
        }

        let cli = parse(&["harness", "trace", "abc"]);
        match cli.command {
            Some(Commands::Trace { id }) => assert_eq!(id.as_deref(), Some("abc")),
            _ => panic!("expected trace id"),
        }
        assert!(matches!(
            parse(&["harness", "trace"]).command,
            Some(Commands::Trace { id: None })
        ));

        // Completions shell enum — just ensure parse succeeds for common shells.
        for shell in ["bash", "zsh", "fish"] {
            let cli = parse(&["harness", "completions", shell]);
            assert!(
                matches!(cli.command, Some(Commands::Completions { .. })),
                "completions {shell}"
            );
        }
    }

    #[test]
    fn parse_project_exec_trailing_and_publish_flags() {
        let cli = parse(&[
            "harness",
            "project",
            "exec",
            "demo",
            "--",
            "cargo",
            "test",
            "--",
            "--nocapture",
        ]);
        match cli.command {
            Some(Commands::Project {
                action: ProjectAction::Exec { target, command },
            }) => {
                assert_eq!(target, "demo");
                assert_eq!(command, vec!["cargo", "test", "--", "--nocapture"]);
            }
            _ => panic!("expected project exec"),
        }

        let cli = parse(&[
            "harness",
            "project",
            "publish",
            "demo",
            "--public",
            "--repo",
            "owner/demo",
        ]);
        match cli.command {
            Some(Commands::Project {
                action:
                    ProjectAction::Publish {
                        target,
                        repo,
                        remote,
                        public,
                        private,
                        push,
                    },
            }) => {
                assert_eq!(target, "demo");
                assert_eq!(repo.as_deref(), Some("owner/demo"));
                assert_eq!(remote, "origin");
                assert!(public);
                // --public conflicts with --private; clap still stores private default_t
                // but public flag is true when passed.
                let _ = private;
                assert!(push);
            }
            _ => panic!("expected publish"),
        }
    }
}
