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
                HookEvent::Pre(a) => hooks::pre::handle_pre(a.agent.as_deref()),
                // Stop resolves the turn via session_id alone; the flag is
                // accepted for symmetry with the installed pre command.
                HookEvent::Stop(_) => hooks::stop::handle_stop(),
            }
            Ok(())
        }
        Command::Init => commands::init::run().map_err(|e| e.to_string()),
        Command::Status => commands::status::run().map_err(|e| e.to_string()),
        Command::Doctor(args) => commands::doctor::run(
            args.prune,
            args.check,
            args.logs,
            args.fix_outliers,
            args.yes,
        )
        .map_err(|e| e.to_string()),
        Command::Stats(args) => handle_stats(args).map_err(|e| e.to_string()),
        Command::Uninstall(args) => commands::uninstall::run(args.purge).map_err(|e| e.to_string()),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

/// Start of today's local day as UTC RFC3339.
fn today_start_rfc3339() -> Option<String> {
    analytics::local_today_start_rfc3339()
}

/// Resolve --today/--week/--month/--since/--all into an optional RFC3339 lower bound.
/// Returns Ok(None) for --all (no lower bound) and for the default case —
/// the default stats view manages its own time windows internally.
/// An unparseable --since date is an error, not a silent no-op.
fn resolve_since(args: &cli::StatsArgs) -> Result<Option<String>, anyhow::Error> {
    let now = chrono::Utc::now();
    if args.all {
        return Ok(None);
    }
    if let Some(date_str) = &args.since {
        let date = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").map_err(|_| {
            anyhow::anyhow!("invalid --since date '{}' (expected YYYY-MM-DD)", date_str)
        })?;
        // Interpret the date in the user's timezone, like every other window.
        return Ok(analytics::local_date_start_rfc3339(date));
    }
    if args.today {
        return Ok(today_start_rfc3339());
    }
    if args.week {
        return Ok(Some((now - chrono::Duration::days(7)).to_rfc3339()));
    }
    if args.month {
        return Ok(Some((now - chrono::Duration::days(30)).to_rfc3339()));
    }
    // No time flag — None; callers that need a default window handle it themselves.
    Ok(None)
}

fn handle_stats(args: cli::StatsArgs) -> Result<(), anyhow::Error> {
    let since = resolve_since(&args)?;

    // Auto-prune retention on every stats run (Gap #9)
    if let Ok(conn) = db::open() {
        let retention = config::load().unwrap_or_default().tracking.retention_days;
        let _ = db::delete_beyond_retention(&conn, retention);
    }
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
