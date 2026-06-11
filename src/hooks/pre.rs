use crate::{agents, config, db};
use chrono::Utc;
use std::io::Read;
use tracing::{debug, info, instrument, warn};

pub fn handle_pre(agent_flag: Option<&str>) {
    if let Err(e) = run(agent_flag) {
        warn!(error = %e, "hook pre failed");
    }
}

#[instrument(skip_all, name = "hook_pre")]
fn run(agent_flag: Option<&str>) -> Result<(), anyhow::Error> {
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin)?;
    let stdin = stdin.trim().to_string();
    if stdin.is_empty() {
        debug!("empty stdin; no-op");
        return Ok(());
    }
    debug!(stdin_len = stdin.len(), stdin = %stdin, "received payload");

    let agent = match resolve_agent(agent_flag, &stdin) {
        Some(a) => {
            debug!(agent_id = a.id(), "agent resolved");
            a
        }
        None => {
            warn!("no agent matched; dropping turn");
            return Ok(());
        }
    };

    let payload = match agent.parse_payload(&stdin) {
        Some(p) => p,
        None => {
            warn!(
                agent_id = agent.id(),
                "parse_payload returned None (missing session_id?); dropping turn"
            );
            return Ok(());
        }
    };

    let config = config::load().unwrap_or_default();
    let (project_path, project_name) = match config::find_project(&config, &payload.cwd) {
        Some((entry, path)) => {
            let name = entry
                .name
                .clone()
                .or_else(|| path.file_name().map(|n| n.to_string_lossy().to_string()));
            (Some(path.to_string_lossy().to_string()), name)
        }
        None => {
            debug!(cwd = %payload.cwd, "cwd does not match any tracked project");
            (None, None)
        }
    };

    let conn = db::open()?;
    let now = Utc::now();

    // A turn left open in this session (Stop never fired: interrupt, crash)
    // would otherwise go stale and be pruned. Close it at this prompt's
    // timestamp instead — the gap to the next turn is zero, so the effective
    // time is just the elapsed duration. Skip the buffer correction in that
    // case: the closed turn already carries its final value.
    let closed_orphan = match close_orphaned_turn(&conn, &payload.session_id, now) {
        Ok(closed) => closed,
        Err(e) => {
            warn!(error = %e, "failed to close orphaned open turn");
            false
        }
    };

    // Buffer correction: correct previous turn's effective duration if gap < buffer*2
    let buffer_secs = config.tracking.human_buffer_secs as f64;
    if !closed_orphan {
        apply_buffer_correction(&conn, &payload.session_id, now, buffer_secs);
    }

    let started_at = now.to_rfc3339();
    let id = db::insert_turn(
        &conn,
        &db::TurnInsert {
            session_id: &payload.session_id,
            started_at: &started_at,
            cwd: &payload.cwd,
            agent: agent.id(),
            model: payload.model.as_deref(),
            project_path: project_path.as_deref(),
            project_name: project_name.as_deref(),
        },
    )?;

    info!(
        turn_id = id,
        session_id = %payload.session_id,
        agent = agent.id(),
        project = ?project_name,
        cwd = %payload.cwd,
        "turn inserted"
    );

    Ok(())
}

/// Close a turn whose Stop never fired (interrupt, crash), charging it the
/// elapsed time up to `now`. Returns true if a turn was closed.
fn close_orphaned_turn(
    conn: &rusqlite::Connection,
    session_id: &str,
    now: chrono::DateTime<Utc>,
) -> Result<bool, crate::error::SuiviError> {
    let Some(open) = db::last_open_turn(conn, session_id)? else {
        return Ok(false);
    };
    let duration_secs = chrono::DateTime::parse_from_rfc3339(&open.started_at)
        .map(|s| ((now - s.with_timezone(&Utc)).num_milliseconds() as f64 / 1000.0).max(0.0))
        .unwrap_or(0.0);
    db::stop_turn(
        conn,
        open.id,
        &db::TurnStop {
            ended_at: now.to_rfc3339(),
            agent_duration_secs: duration_secs,
            effective_duration_secs: duration_secs,
        },
    )?;
    info!(
        turn_id = open.id,
        session_id = %session_id,
        duration_secs,
        "closed orphaned open turn at next prompt"
    );
    Ok(true)
}

/// If the previous turn in this session ended less than `buffer*2` seconds
/// ago, replace its buffered effective duration with the real gap + agent time.
fn apply_buffer_correction(
    conn: &rusqlite::Connection,
    session_id: &str,
    now: chrono::DateTime<Utc>,
    buffer_secs: f64,
) {
    if let Ok(Some(prev)) = db::last_ended_turn(conn, session_id) {
        if let Ok(prev_ended) = chrono::DateTime::parse_from_rfc3339(&prev.ended_at) {
            let gap_secs = (now - prev_ended.with_timezone(&Utc)).num_seconds() as f64;
            if gap_secs >= 0.0 && gap_secs < buffer_secs * 2.0 {
                let agent_t = prev.agent_duration_secs.unwrap_or(0.0);
                let new_effective = gap_secs + agent_t;
                if let Err(e) = db::correct_effective_duration(conn, prev.id, new_effective) {
                    warn!(error = %e, prev_turn_id = prev.id, "failed to correct previous turn duration");
                } else {
                    debug!(
                        prev_turn_id = prev.id,
                        gap_secs,
                        new_effective_secs = new_effective,
                        "corrected previous turn duration"
                    );
                }
            }
        }
    }
}

/// Resolve the calling agent, by decreasing trust:
/// 1. the `--agent` flag baked into the installed hook command,
/// 2. the `agent` field inside the payload (sent by the Pi/OpenCode plugins),
/// 3. environment / parent-process sniffing, for hooks installed before the
///    flag existed.
///
/// The explicit sources matter: env sniffing misattributes turns when one
/// agent is launched from inside another's session (inherited env vars), and
/// drops them when the parent process name is a generic runtime like `node`.
fn resolve_agent(agent_flag: Option<&str>, raw_payload: &str) -> Option<Box<dyn agents::Agent>> {
    if let Some(id) = agent_flag {
        match agents::find_by_id(id) {
            Some(a) => return Some(a),
            None => warn!(agent_id = id, "--agent value matches no known agent"),
        }
    }

    let payload_agent = serde_json::from_str::<serde_json::Value>(raw_payload)
        .ok()
        .and_then(|v| v.get("agent").and_then(|a| a.as_str().map(String::from)));
    if let Some(id) = payload_agent {
        match agents::find_by_id(&id) {
            Some(a) => return Some(a),
            None => warn!(agent_id = %id, "payload agent field matches no known agent"),
        }
    }

    let env = agents::Env::capture();
    debug!(
        parent_process = ?env.parent_process_name,
        agent_env_vars = ?env
            .vars
            .keys()
            .filter(|k| {
                let u = k.to_ascii_uppercase();
                u.starts_with("CLAUDE") || u.starts_with("CODEX")
                    || u.starts_with("OPENCODE") || u.starts_with("PI_")
            })
            .collect::<Vec<_>>(),
        "falling back to env detection"
    );
    agents::all_agents().into_iter().find(|a| a.detect(&env))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use tempfile::TempDir;

    fn test_conn() -> (rusqlite::Connection, TempDir) {
        let dir = TempDir::new().unwrap();
        let conn = db::open_at(&dir.path().join("test.db")).unwrap();
        (conn, dir)
    }

    fn utc(s: &str) -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn insert_open(conn: &rusqlite::Connection, session: &str, started_at: &str) -> i64 {
        db::insert_turn(
            conn,
            &db::TurnInsert {
                session_id: session,
                started_at,
                cwd: "/tmp",
                agent: "claude-code",
                model: None,
                project_path: None,
                project_name: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn test_close_orphaned_turn_charges_elapsed_time() {
        let (conn, _dir) = test_conn();
        insert_open(&conn, "sess", "2024-01-01T10:00:00Z");
        let closed = close_orphaned_turn(&conn, "sess", utc("2024-01-01T10:02:00Z")).unwrap();
        assert!(closed);
        let rows = db::query_turns(&conn, None, None, None).unwrap();
        assert_eq!(rows[0].agent_duration_secs, Some(120.0));
        assert_eq!(rows[0].effective_duration_secs, Some(120.0));
        assert!(rows[0].ended_at.is_some());
    }

    #[test]
    fn test_close_orphaned_turn_noop_without_open_turn() {
        let (conn, _dir) = test_conn();
        let closed = close_orphaned_turn(&conn, "sess", utc("2024-01-01T10:00:00Z")).unwrap();
        assert!(!closed);
    }

    #[test]
    fn test_close_orphaned_turn_ignores_other_sessions() {
        let (conn, _dir) = test_conn();
        insert_open(&conn, "other", "2024-01-01T10:00:00Z");
        let closed = close_orphaned_turn(&conn, "sess", utc("2024-01-01T10:02:00Z")).unwrap();
        assert!(!closed);
        // The other session's turn must still be open.
        assert!(db::last_open_turn(&conn, "other").unwrap().is_some());
    }

    #[test]
    fn test_buffer_correction_short_gap_uses_real_gap() {
        let (conn, _dir) = test_conn();
        let id = insert_open(&conn, "sess", "2024-01-01T10:00:00Z");
        db::stop_turn(
            &conn,
            id,
            &db::TurnStop {
                ended_at: "2024-01-01T10:01:00Z".to_string(),
                agent_duration_secs: 60.0,
                effective_duration_secs: 660.0,
            },
        )
        .unwrap();
        // Next prompt 30s after the stop, buffer 300s → corrected to gap + agent time.
        apply_buffer_correction(&conn, "sess", utc("2024-01-01T10:01:30Z"), 300.0);
        let rows = db::query_turns(&conn, None, None, None).unwrap();
        assert_eq!(rows[0].effective_duration_secs, Some(90.0));
    }

    #[test]
    fn test_buffer_correction_long_gap_untouched() {
        let (conn, _dir) = test_conn();
        let id = insert_open(&conn, "sess", "2024-01-01T10:00:00Z");
        db::stop_turn(
            &conn,
            id,
            &db::TurnStop {
                ended_at: "2024-01-01T10:01:00Z".to_string(),
                agent_duration_secs: 60.0,
                effective_duration_secs: 660.0,
            },
        )
        .unwrap();
        // Next prompt 20 minutes later (> buffer*2) → keep the buffered value.
        apply_buffer_correction(&conn, "sess", utc("2024-01-01T10:21:00Z"), 300.0);
        let rows = db::query_turns(&conn, None, None, None).unwrap();
        assert_eq!(rows[0].effective_duration_secs, Some(660.0));
    }

    #[test]
    fn test_resolve_agent_flag_wins() {
        // Payload says opencode, flag says pi — the installed command is the
        // most explicit source and must win.
        let payload = r#"{"session_id":"s","cwd":"/tmp","agent":"opencode"}"#;
        let agent = resolve_agent(Some("pi"), payload).unwrap();
        assert_eq!(agent.id(), "pi");
    }

    #[test]
    fn test_resolve_agent_payload_field_beats_env() {
        // Even when agent env vars are present in the test process (e.g. the
        // suite runs inside a Claude Code session), the payload field wins.
        let payload = r#"{"session_id":"s","cwd":"/tmp","agent":"opencode"}"#;
        let agent = resolve_agent(None, payload).unwrap();
        assert_eq!(agent.id(), "opencode");
    }

    #[test]
    fn test_resolve_agent_unknown_flag_falls_back_to_payload() {
        let payload = r#"{"session_id":"s","cwd":"/tmp","agent":"codex"}"#;
        let agent = resolve_agent(Some("not-an-agent"), payload).unwrap();
        assert_eq!(agent.id(), "codex");
    }

    #[test]
    fn test_resolve_agent_unknown_everywhere_uses_env_detection() {
        // No flag, no payload field — result depends on the test environment,
        // so only assert it doesn't panic.
        let payload = r#"{"session_id":"s","cwd":"/tmp"}"#;
        let _ = resolve_agent(None, payload);
    }
}
