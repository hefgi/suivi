mod agents;
mod analytics;
mod cli;
mod commands;
mod config;
mod db;
mod error;
mod hooks;
mod log;

use clap::Parser;
use cli::{Cli, Command, HookEvent};

fn main() {
    let cli = Cli::parse();
    let result: Result<(), String> = match cli.command {
        Command::Hook(args) => {
            match args.event {
                HookEvent::Pre => hooks::pre::handle_pre(),
                HookEvent::Stop => hooks::stop::handle_stop(),
            }
            Ok(())
        }
        Command::Init => commands::init::run().map_err(|e| e.to_string()),
        Command::Status => commands::status::run().map_err(|e| e.to_string()),
        Command::Doctor(args) => {
            commands::doctor::run(args.prune, args.check).map_err(|e| e.to_string())
        }
        Command::Stats(args) => handle_stats(args).map_err(|e| e.to_string()),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn handle_stats(args: cli::StatsArgs) -> Result<(), anyhow::Error> {
    if args.projects {
        let since = if args.all {
            None
        } else {
            Some((chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339())
        };
        analytics::projects::run(since.as_deref(), args.agent.as_deref())?;
    } else if args.history {
        let since = if args.all {
            None
        } else {
            Some((chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339())
        };
        analytics::history::run(
            since.as_deref(),
            args.project.as_deref(),
            args.agent.as_deref(),
            &args.format,
        )?;
    } else if args.graph {
        let since = if args.all {
            None
        } else {
            Some((chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339())
        };
        analytics::graph::run(
            since.as_deref(),
            args.project.as_deref(),
            args.agent.as_deref(),
        )?;
    } else if args.daily {
        let since = if args.all {
            None
        } else {
            Some((chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339())
        };
        analytics::daily::run(
            since.as_deref(),
            args.project.as_deref(),
            args.agent.as_deref(),
        )?;
    } else {
        analytics::stats::run(
            args.all,
            args.project.as_deref(),
            args.agent.as_deref(),
            &args.format,
        )?;
    }
    Ok(())
}
