use crate::error::SuiviError;
use rusqlite::{params, Connection};
use std::path::PathBuf;

/// Returns `$XDG_DATA_HOME/suivi/history.db` if set, otherwise `~/.local/share/suivi/history.db`.
/// PRD specifies XDG paths on every OS; we don't follow `dirs::data_local_dir()` on macOS because
/// it returns `~/Library/Application Support`, which violates the spec.
pub fn db_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    base.join("suivi").join("history.db")
}

/// One-shot migration from the legacy macOS path
/// (`~/Library/Application Support/suivi/history.db`) to the XDG-spec'd location.
/// Runs at most once: if the XDG target already exists, does nothing.
/// No-op on non-macOS systems.
fn migrate_legacy_macos_db() {
    if cfg!(not(target_os = "macos")) {
        return;
    }
    let target = db_path();
    if target.exists() {
        return;
    }
    let Some(home) = dirs::home_dir() else { return };
    let legacy = home
        .join("Library")
        .join("Application Support")
        .join("suivi")
        .join("history.db");
    if !legacy.exists() {
        return;
    }
    if let Some(parent) = target.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let _ = std::fs::rename(&legacy, &target);
}

pub fn open() -> Result<Connection, SuiviError> {
    migrate_legacy_macos_db();
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    open_at(&path)
}

pub fn open_at(path: &std::path::Path) -> Result<Connection, SuiviError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    configure_for_concurrency(&conn)?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Hooks from parallel agent sessions are separate processes writing to the
/// same database; `suivi stats` reads (and auto-prunes) concurrently. With
/// rusqlite defaults a lock collision fails immediately with SQLITE_BUSY and
/// the hook silently drops the turn. Wait out brief collisions instead, and
/// use WAL so readers don't block the writer.
fn configure_for_concurrency(conn: &Connection) -> Result<(), SuiviError> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    // WAL can be unavailable on some filesystems (e.g. network mounts);
    // fall back to the default journal mode rather than failing.
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    Ok(())
}

pub fn init_schema(conn: &Connection) -> Result<(), SuiviError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS turns (
            id                      INTEGER PRIMARY KEY,
            session_id              TEXT NOT NULL,
            started_at              TEXT NOT NULL,
            ended_at                TEXT,
            project_path            TEXT,
            project_name            TEXT,
            cwd                     TEXT NOT NULL,
            agent                   TEXT NOT NULL,
            model                   TEXT,
            agent_duration_secs     REAL,
            effective_duration_secs REAL
        );
        CREATE INDEX IF NOT EXISTS idx_started_at   ON turns(started_at);
        CREATE INDEX IF NOT EXISTS idx_session_id   ON turns(session_id, started_at);
        CREATE INDEX IF NOT EXISTS idx_project_path ON turns(project_path, started_at);
        CREATE INDEX IF NOT EXISTS idx_agent        ON turns(agent, started_at);",
    )?;
    Ok(())
}

pub struct TurnInsert<'a> {
    pub session_id: &'a str,
    pub started_at: &'a str,
    pub cwd: &'a str,
    pub agent: &'a str,
    pub model: Option<&'a str>,
    pub project_path: Option<&'a str>,
    pub project_name: Option<&'a str>,
}

pub fn insert_turn(conn: &Connection, turn: &TurnInsert) -> Result<i64, SuiviError> {
    conn.execute(
        "INSERT INTO turns (session_id, started_at, cwd, agent, model, project_path, project_name)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            turn.session_id,
            turn.started_at,
            turn.cwd,
            turn.agent,
            turn.model,
            turn.project_path,
            turn.project_name,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub struct TurnStop {
    pub ended_at: String,
    pub agent_duration_secs: f64,
    pub effective_duration_secs: f64,
}

pub fn stop_turn(conn: &Connection, id: i64, stop: &TurnStop) -> Result<bool, SuiviError> {
    let rows = conn.execute(
        "UPDATE turns SET ended_at = ?1, agent_duration_secs = ?2, effective_duration_secs = ?3
         WHERE id = ?4 AND ended_at IS NULL",
        params![
            stop.ended_at,
            stop.agent_duration_secs,
            stop.effective_duration_secs,
            id
        ],
    )?;
    Ok(rows > 0)
}

#[allow(dead_code)]
pub struct LastOpenTurn {
    pub id: i64,
    pub started_at: String,
    pub agent_duration_secs: Option<f64>,
}

pub fn last_open_turn(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<LastOpenTurn>, SuiviError> {
    let mut stmt = conn.prepare(
        "SELECT id, started_at, agent_duration_secs FROM turns
         WHERE session_id = ?1 AND ended_at IS NULL
         ORDER BY started_at DESC LIMIT 1",
    )?;
    let result = stmt.query_row(params![session_id], |row| {
        Ok(LastOpenTurn {
            id: row.get(0)?,
            started_at: row.get(1)?,
            agent_duration_secs: row.get(2)?,
        })
    });
    match result {
        Ok(t) => Ok(Some(t)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(SuiviError::Db(e)),
    }
}

#[allow(dead_code)]
pub struct LastEndedTurn {
    pub id: i64,
    pub ended_at: String,
    pub agent_duration_secs: Option<f64>,
    pub effective_duration_secs: Option<f64>,
}

pub fn last_ended_turn(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<LastEndedTurn>, SuiviError> {
    let mut stmt = conn.prepare(
        "SELECT id, ended_at, agent_duration_secs, effective_duration_secs FROM turns
         WHERE session_id = ?1 AND ended_at IS NOT NULL
         ORDER BY ended_at DESC LIMIT 1",
    )?;
    let result = stmt.query_row(params![session_id], |row| {
        Ok(LastEndedTurn {
            id: row.get(0)?,
            ended_at: row.get(1)?,
            agent_duration_secs: row.get(2)?,
            effective_duration_secs: row.get(3)?,
        })
    });
    match result {
        Ok(t) => Ok(Some(t)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(SuiviError::Db(e)),
    }
}

pub fn correct_effective_duration(
    conn: &Connection,
    id: i64,
    new_effective: f64,
) -> Result<(), SuiviError> {
    conn.execute(
        "UPDATE turns SET effective_duration_secs = ?1 WHERE id = ?2",
        params![new_effective, id],
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct OutlierTurn {
    pub id: i64,
    pub session_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub project_name: Option<String>,
    pub agent: String,
    pub agent_duration_secs: f64,
    pub effective_duration_secs: Option<f64>,
}

/// Find turns where either the agent duration OR the wall window
/// (ended_at - started_at) exceeds the cap. The wall-window check catches
/// rows that were already partially clamped (agent_duration_secs ≤ cap) but
/// whose `ended_at` still reflects the original idle-conflated value.
pub fn find_outlier_turns(
    conn: &Connection,
    cap_secs: f64,
) -> Result<Vec<OutlierTurn>, SuiviError> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, started_at, ended_at, project_name, agent,
                agent_duration_secs, effective_duration_secs
         FROM turns
         WHERE ended_at IS NOT NULL
           AND (
                 agent_duration_secs > ?1
                 OR (julianday(ended_at) - julianday(started_at)) * 86400.0 > ?1 + 1.0
               )
         ORDER BY (julianday(ended_at) - julianday(started_at)) * 86400.0 DESC,
                  agent_duration_secs DESC",
    )?;
    let rows = stmt
        .query_map(params![cap_secs], |row| {
            Ok(OutlierTurn {
                id: row.get(0)?,
                session_id: row.get(1)?,
                started_at: row.get(2)?,
                ended_at: row.get(3)?,
                project_name: row.get(4)?,
                agent: row.get(5)?,
                agent_duration_secs: row.get(6)?,
                effective_duration_secs: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Clamp all outlier turns. For each affected row:
///   - agent_duration_secs ← MIN(current, cap_secs)
///   - effective_duration_secs ← 2 * buffer_secs + new agent_duration_secs
///   - ended_at ← started_at + new agent_duration_secs
/// Returns the number of rows modified.
pub fn clamp_outliers(
    conn: &Connection,
    cap_secs: f64,
    buffer_secs: f64,
) -> Result<usize, SuiviError> {
    let updated = conn.execute(
        "UPDATE turns
         SET agent_duration_secs = MIN(COALESCE(agent_duration_secs, 0), ?1),
             effective_duration_secs = 2 * ?2
               + MIN(COALESCE(agent_duration_secs, 0), ?1),
             ended_at = strftime('%Y-%m-%dT%H:%M:%fZ',
                                  julianday(started_at)
                                  + MIN(COALESCE(agent_duration_secs, 0), ?1) / 86400.0)
         WHERE ended_at IS NOT NULL
           AND (
                 agent_duration_secs > ?1
                 OR (julianday(ended_at) - julianday(started_at)) * 86400.0 > ?1 + 1.0
               )",
        params![cap_secs, buffer_secs],
    )?;
    Ok(updated)
}

/// Set the model for a turn. Used by `hook stop` when the model is discovered
/// after the turn was inserted (e.g. Claude Code's UserPromptSubmit payload
/// omits the model — we read it from the transcript at Stop time instead).
pub fn set_model(conn: &Connection, id: i64, model: &str) -> Result<(), SuiviError> {
    conn.execute(
        "UPDATE turns SET model = ?1 WHERE id = ?2",
        params![model, id],
    )?;
    Ok(())
}

pub const STALE_FILTER: &str =
    "NOT (ended_at IS NULL AND (julianday('now') - julianday(started_at)) * 86400.0 > 7200)";

#[derive(Clone)]
#[allow(dead_code)]
pub struct TurnRow {
    pub id: i64,
    pub session_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub project_path: Option<String>,
    pub project_name: Option<String>,
    pub cwd: String,
    pub agent: String,
    pub model: Option<String>,
    pub agent_duration_secs: Option<f64>,
    pub effective_duration_secs: Option<f64>,
}

pub fn query_turns(
    conn: &Connection,
    since: Option<&str>,
    project_path: Option<&str>,
    agent: Option<&str>,
) -> Result<Vec<TurnRow>, SuiviError> {
    let mut sql = format!(
        "SELECT id, session_id, started_at, ended_at, project_path, project_name, cwd, agent, model, agent_duration_secs, effective_duration_secs
         FROM turns WHERE {}", STALE_FILTER
    );
    let mut param_values: Vec<String> = vec![];

    if let Some(s) = since {
        sql.push_str(" AND started_at >= ?");
        param_values.push(s.to_string());
    }
    if let Some(p) = project_path {
        sql.push_str(" AND project_path = ?");
        param_values.push(p.to_string());
    }
    if let Some(a) = agent {
        sql.push_str(" AND agent = ?");
        param_values.push(a.to_string());
    }
    sql.push_str(" ORDER BY started_at ASC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(param_values.iter()), |row| {
        Ok(TurnRow {
            id: row.get(0)?,
            session_id: row.get(1)?,
            started_at: row.get(2)?,
            ended_at: row.get(3)?,
            project_path: row.get(4)?,
            project_name: row.get(5)?,
            cwd: row.get(6)?,
            agent: row.get(7)?,
            model: row.get(8)?,
            agent_duration_secs: row.get(9)?,
            effective_duration_secs: row.get(10)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(SuiviError::Db)
}

pub fn count_stale(conn: &Connection) -> Result<u64, SuiviError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM turns WHERE ended_at IS NULL AND (julianday('now') - julianday(started_at)) * 86400.0 > 7200",
        [],
        |row| row.get(0),
    )?;
    Ok(count as u64)
}

pub fn delete_stale(conn: &Connection) -> Result<u64, SuiviError> {
    let rows = conn.execute(
        "DELETE FROM turns WHERE ended_at IS NULL AND (julianday('now') - julianday(started_at)) * 86400.0 > 7200",
        [],
    )?;
    Ok(rows as u64)
}

pub fn count_beyond_retention(conn: &Connection, retention_days: u32) -> Result<u64, SuiviError> {
    let count: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM turns WHERE started_at < datetime('now', '-{} days')",
            retention_days
        ),
        [],
        |row| row.get(0),
    )?;
    Ok(count as u64)
}

pub fn delete_beyond_retention(conn: &Connection, retention_days: u32) -> Result<u64, SuiviError> {
    let rows = conn.execute(
        &format!(
            "DELETE FROM turns WHERE started_at < datetime('now', '-{} days')",
            retention_days
        ),
        [],
    )?;
    Ok(rows as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_conn() -> (Connection, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let conn = open_at(&path).unwrap();
        (conn, dir)
    }

    #[test]
    fn test_init_schema() {
        let (conn, _dir) = test_conn();
        // If schema init runs twice, it should not error (IF NOT EXISTS)
        init_schema(&conn).unwrap();
    }

    #[test]
    fn test_open_configures_concurrency() {
        let (conn, _dir) = test_conn();
        let timeout_ms: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout_ms, 5000);
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_lowercase(), "wal");
    }

    #[test]
    fn test_concurrent_connections_can_both_write() {
        // Two connections to the same file — the second write must succeed
        // rather than failing with SQLITE_BUSY while the first holds the db.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let conn_a = open_at(&path).unwrap();
        let conn_b = open_at(&path).unwrap();

        let turn = |sess: &'static str| TurnInsert {
            session_id: sess,
            started_at: "2024-01-01T10:00:00Z",
            cwd: "/tmp",
            agent: "claude-code",
            model: None,
            project_path: None,
            project_name: None,
        };
        insert_turn(&conn_a, &turn("a")).unwrap();
        insert_turn(&conn_b, &turn("b")).unwrap();

        let count: i64 = conn_a
            .query_row("SELECT COUNT(*) FROM turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_insert_and_query_turn() {
        let (conn, _dir) = test_conn();
        let turn = TurnInsert {
            session_id: "sess1",
            started_at: "2024-01-01T10:00:00Z",
            cwd: "/home/user/project",
            agent: "claude-code",
            model: Some("claude-3-5-sonnet"),
            project_path: Some("/home/user/project"),
            project_name: Some("My Project"),
        };
        let id = insert_turn(&conn, &turn).unwrap();
        // Close the turn so it is not filtered by the stale-open-turn filter
        stop_turn(
            &conn,
            id,
            &TurnStop {
                ended_at: "2024-01-01T10:30:00Z".to_string(),
                agent_duration_secs: 10.0,
                effective_duration_secs: 610.0,
            },
        )
        .unwrap();
        let rows = query_turns(&conn, None, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, "sess1");
        assert_eq!(rows[0].agent, "claude-code");
        assert_eq!(rows[0].model.as_deref(), Some("claude-3-5-sonnet"));
    }

    #[test]
    fn test_stop_turn() {
        let (conn, _dir) = test_conn();
        let turn = TurnInsert {
            session_id: "sess2",
            started_at: "2024-01-01T10:00:00Z",
            cwd: "/tmp",
            agent: "claude-code",
            model: None,
            project_path: None,
            project_name: None,
        };
        let id = insert_turn(&conn, &turn).unwrap();
        let stop = TurnStop {
            ended_at: "2024-01-01T10:05:00Z".to_string(),
            agent_duration_secs: 30.0,
            effective_duration_secs: 630.0,
        };
        let updated = stop_turn(&conn, id, &stop).unwrap();
        assert!(updated);
        let rows = query_turns(&conn, None, None, None).unwrap();
        assert_eq!(rows[0].ended_at.as_deref(), Some("2024-01-01T10:05:00Z"));
        assert_eq!(rows[0].agent_duration_secs, Some(30.0));
    }

    #[test]
    fn test_stop_turn_double_fire_guard() {
        let (conn, _dir) = test_conn();
        let turn = TurnInsert {
            session_id: "sess3",
            started_at: "2024-01-01T10:00:00Z",
            cwd: "/tmp",
            agent: "codex",
            model: None,
            project_path: None,
            project_name: None,
        };
        let id = insert_turn(&conn, &turn).unwrap();
        let stop = TurnStop {
            ended_at: "2024-01-01T10:05:00Z".to_string(),
            agent_duration_secs: 30.0,
            effective_duration_secs: 630.0,
        };
        let first = stop_turn(&conn, id, &stop).unwrap();
        let second = stop_turn(&conn, id, &stop).unwrap();
        assert!(first);
        assert!(!second); // double-fire guard
    }

    #[test]
    fn test_last_open_turn() {
        let (conn, _dir) = test_conn();
        let t1 = TurnInsert {
            session_id: "sess4",
            started_at: "2024-01-01T10:00:00Z",
            cwd: "/tmp",
            agent: "claude-code",
            model: None,
            project_path: None,
            project_name: None,
        };
        let t2 = TurnInsert {
            session_id: "sess4",
            started_at: "2024-01-01T10:10:00Z",
            cwd: "/tmp",
            agent: "claude-code",
            model: None,
            project_path: None,
            project_name: None,
        };
        insert_turn(&conn, &t1).unwrap();
        insert_turn(&conn, &t2).unwrap();
        let last = last_open_turn(&conn, "sess4").unwrap().unwrap();
        assert_eq!(last.started_at, "2024-01-01T10:10:00Z");
    }

    #[test]
    fn test_last_ended_turn() {
        let (conn, _dir) = test_conn();
        let turn = TurnInsert {
            session_id: "sess5",
            started_at: "2024-01-01T10:00:00Z",
            cwd: "/tmp",
            agent: "claude-code",
            model: None,
            project_path: None,
            project_name: None,
        };
        let id = insert_turn(&conn, &turn).unwrap();
        let stop = TurnStop {
            ended_at: "2024-01-01T10:05:00Z".to_string(),
            agent_duration_secs: 30.0,
            effective_duration_secs: 630.0,
        };
        stop_turn(&conn, id, &stop).unwrap();
        let result = last_ended_turn(&conn, "sess5").unwrap().unwrap();
        assert_eq!(result.ended_at, "2024-01-01T10:05:00Z");
        assert_eq!(result.agent_duration_secs, Some(30.0));
    }

    #[test]
    fn test_correct_effective_duration() {
        let (conn, _dir) = test_conn();
        let turn = TurnInsert {
            session_id: "sess6",
            started_at: "2024-01-01T10:00:00Z",
            cwd: "/tmp",
            agent: "claude-code",
            model: None,
            project_path: None,
            project_name: None,
        };
        let id = insert_turn(&conn, &turn).unwrap();
        let stop = TurnStop {
            ended_at: "2024-01-01T10:05:00Z".to_string(),
            agent_duration_secs: 30.0,
            effective_duration_secs: 630.0,
        };
        stop_turn(&conn, id, &stop).unwrap();
        correct_effective_duration(&conn, id, 400.0).unwrap();
        let rows = query_turns(&conn, None, None, None).unwrap();
        assert_eq!(rows[0].effective_duration_secs, Some(400.0));
    }

    #[test]
    fn test_count_stale() {
        let (conn, _dir) = test_conn();
        let turn = TurnInsert {
            session_id: "old_sess",
            started_at: "2020-01-01T00:00:00Z",
            cwd: "/tmp",
            agent: "claude-code",
            model: None,
            project_path: None,
            project_name: None,
        };
        insert_turn(&conn, &turn).unwrap();
        let count = count_stale(&conn).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_delete_stale() {
        let (conn, _dir) = test_conn();
        let turn = TurnInsert {
            session_id: "old_sess2",
            started_at: "2020-01-01T00:00:00Z",
            cwd: "/tmp",
            agent: "claude-code",
            model: None,
            project_path: None,
            project_name: None,
        };
        insert_turn(&conn, &turn).unwrap();
        let deleted = delete_stale(&conn).unwrap();
        assert_eq!(deleted, 1);
        let count = count_stale(&conn).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_count_beyond_retention() {
        let (conn, _dir) = test_conn();
        let turn = TurnInsert {
            session_id: "old_sess3",
            started_at: "2020-01-01T00:00:00Z",
            cwd: "/tmp",
            agent: "claude-code",
            model: None,
            project_path: None,
            project_name: None,
        };
        // Insert with ended_at so it's not stale
        let id = insert_turn(&conn, &turn).unwrap();
        stop_turn(
            &conn,
            id,
            &TurnStop {
                ended_at: "2020-01-01T00:05:00Z".to_string(),
                agent_duration_secs: 10.0,
                effective_duration_secs: 610.0,
            },
        )
        .unwrap();
        let count = count_beyond_retention(&conn, 30).unwrap();
        assert!(count >= 1);
    }

    #[test]
    fn test_query_turns_with_filter() {
        let (conn, _dir) = test_conn();
        for (proj, agent) in [("/proj/a", "claude-code"), ("/proj/b", "codex")] {
            let turn = TurnInsert {
                session_id: "filter_sess",
                started_at: "2024-06-01T10:00:00Z",
                cwd: proj,
                agent,
                model: None,
                project_path: Some(proj),
                project_name: None,
            };
            let id = insert_turn(&conn, &turn).unwrap();
            stop_turn(
                &conn,
                id,
                &TurnStop {
                    ended_at: "2024-06-01T10:05:00Z".to_string(),
                    agent_duration_secs: 30.0,
                    effective_duration_secs: 630.0,
                },
            )
            .unwrap();
        }
        let rows = query_turns(&conn, None, Some("/proj/a"), None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].agent, "claude-code");
    }

    #[test]
    fn test_query_turns_multi_filter_positional_binding() {
        let (conn, _dir) = test_conn();
        for (sess, proj, agent, started) in [
            ("s1", "/proj/a", "claude-code", "2024-06-01T10:00:00Z"),
            ("s2", "/proj/a", "codex", "2024-06-02T10:00:00Z"),
            ("s3", "/proj/b", "claude-code", "2024-06-02T10:00:00Z"),
            ("s4", "/proj/a", "claude-code", "2024-05-01T10:00:00Z"),
        ] {
            let turn = TurnInsert {
                session_id: sess,
                started_at: started,
                cwd: proj,
                agent,
                model: None,
                project_path: Some(proj),
                project_name: None,
            };
            let id = insert_turn(&conn, &turn).unwrap();
            stop_turn(
                &conn,
                id,
                &TurnStop {
                    ended_at: started.to_string(),
                    agent_duration_secs: 1.0,
                    effective_duration_secs: 1.0,
                },
            )
            .unwrap();
        }
        // since + project + agent all together; only "s1" matches.
        let rows = query_turns(
            &conn,
            Some("2024-06-01T00:00:00Z"),
            Some("/proj/a"),
            Some("claude-code"),
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, "s1");
    }

    fn insert_completed_turn(conn: &Connection, session: &str, agent_secs: f64, eff_secs: f64) {
        let id = insert_turn(
            conn,
            &TurnInsert {
                session_id: session,
                started_at: "2024-01-01T10:00:00Z",
                cwd: "/tmp",
                agent: "claude-code",
                model: None,
                project_path: None,
                project_name: None,
            },
        )
        .unwrap();
        stop_turn(
            conn,
            id,
            &TurnStop {
                ended_at: "2024-01-01T10:05:00Z".to_string(),
                agent_duration_secs: agent_secs,
                effective_duration_secs: eff_secs,
            },
        )
        .unwrap();
    }

    #[test]
    fn test_find_outlier_turns_only_returns_above_cap() {
        let (conn, _dir) = test_conn();
        insert_completed_turn(&conn, "ok1", 60.0, 660.0);
        insert_completed_turn(&conn, "big1", 9000.0, 9600.0);
        insert_completed_turn(&conn, "big2", 100_000.0, 100_600.0);

        let outliers = find_outlier_turns(&conn, 7200.0).unwrap();
        let sessions: Vec<&str> = outliers.iter().map(|o| o.session_id.as_str()).collect();
        assert_eq!(sessions, vec!["big2", "big1"]);
    }

    #[test]
    fn test_clamp_outliers_updates_columns_and_ended_at() {
        let (conn, _dir) = test_conn();
        insert_completed_turn(&conn, "ok1", 60.0, 660.0);
        insert_completed_turn(&conn, "big1", 9000.0, 9600.0);

        let cap = 7200.0;
        let buffer = 300.0;
        let updated = clamp_outliers(&conn, cap, buffer).unwrap();
        assert_eq!(updated, 1);

        let after = find_outlier_turns(&conn, cap).unwrap();
        assert!(after.is_empty(), "no outliers should remain");

        let rows = query_turns(&conn, None, None, None).unwrap();
        let big = rows.iter().find(|r| r.session_id == "big1").unwrap();
        assert_eq!(big.agent_duration_secs, Some(7200.0));
        assert_eq!(big.effective_duration_secs, Some(2.0 * 300.0 + 7200.0));
        let ok = rows.iter().find(|r| r.session_id == "ok1").unwrap();
        assert_eq!(ok.agent_duration_secs, Some(60.0));
    }
}
