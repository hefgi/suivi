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

    // Buffer correction: correct previous turn's effective duration if gap < buffer*2
    let buffer_secs = config.tracking.human_buffer_secs as f64;
    if let Ok(Some(prev)) = db::last_ended_turn(&conn, &payload.session_id) {
        if let Ok(prev_ended) = chrono::DateTime::parse_from_rfc3339(&prev.ended_at) {
            let gap_secs = (now - prev_ended.with_timezone(&Utc)).num_seconds() as f64;
            if gap_secs >= 0.0 && gap_secs < buffer_secs * 2.0 {
                let agent_t = prev.agent_duration_secs.unwrap_or(0.0);
                let new_effective = gap_secs + agent_t;
                if let Err(e) = db::correct_effective_duration(&conn, prev.id, new_effective) {
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
