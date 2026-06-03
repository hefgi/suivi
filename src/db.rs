use crate::error::SuiviError;
use rusqlite::{params, Connection};
use std::path::PathBuf;

pub fn db_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("suivi")
        .join("history.db")
}

pub fn open() -> Result<Connection, SuiviError> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path)?;
    init_schema(&conn)?;
    Ok(conn)
}

pub fn open_at(path: &std::path::Path) -> Result<Connection, SuiviError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    init_schema(&conn)?;
    Ok(conn)
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

pub const STALE_FILTER: &str =
    "NOT (ended_at IS NULL AND (julianday('now') - julianday(started_at)) * 86400.0 > 7200)";

#[derive(Clone)]
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
        sql.push_str(&(param_values.len() + 1).to_string());
        param_values.push(s.to_string());
    }
    if let Some(p) = project_path {
        sql.push_str(" AND project_path = ?");
        sql.push_str(&(param_values.len() + 1).to_string());
        param_values.push(p.to_string());
    }
    if let Some(a) = agent {
        sql.push_str(" AND agent = ?");
        sql.push_str(&(param_values.len() + 1).to_string());
        param_values.push(a.to_string());
    }
    sql.push_str(" ORDER BY started_at ASC");

    // Build query with dynamic parameters
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
}
