use anyhow::Result;
use colored::Colorize;

use crate::{config, db, logging};

pub fn run(prune: bool, check: bool, logs: Option<usize>) -> Result<()> {
    if let Some(n) = logs {
        return print_logs(n);
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
