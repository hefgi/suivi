use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "suivi", about = "Track time spent working with AI coding agents", version)]
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
}

#[derive(Args)]
pub struct HookArgs {
    #[command(subcommand)]
    pub event: HookEvent,
}

#[derive(Subcommand)]
pub enum HookEvent {
    /// Record turn start
    Pre,
    /// Record turn end
    Stop,
}

#[derive(Args)]
pub struct StatsArgs {
    /// Show stats for all time (default shows today + this week + all time)
    #[arg(long)]
    pub all: bool,

    /// Filter by project path
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

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[derive(Args)]
pub struct DoctorArgs {
    /// Delete stale turns and turns beyond retention period
    #[arg(long)]
    pub prune: bool,

    /// Run SQLite PRAGMA integrity_check
    #[arg(long)]
    pub check: bool,
}

#[derive(ValueEnum, Clone, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Csv,
}
