use crate::{agents, config, db};
use chrono::Utc;
use std::io::Read;

pub fn handle_pre() {
    let _ = run();
}

fn run() -> Result<(), anyhow::Error> {
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin)?;
    let stdin = stdin.trim().to_string();
    if stdin.is_empty() {
        return Ok(());
    }

    let env = agents::Env::capture();
    let all = agents::all_agents();
    let agent = all.iter().find(|a| a.detect(&env));
    let agent = match agent {
        Some(a) => a,
        None => return Ok(()),
    };

    let payload = match agent.parse_payload(&stdin) {
        Some(p) => p,
        None => return Ok(()),
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
        None => (None, None),
    };

    let conn = db::open()?;
    let now = Utc::now();

    // Buffer correction: correct previous turn's effective duration if gap < buffer*2
    let buffer_secs = config.buffer_mins as f64 * 60.0;
    if let Ok(Some(prev)) = db::last_ended_turn(&conn, &payload.session_id) {
        if let Ok(prev_ended) = chrono::DateTime::parse_from_rfc3339(&prev.ended_at) {
            let gap_secs = (now - prev_ended.with_timezone(&Utc)).num_seconds() as f64;
            if gap_secs >= 0.0 && gap_secs < buffer_secs * 2.0 {
                let agent_t = prev.agent_duration_secs.unwrap_or(0.0);
                let new_effective = gap_secs + agent_t;
                let _ = db::correct_effective_duration(&conn, prev.id, new_effective);
            }
        }
    }

    let started_at = now.to_rfc3339();
    db::insert_turn(
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

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_compiles() {
        // Phase 4 will add integration tests once main.rs is wired
    }
}
