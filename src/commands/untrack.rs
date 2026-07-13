use std::io::{self, Write};
use std::path::Path;

use anyhow::{anyhow, Result};
use colored::Colorize;
use rusqlite::Connection;

use crate::cli::UntrackArgs;
use crate::config::{self, Config};
use crate::db;

pub fn run(args: UntrackArgs) -> Result<()> {
    let config_path = config::config_path();
    let conn = db::open()?;
    let outcome = run_with(&args, &conn, &config_path, &interactive_confirm)?;
    print_outcome(&outcome);
    Ok(())
}

#[derive(Debug, Clone)]
pub enum UntrackOutcome {
    Removed {
        path: String,
        name: Option<String>,
        preserved_turns: u64,
    },
    Aborted,
}

/// Testable core. Confirmation is delegated to `confirm` so tests can inject
/// a closure that doesn't touch stdin.
pub fn run_with(
    args: &UntrackArgs,
    conn: &Connection,
    config_path: &Path,
    confirm: &dyn Fn(&str, Option<&str>) -> bool,
) -> Result<UntrackOutcome> {
    let mut cfg = config::load_from(config_path)?;
    let idx = resolve_target(&cfg, &args.target)?;

    let entry = cfg.projects[idx].clone();
    let entry_path = entry.path.clone();
    let entry_name = entry.name.clone();

    if !args.yes && !confirm(&entry_path, entry_name.as_deref()) {
        return Ok(UntrackOutcome::Aborted);
    }

    cfg.projects.remove(idx);
    config::save_to(&cfg, config_path)?;

    let preserved_turns = count_attributed_turns(conn, &entry_path)?;

    Ok(UntrackOutcome::Removed {
        path: entry_path,
        name: entry_name,
        preserved_turns,
    })
}

/// Resolve `target` (a path or a project name) to an index in `cfg.projects`.
///
/// Tries path-equality first (best-effort canonicalize; falls back to literal
/// string match if the path doesn't exist on disk anymore). Then tries name.
/// Errors on no match; errors on multiple matches by name.
fn resolve_target(cfg: &Config, target: &str) -> Result<usize> {
    let raw_target = config::expand_tilde(target);
    let canonical_target = std::fs::canonicalize(&raw_target).ok();

    let mut path_matches: Vec<usize> = Vec::new();
    for (i, entry) in cfg.projects.iter().enumerate() {
        // Try canonical comparison.
        if let Some(ct) = canonical_target.as_ref() {
            for p in config::expand_globs(&entry.path) {
                if std::fs::canonicalize(&p).ok().as_ref() == Some(ct) {
                    path_matches.push(i);
                    break;
                }
            }
        }
        // Also accept a literal string match against the stored path
        // (covers untrack of a now-deleted directory).
        if entry.path == raw_target && !path_matches.contains(&i) {
            path_matches.push(i);
        }
    }

    if path_matches.len() == 1 {
        return Ok(path_matches[0]);
    }
    if path_matches.len() > 1 {
        return Err(anyhow!(
            "ambiguous path '{}' matches {} entries; remove duplicates from the config",
            target,
            path_matches.len()
        ));
    }

    // Fall back to name.
    let name_matches: Vec<usize> = cfg
        .projects
        .iter()
        .enumerate()
        .filter(|(_, e)| e.name.as_deref() == Some(target))
        .map(|(i, _)| i)
        .collect();
    match name_matches.len() {
        0 => Err(anyhow!("no tracked project matches '{}'", target)),
        1 => Ok(name_matches[0]),
        n => Err(anyhow!(
            "ambiguous name '{}' ({} entries); use the path instead",
            target,
            n
        )),
    }
}

fn count_attributed_turns(conn: &Connection, project_path: &str) -> Result<u64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM turns WHERE project_path = ?1",
        rusqlite::params![project_path],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as u64)
}

fn interactive_confirm(path: &str, name: Option<&str>) -> bool {
    let label = match name {
        Some(n) => format!("'{}' (name: {})", path, n),
        None => format!("'{}'", path),
    };
    print!("Untrack {}? Historical turns are preserved. (y/N) ", label);
    let _ = io::stdout().flush();
    let mut buf = String::new();
    if io::stdin().read_line(&mut buf).is_err() {
        return false;
    }
    matches!(buf.trim().chars().next(), Some('y') | Some('Y'))
}

fn print_outcome(outcome: &UntrackOutcome) {
    match outcome {
        UntrackOutcome::Aborted => println!("Aborted."),
        UntrackOutcome::Removed {
            path,
            name,
            preserved_turns,
        } => {
            let label = match name {
                Some(n) => format!("{} ({})", path, n),
                None => path.clone(),
            };
            println!(
                "{} {}. History preserved ({} turn(s)).",
                "Untracked".green().bold(),
                label,
                preserved_turns
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectEntry;
    use tempfile::TempDir;

    fn always_yes(_p: &str, _n: Option<&str>) -> bool {
        true
    }
    fn always_no(_p: &str, _n: Option<&str>) -> bool {
        false
    }
    fn panic_if_called(_p: &str, _n: Option<&str>) -> bool {
        panic!("confirm should not be called when --yes is set");
    }

    fn args(target: &str, yes: bool) -> UntrackArgs {
        UntrackArgs {
            target: target.to_string(),
            yes,
        }
    }

    fn fresh_with(entries: Vec<(&str, Option<&str>)>) -> (TempDir, Connection, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let cfg_path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        for (path, name) in entries {
            cfg.projects.push(ProjectEntry {
                path: path.to_string(),
                name: name.map(|s| s.to_string()),
            });
        }
        config::save_to(&cfg, &cfg_path).unwrap();
        let conn = db::open_at(&dir.path().join("history.db")).unwrap();
        (dir, conn, cfg_path)
    }

    #[test]
    fn test_untrack_removes_by_literal_path() {
        let (_d, conn, cfg_path) = fresh_with(vec![("/code/limes", Some("Limes"))]);
        let outcome = run_with(
            &args("/code/limes", true),
            &conn,
            &cfg_path,
            &panic_if_called,
        )
        .unwrap();
        match outcome {
            UntrackOutcome::Removed { path, .. } => assert_eq!(path, "/code/limes"),
            _ => panic!("expected Removed"),
        }
        let loaded = config::load_from(&cfg_path).unwrap();
        assert!(loaded.projects.is_empty());
    }

    #[test]
    fn test_untrack_removes_by_name() {
        let (_d, conn, cfg_path) = fresh_with(vec![("/code/limes", Some("Limes"))]);
        let outcome = run_with(&args("Limes", true), &conn, &cfg_path, &always_yes).unwrap();
        match outcome {
            UntrackOutcome::Removed { name, .. } => assert_eq!(name.as_deref(), Some("Limes")),
            _ => panic!("expected Removed"),
        }
    }

    #[test]
    fn test_untrack_errors_on_unknown_target() {
        let (_d, conn, cfg_path) = fresh_with(vec![("/code/limes", Some("Limes"))]);
        let err = run_with(&args("Nope", true), &conn, &cfg_path, &always_yes).unwrap_err();
        assert!(err.to_string().contains("no tracked project matches"));
    }

    #[test]
    fn test_untrack_errors_on_ambiguous_name() {
        let (_d, conn, cfg_path) =
            fresh_with(vec![("/code/a", Some("Dup")), ("/code/b", Some("Dup"))]);
        let err = run_with(&args("Dup", true), &conn, &cfg_path, &always_yes).unwrap_err();
        assert!(err.to_string().contains("ambiguous name"));
    }

    #[test]
    fn test_untrack_preserves_db_rows() {
        let (_d, conn, cfg_path) = fresh_with(vec![("/code/limes", Some("Limes"))]);
        // Insert two attributed turns.
        for sess in ["a", "b"] {
            let id = db::insert_turn(
                &conn,
                &db::TurnInsert {
                    session_id: sess,
                    started_at: "2024-06-01T10:00:00Z",
                    cwd: "/code/limes",
                    agent: "claude-code",
                    model: None,
                    project_path: Some("/code/limes"),
                    project_name: Some("Limes"),
                },
            )
            .unwrap();
            db::stop_turn(
                &conn,
                id,
                &db::TurnStop {
                    ended_at: "2024-06-01T10:05:00Z".to_string(),
                    agent_duration_secs: 1.0,
                    effective_duration_secs: 1.0,
                },
            )
            .unwrap();
        }

        let outcome = run_with(&args("Limes", true), &conn, &cfg_path, &always_yes).unwrap();
        match outcome {
            UntrackOutcome::Removed {
                preserved_turns, ..
            } => assert_eq!(preserved_turns, 2),
            _ => panic!("expected Removed"),
        }
        // DB rows still carry their attribution.
        let rows = db::query_turns(&conn, None, None, None, None).unwrap();
        assert_eq!(rows.len(), 2);
        for r in &rows {
            assert_eq!(r.project_name.as_deref(), Some("Limes"));
        }
    }

    #[test]
    fn test_untrack_yes_skips_prompt() {
        let (_d, conn, cfg_path) = fresh_with(vec![("/code/limes", Some("Limes"))]);
        // panic_if_called confirms that --yes never invokes the prompt.
        let outcome = run_with(&args("Limes", true), &conn, &cfg_path, &panic_if_called).unwrap();
        assert!(matches!(outcome, UntrackOutcome::Removed { .. }));
    }

    #[test]
    fn test_untrack_aborted_when_confirm_says_no() {
        let (_d, conn, cfg_path) = fresh_with(vec![("/code/limes", Some("Limes"))]);
        let outcome = run_with(&args("Limes", false), &conn, &cfg_path, &always_no).unwrap();
        assert!(matches!(outcome, UntrackOutcome::Aborted));
        // Config still has the entry.
        let loaded = config::load_from(&cfg_path).unwrap();
        assert_eq!(loaded.projects.len(), 1);
    }
}
