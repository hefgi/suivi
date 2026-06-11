mod agents;
mod analytics;
mod cli;
mod commands;
mod config;
mod db;
mod error;
mod hooks;
mod logging;

use clap::Parser;
use cli::{Cli, Command, HookEvent};

fn main() {
    // Rust ignores SIGPIPE, which turns `suivi … | head` into a panic on
    // EPIPE. Restore the conventional Unix behavior: die quietly.
    // SAFETY: resetting a signal disposition before any other thread exists.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    // _log_guard holds the non-blocking log writer's worker handle until the
    // end of main() so pending log events are flushed on exit.
    let _log_guard = logging::init();
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
            commands::doctor::run(args.prune, args.check, args.logs).map_err(|e| e.to_string())
        }
        Command::Stats(args) => handle_stats(args).map_err(|e| e.to_string()),
        Command::Uninstall(args) => commands::uninstall::run(args.purge).map_err(|e| e.to_string()),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

/// Start-of-today UTC as RFC3339.
fn today_start_rfc3339() -> Option<String> {
    chrono::Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|naive| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc))
        .map(|d| d.to_rfc3339())
}

/// Resolve --today/--week/--month/--since/--all into an optional RFC3339 lower bound.
/// Returns None for --all (no lower bound). Returns None for the default case too —
/// the default stats view manages its own time windows internally.
fn resolve_since(args: &cli::StatsArgs) -> Option<String> {
    let now = chrono::Utc::now();
    if args.all {
        return None;
    }
    if let Some(date_str) = &args.since {
        // Parse YYYY-MM-DD
        if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            let dt = date.and_hms_opt(0, 0, 0).map(|naive| {
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
            });
            return dt.map(|d| d.to_rfc3339());
        }
    }
    if args.today {
        return today_start_rfc3339();
    }
    if args.week {
        return Some((now - chrono::Duration::days(7)).to_rfc3339());
    }
    if args.month {
        return Some((now - chrono::Duration::days(30)).to_rfc3339());
    }
    // No time flag — return None; callers that need a default window handle it themselves.
    None
}

fn handle_stats(args: cli::StatsArgs) -> Result<(), anyhow::Error> {
    // Auto-prune retention on every stats run (Gap #9)
    if let Ok(conn) = db::open() {
        let retention = config::load().unwrap_or_default().tracking.retention_days;
        let _ = db::delete_beyond_retention(&conn, retention);
    }

    let since = resolve_since(&args);
    let has_time_flag = args.all || args.today || args.week || args.month || args.since.is_some();

    // --project scoped view (Gap #8): when --project is set with no other mode flag
    if args.project.is_some() && !args.projects && !args.history && !args.graph && !args.daily {
        return analytics::project_view::run(
            args.project.as_deref().unwrap(),
            since.as_deref(),
            has_time_flag,
        );
    }

    // --agent scoped view (Gap #7): when --agent is set with no other mode flag and no --project
    if args.agent.is_some()
        && !args.projects
        && !args.history
        && !args.graph
        && !args.daily
        && args.project.is_none()
    {
        return analytics::agent_view::run(
            args.agent.as_deref().unwrap(),
            since.as_deref(),
            has_time_flag,
        );
    }

    if args.projects {
        let query_since = if has_time_flag {
            since.clone()
        } else {
            Some((chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339())
        };
        analytics::projects::run(query_since.as_deref(), args.agent.as_deref())?;
    } else if args.history {
        // PRD: --history defaults to today when no time flag is supplied.
        let query_since = if has_time_flag {
            since.clone()
        } else {
            today_start_rfc3339()
        };
        analytics::history::run(
            query_since.as_deref(),
            args.project.as_deref(),
            args.agent.as_deref(),
            &args.format,
        )?;
    } else if args.graph {
        let query_since = if has_time_flag {
            since.clone()
        } else {
            Some((chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339())
        };
        analytics::graph::run(
            query_since.as_deref(),
            args.project.as_deref(),
            args.agent.as_deref(),
        )?;
    } else if args.daily {
        let query_since = if has_time_flag {
            since.clone()
        } else {
            Some((chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339())
        };
        analytics::daily::run(
            query_since.as_deref(),
            args.project.as_deref(),
            args.agent.as_deref(),
        )?;
    } else {
        analytics::stats::run(
            args.all,
            args.today,
            args.week,
            args.month,
            since.as_deref(),
            args.project.as_deref(),
            args.agent.as_deref(),
            &args.format,
        )?;
    }
    Ok(())
}
