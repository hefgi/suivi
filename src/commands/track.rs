use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use colored::Colorize;
use rusqlite::Connection;

use crate::cli::TrackArgs;
use crate::config::{self, Config, ProjectEntry};
use crate::{analytics, db};

pub fn run(args: TrackArgs) -> Result<()> {
    let config_path = config::config_path();
    let conn = db::open()?;
    let summary = run_with(&args, &conn, &config_path)?;
    print_summary(&summary, args.no_backfill);
    Ok(())
}

/// Outcome of a `track` invocation. Holds the data needed for the user-facing
/// summary and used by tests to assert behavior without parsing stdout.
#[derive(Debug, Clone)]
pub struct TrackOutcome {
    pub absolute_path: String,
    pub display_name: String,
    pub backfilled: Option<BackfillSummary>,
}

#[derive(Debug, Clone, Default)]
pub struct BackfillSummary {
    pub count: usize,
    pub first_day: Option<String>,
    pub last_day: Option<String>,
}

/// Testable core. Mirrors `run` but takes an injected DB connection and a
/// config path so tests can drive both against tempdirs.
pub fn run_with(args: &TrackArgs, conn: &Connection, config_path: &Path) -> Result<TrackOutcome> {
    // 1. Path resolution + validation.
    let raw = config::expand_tilde(&args.path);
    if has_glob_chars(&raw) {
        return Err(anyhow!(
            "globs are not supported by `suivi track`; edit {} directly to add glob entries",
            config_path.display()
        ));
    }
    let abs = std::fs::canonicalize(&raw)
        .map_err(|_| anyhow!("no such directory: {}", args.path))?;
    if !abs.is_dir() {
        return Err(anyhow!("not a directory: {}", abs.display()));
    }
    let abs_str = abs.to_string_lossy().to_string();

    // 2. Load config and detect duplicate (canonicalize-equality, NOT find_project
    // semantics — tracking a child of an already-tracked dir is legitimate).
    let mut cfg = if config_path.exists() {
        config::load_from(config_path)?
    } else {
        Config::default()
    };
    for entry in &cfg.projects {
        for p in config::expand_globs(&entry.path) {
            if let Ok(c) = std::fs::canonicalize(&p) {
                if c == abs {
                    return Err(anyhow!("already tracked: {}", entry.path));
                }
            }
        }
    }

    // 3. Name.
    let display_name = args
        .name
        .clone()
        .or_else(|| abs.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| abs_str.clone());

    // 4. Push entry and save.
    cfg.projects.push(ProjectEntry {
        path: abs_str.clone(),
        name: Some(display_name.clone()),
    });
    config::save_to(&cfg, config_path)?;

    // 5. Backfill (unless --no-backfill). Wrapped in a single transaction.
    let backfilled = if args.no_backfill {
        None
    } else {
        Some(backfill(conn, &cfg, &abs)?)
    };

    Ok(TrackOutcome {
        absolute_path: abs_str,
        display_name,
        backfilled,
    })
}

fn backfill(conn: &Connection, cfg: &Config, new_abs: &Path) -> Result<BackfillSummary> {
    let candidates = db::unattributed_turns_under(conn, &new_abs.to_string_lossy())?;
    let tx = conn.unchecked_transaction()?;

    let new_abs_str = new_abs.to_string_lossy().to_string();
    let mut count = 0usize;
    let mut first_day: Option<String> = None;
    let mut last_day: Option<String> = None;

    for (id, cwd, started_at) in candidates {
        // CRITICAL: route through `find_project` so a deeper, pre-existing
        // entry keeps ownership of its own turns — longest-match wins,
        // matching `src/hooks/pre.rs` exactly.
        let Some((entry, resolved)) = config::find_project(cfg, &cwd) else {
            continue;
        };
        if resolved != new_abs {
            continue;
        }
        let name = entry.name.clone().or_else(|| {
            resolved
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        });
        db::set_project_attribution(&tx, id, &new_abs_str, name.as_deref())?;
        count += 1;

        if let Some(day) = analytics::local_day_key(&started_at) {
            if first_day.as_ref().map_or(true, |f| &day < f) {
                first_day = Some(day.clone());
            }
            if last_day.as_ref().map_or(true, |l| &day > l) {
                last_day = Some(day);
            }
        }
    }
    tx.commit()?;

    Ok(BackfillSummary {
        count,
        first_day,
        last_day,
    })
}

fn has_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

fn print_summary(outcome: &TrackOutcome, no_backfill: bool) {
    println!(
        "{} {} as {}",
        "Tracking".green().bold(),
        outcome.absolute_path,
        format!("\"{}\"", outcome.display_name).cyan(),
    );
    match (&outcome.backfilled, no_backfill) {
        (_, true) => println!("Skipped backfill (--no-backfill)."),
        (Some(b), _) if b.count > 0 => {
            let range = match (&b.first_day, &b.last_day) {
                (Some(a), Some(b)) if a == b => a.clone(),
                (Some(a), Some(b)) => format!("{} → {}", a, b),
                _ => String::new(),
            };
            if range.is_empty() {
                println!("Backfilled {} turn(s).", b.count);
            } else {
                println!("Backfilled {} turn(s) ({}).", b.count, range);
            }
        }
        (Some(_), _) => println!("No historical turns to backfill."),
        (None, _) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn args(path: &str, name: Option<&str>, no_backfill: bool) -> TrackArgs {
        TrackArgs {
            path: path.to_string(),
            name: name.map(|s| s.to_string()),
            no_backfill,
        }
    }

    fn fresh() -> (TempDir, TempDir, Connection, PathBuf) {
        // dir_for_project: a real on-disk directory we can canonicalize.
        // dir_for_config: holds the config.toml file.
        let project_dir = TempDir::new().unwrap();
        let config_dir = TempDir::new().unwrap();
        let conn = db::open_at(&config_dir.path().join("history.db")).unwrap();
        let config_path = config_dir.path().join("config.toml");
        (project_dir, config_dir, conn, config_path)
    }

    fn insert_turn_at(conn: &Connection, session: &str, cwd: &str, project_name: Option<&str>) {
        let id = db::insert_turn(
            conn,
            &db::TurnInsert {
                session_id: session,
                started_at: "2024-06-01T10:00:00Z",
                cwd,
                agent: "claude-code",
                model: None,
                project_path: None,
                project_name,
            },
        )
        .unwrap();
        db::stop_turn(
            conn,
            id,
            &db::TurnStop {
                ended_at: "2024-06-01T10:05:00Z".to_string(),
                agent_duration_secs: 60.0,
                effective_duration_secs: 660.0,
            },
        )
        .unwrap();
    }

    #[test]
    fn test_track_adds_entry_with_explicit_name() {
        let (proj, _cfg, conn, cfg_path) = fresh();
        let outcome = run_with(
            &args(proj.path().to_str().unwrap(), Some("MyProj"), true),
            &conn,
            &cfg_path,
        )
        .unwrap();
        assert_eq!(outcome.display_name, "MyProj");

        let loaded = config::load_from(&cfg_path).unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name.as_deref(), Some("MyProj"));
    }

    #[test]
    fn test_track_falls_back_to_dir_basename() {
        let (proj, _cfg, conn, cfg_path) = fresh();
        let outcome = run_with(
            &args(proj.path().to_str().unwrap(), None, true),
            &conn,
            &cfg_path,
        )
        .unwrap();
        // basename of a TempDir is something like `.tmpXXXX` — assert non-empty,
        // and that it matches the directory's own file_name.
        let expected = proj
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_eq!(outcome.display_name, expected);
    }

    #[test]
    fn test_track_rejects_globs() {
        let (_proj, _cfg, conn, cfg_path) = fresh();
        let err = run_with(&args("~/foo/*", None, true), &conn, &cfg_path).unwrap_err();
        assert!(err.to_string().contains("globs are not supported"));
        // Config must not have been written.
        assert!(!cfg_path.exists());
    }

    #[test]
    fn test_track_rejects_missing_directory() {
        let (_proj, _cfg, conn, cfg_path) = fresh();
        let err = run_with(
            &args("/definitely/not/a/real/path/xyz123", None, true),
            &conn,
            &cfg_path,
        )
        .unwrap_err();
        assert!(err.to_string().contains("no such directory"));
    }

    #[test]
    fn test_track_rejects_duplicate() {
        let (proj, _cfg, conn, cfg_path) = fresh();
        let path_str = proj.path().to_str().unwrap();
        run_with(&args(path_str, None, true), &conn, &cfg_path).unwrap();
        let err = run_with(&args(path_str, None, true), &conn, &cfg_path).unwrap_err();
        assert!(err.to_string().contains("already tracked"));
    }

    #[test]
    fn test_track_backfills_unattributed_turns_under_path() {
        let (proj, _cfg, conn, cfg_path) = fresh();
        let abs = std::fs::canonicalize(proj.path()).unwrap();
        let abs_str = abs.to_string_lossy().to_string();

        insert_turn_at(&conn, "under-null", &abs_str, None);
        insert_turn_at(
            &conn,
            "under-nested-null",
            &format!("{}/sub", abs_str),
            None,
        );
        insert_turn_at(&conn, "under-already", &abs_str, Some("Existing"));
        insert_turn_at(&conn, "outside", "/some/other/dir", None);

        let outcome = run_with(
            &args(&abs_str, Some("Limes"), false),
            &conn,
            &cfg_path,
        )
        .unwrap();
        let b = outcome.backfilled.expect("backfill should have run");
        assert_eq!(b.count, 2, "only the two null-named under-path turns");

        // Verify the DB state directly.
        let rows = db::query_turns(&conn, None, None, None, None).unwrap();
        for r in &rows {
            match r.session_id.as_str() {
                "under-null" | "under-nested-null" => {
                    assert_eq!(r.project_name.as_deref(), Some("Limes"));
                }
                "under-already" => {
                    assert_eq!(r.project_name.as_deref(), Some("Existing"));
                }
                "outside" => assert_eq!(r.project_name, None),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn test_track_no_backfill_leaves_db_untouched() {
        let (proj, _cfg, conn, cfg_path) = fresh();
        let abs = std::fs::canonicalize(proj.path()).unwrap();
        let abs_str = abs.to_string_lossy().to_string();
        insert_turn_at(&conn, "x", &abs_str, None);

        let outcome = run_with(&args(&abs_str, None, true), &conn, &cfg_path).unwrap();
        assert!(outcome.backfilled.is_none());

        let rows = db::query_turns(&conn, None, None, None, None).unwrap();
        assert_eq!(rows[0].project_name, None, "DB must be untouched");
    }

    #[test]
    fn test_track_respects_longest_match_for_nested_existing_project() {
        // Pre-track ~/proj/nested. Then `track ~/proj`. A turn whose cwd is
        // under ~/proj/nested must NOT be claimed by ~/proj — find_project
        // returns nested (longest match), so backfill skips it.
        let (proj, _cfg, conn, cfg_path) = fresh();
        let parent = std::fs::canonicalize(proj.path()).unwrap();
        let nested = parent.join("nested");
        std::fs::create_dir_all(&nested).unwrap();

        // Track the nested one first.
        run_with(
            &args(nested.to_str().unwrap(), Some("Nested"), true),
            &conn,
            &cfg_path,
        )
        .unwrap();

        // Insert a NULL-named turn under the nested path.
        let nested_str = nested.to_string_lossy().to_string();
        insert_turn_at(&conn, "in-nested", &nested_str, None);
        // And a NULL-named turn in the parent but outside nested.
        let parent_str = parent.to_string_lossy().to_string();
        insert_turn_at(&conn, "in-parent-only", &parent_str, None);

        // Now track the parent.
        let outcome = run_with(
            &args(parent.to_str().unwrap(), Some("Parent"), false),
            &conn,
            &cfg_path,
        )
        .unwrap();
        let b = outcome.backfilled.expect("backfill should have run");
        assert_eq!(
            b.count, 1,
            "parent should only claim its own turn; nested-owned turn stays unattributed"
        );

        let rows = db::query_turns(&conn, None, None, None, None).unwrap();
        for r in &rows {
            match r.session_id.as_str() {
                "in-nested" => {
                    // Stayed unattributed — nested longer-match owns this cwd
                    // semantically, even though no historical record reflects it.
                    // (A future `suivi doctor --reproject` could fix it; out of scope.)
                    assert_eq!(r.project_name, None);
                }
                "in-parent-only" => {
                    assert_eq!(r.project_name.as_deref(), Some("Parent"));
                }
                _ => unreachable!(),
            }
        }
    }
}
