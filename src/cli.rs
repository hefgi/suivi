use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "suivi",
    about = "Track time spent working with AI coding agents",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Record a turn start (called by agent hooks)
    Hook(HookArgs),
    /// Initialize suivi: create config and install agent hooks
    Init,
    /// Show hook health and recent untracked activity
    Status,
    /// Show time analytics
    Stats(StatsArgs),
    /// Database maintenance
    Doctor(DoctorArgs),
    /// Start tracking a project directory and backfill historical attribution
    Track(TrackArgs),
    /// Remove a project from tracking (history is preserved)
    Untrack(UntrackArgs),
    /// Remove suivi hooks from all agent configs
    Uninstall(UninstallArgs),
}

#[derive(Args)]
pub struct TrackArgs {
    /// Directory to track (absolute, relative, or ~-prefixed)
    #[arg(value_name = "PATH")]
    pub path: String,

    /// Friendly name shown in stats (defaults to directory basename)
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Skip historical backfill of unattributed turns under this path
    #[arg(long)]
    pub no_backfill: bool,
}

#[derive(Args)]
pub struct UntrackArgs {
    /// Path or name of the tracked project to remove
    #[arg(value_name = "PATH_OR_NAME")]
    pub target: String,

    /// Skip the (y/N) confirmation prompt
    #[arg(long)]
    pub yes: bool,
}

#[derive(Args)]
pub struct HookArgs {
    #[command(subcommand)]
    pub event: HookEvent,
}

#[derive(Subcommand)]
pub enum HookEvent {
    /// Record turn start
    Pre(HookEventArgs),
    /// Record turn end
    Stop(HookEventArgs),
}

#[derive(Args)]
pub struct HookEventArgs {
    /// Agent identity (e.g. "claude-code"), set by the installed hook command.
    /// Falls back to payload/environment detection when absent.
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Args)]
pub struct StatsArgs {
    /// Show stats for all time
    #[arg(long)]
    pub all: bool,

    /// Show stats for today only
    #[arg(long)]
    pub today: bool,

    /// Show stats for the last 7 rolling days
    #[arg(long)]
    pub week: bool,

    /// Show stats for the last 30 rolling days
    #[arg(long)]
    pub month: bool,

    /// Show stats from a specific date (YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    pub since: Option<String>,

    /// Show stats up to (and including) this date (YYYY-MM-DD). Requires --since.
    #[arg(
        long,
        value_name = "DATE",
        requires = "since",
        conflicts_with_all = ["today", "week", "month", "all"],
    )]
    pub until: Option<String>,

    /// Filter by project name or path
    #[arg(long)]
    pub project: Option<String>,

    /// Filter by agent name
    #[arg(long)]
    pub agent: Option<String>,

    /// Show per-project breakdown
    #[arg(long)]
    pub projects: bool,

    /// Show turn history
    #[arg(long)]
    pub history: bool,

    /// Show ASCII activity graph
    #[arg(long)]
    pub graph: bool,

    /// Show daily breakdown
    #[arg(long)]
    pub daily: bool,

    /// Output format (applies to default stats summary and --history; --graph and --daily are text-only)
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[derive(Args)]
pub struct UninstallArgs {
    /// Also delete suivi's config, database, and logs
    #[arg(long)]
    pub purge: bool,
}

#[derive(Args)]
pub struct DoctorArgs {
    /// Delete stale turns and turns beyond retention period
    #[arg(long)]
    pub prune: bool,

    /// Run SQLite PRAGMA integrity_check
    #[arg(long)]
    pub check: bool,

    /// Clamp historical turns whose agent_duration_secs or wall window
    /// exceeds `tracking.max_turn_secs`. Without --yes, runs as a dry run.
    #[arg(long)]
    pub fix_outliers: bool,

    /// Recompute ended_at for historical Claude Code turns by scanning
    /// their JSONL transcripts. Reclaims phantom idle time recorded when
    /// sessions were suspended (laptop sleep, walk-away). Without --yes,
    /// runs as a dry run.
    #[arg(long)]
    pub fix_from_transcripts: bool,

    /// Delete historical turns whose cwd matches any user-configured exclude
    /// path or built-in default. Without --yes, runs as a dry run.
    #[arg(long)]
    pub prune_excluded: bool,

    /// Confirm destructive actions (--fix-outliers, --fix-from-transcripts,
    /// --prune-excluded).
    #[arg(long)]
    pub yes: bool,

    /// Print the last N lines from the suivi log (default 50 if no N given).
    /// Logs are only written when SUIVI_LOG is set.
    #[arg(long, value_name = "N", num_args = 0..=1, default_missing_value = "50")]
    pub logs: Option<usize>,
}

#[derive(ValueEnum, Clone, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Csv,
}
