use anyhow::Result;
use colored::Colorize;

use crate::{config, db, logging};

pub fn run(
    prune: bool,
    check: bool,
    logs: Option<usize>,
    fix_outliers: bool,
    prune_excluded: bool,
    yes: bool,
) -> Result<()> {
    if let Some(n) = logs {
        return print_logs(n);
    }

    if fix_outliers {
        return run_fix_outliers(yes);
    }

    if prune_excluded {
        return run_prune_excluded(yes);
    }

    let conn = db::open()?;

    // Show integrity check by default, and when --check is passed explicitly.
    // Skip when --prune is the only flag (prune-only mode stays fast).
    if check || !prune {
        println!("{}", "Integrity check".bold());
        let result: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap_or_else(|_| "error".to_string());
        if result == "ok" {
            println!("  {}", "ok".green());
        } else {
            println!("  {}", result.red());
        }
        println!();
    }

    let stale_count = db::count_stale(&conn).unwrap_or(0);
    let retention_days = config::load().unwrap_or_default().tracking.retention_days;
    let beyond_retention = db::count_beyond_retention(&conn, retention_days).unwrap_or(0);

    println!("{}", "Database status".bold());
    println!("  Stale turns (open > 2h):    {}", stale_count);
    println!(
        "  Beyond retention ({} days): {}",
        retention_days, beyond_retention
    );

    if prune {
        println!();
        println!("{}", "Pruning".bold());
        let deleted_stale = db::delete_stale(&conn).unwrap_or(0);
        let deleted_old = db::delete_beyond_retention(&conn, retention_days).unwrap_or(0);
        println!("  Deleted {} stale turns", deleted_stale);
        println!("  Deleted {} turns beyond retention", deleted_old);
    } else if stale_count > 0 || beyond_retention > 0 {
        println!();
        println!(
            "Run {} to remove these turns.",
            "'suivi doctor --prune'".cyan()
        );
    }

    Ok(())
}

fn run_fix_outliers(yes: bool) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let cap = cfg.tracking.max_turn_secs as f64;
    let buffer = cfg.tracking.human_buffer_secs as f64;
    let conn = db::open()?;

    let outliers = db::find_outlier_turns(&conn, cap)?;
    if outliers.is_empty() {
        println!(
            "No turns exceed the cap of {} ({}s). Nothing to do.",
            format_secs(cap),
            cap
        );
        return Ok(());
    }

    println!(
        "{}",
        format!(
            "Found {} turn(s) exceeding the {} cap (agent duration OR wall window).",
            outliers.len(),
            format_secs(cap)
        )
        .bold()
    );
    println!();
    println!(
        "  {:<6}  {:<20}  {:<18}  {:>10}  {:>12}  {:>10}",
        "id".bold(),
        "started_at".bold(),
        "project".bold(),
        "agent".bold(),
        "wall_window".bold(),
        "→ clamp".bold(),
    );
    println!("  {}", "─".repeat(90));
    for o in &outliers {
        let project = o.project_name.as_deref().unwrap_or("(untracked)");
        let project_short = if project.len() > 18 {
            format!("{}…", &project[..17])
        } else {
            project.to_string()
        };
        let wall_secs = o.ended_at.as_ref().and_then(|e| {
            let start = chrono::DateTime::parse_from_rfc3339(&o.started_at).ok()?;
            let end = chrono::DateTime::parse_from_rfc3339(e).ok()?;
            Some((end - start).num_seconds() as f64)
        });
        println!(
            "  {:<6}  {:<20}  {:<18}  {:>10}  {:>12}  {:>10}",
            o.id,
            &o.started_at[..o.started_at.len().min(19)],
            project_short,
            format_secs(o.agent_duration_secs),
            wall_secs.map(format_secs).unwrap_or_else(|| "?".to_string()),
            format_secs(cap),
        );
    }
    println!();

    if !yes {
        println!(
            "Dry run. Re-run with {} to clamp these turns.",
            "--fix-outliers --yes".cyan()
        );
        return Ok(());
    }

    let updated = db::clamp_outliers(&conn, cap, buffer)?;
    println!(
        "{}",
        format!(
            "Clamped {} turn(s). agent_duration_secs set to {}, effective_duration_secs to {}.",
            updated,
            cap as u64,
            (2.0 * buffer + cap) as u64,
        )
        .green()
    );
    Ok(())
}

fn run_prune_excluded(yes: bool) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let conn = db::open()?;

    // Collect every distinct cwd in the DB and check it against the exclude
    // list. Doing the match in Rust (rather than SQL) keeps the matching
    // logic identical to the hook-time check in `config::is_excluded`.
    let mut stmt = conn.prepare("SELECT DISTINCT cwd FROM turns")?;
    let cwds: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter(|c| config::is_excluded(&cfg, c))
        .collect();

    if cwds.is_empty() {
        println!("No turns under any excluded path. Nothing to do.");
        return Ok(());
    }

    // Count turns per matched cwd for the preview.
    let mut total = 0usize;
    let mut rows: Vec<(String, i64)> = Vec::new();
    for cwd in &cwds {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM turns WHERE cwd = ?1",
            rusqlite::params![cwd],
            |row| row.get(0),
        )?;
        total += count as usize;
        rows.push((cwd.clone(), count));
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1));

    println!(
        "{}",
        format!(
            "Found {} turn(s) under {} excluded path(s).",
            total,
            cwds.len()
        )
        .bold()
    );
    println!();
    println!("  {:<6}  {}", "turns".bold(), "cwd".bold());
    println!("  {}", "─".repeat(70));
    for (cwd, count) in &rows {
        println!("  {:<6}  {}", count, cwd);
    }
    println!();

    if !yes {
        println!(
            "Dry run. Re-run with {} to delete these turns.",
            "--prune-excluded --yes".cyan()
        );
        return Ok(());
    }

    // Single statement deletion — we already know each cwd matches.
    let mut deleted = 0usize;
    for (cwd, _) in &rows {
        let n = conn.execute(
            "DELETE FROM turns WHERE cwd = ?1",
            rusqlite::params![cwd],
        )?;
        deleted += n;
    }
    println!(
        "{}",
        format!("Deleted {} turn(s) under excluded paths.", deleted).green()
    );
    Ok(())
}

fn format_secs(secs: f64) -> String {
    if secs >= 3600.0 {
        format!("{:.1}h", secs / 3600.0)
    } else if secs >= 60.0 {
        format!("{:.0}m", secs / 60.0)
    } else {
        format!("{:.0}s", secs)
    }
}

/// Print the last `n` lines from suivi log files. Reads every `suivi.log*` file
/// in the log dir (rolling appender writes `suivi.log.YYYY-MM-DD` per day),
/// concatenates them in date order, and tails the last `n` lines.
fn print_logs(n: usize) -> Result<()> {
    let dir = logging::log_dir();
    if !dir.exists() {
        println!(
            "No logs found. Logs are only written when {} is set.",
            "SUIVI_LOG".cyan()
        );
        println!("Try: {}", "SUIVI_LOG=debug suivi stats".cyan());
        return Ok(());
    }

    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|f| f.to_str())
                .map(|f| f.starts_with("suivi.log"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();

    if files.is_empty() {
        println!(
            "No logs found in {}. Logs are only written when {} is set.",
            dir.display(),
            "SUIVI_LOG".cyan()
        );
        return Ok(());
    }

    let mut lines: Vec<String> = Vec::new();
    for f in &files {
        if let Ok(content) = std::fs::read_to_string(f) {
            lines.extend(content.lines().map(|s| s.to_string()));
        }
    }

    let start = lines.len().saturating_sub(n);
    for line in &lines[start..] {
        println!("{}", line);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::db;

    #[test]
    fn test_doctor_prune_sql() {
        // Verify that prune works against a temp DB with stale data
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let conn = db::open_at(&path).unwrap();

        // Insert a stale turn
        db::insert_turn(
            &conn,
            &db::TurnInsert {
                session_id: "old",
                started_at: "2020-01-01T00:00:00Z",
                cwd: "/tmp",
                agent: "claude-code",
                model: None,
                project_path: None,
                project_name: None,
            },
        )
        .unwrap();

        let before = db::count_stale(&conn).unwrap();
        assert_eq!(before, 1);

        let deleted = db::delete_stale(&conn).unwrap();
        assert_eq!(deleted, 1);

        let after = db::count_stale(&conn).unwrap();
        assert_eq!(after, 0);
    }
}
