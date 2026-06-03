use crate::{config, db};
use chrono::Utc;
use std::io::Read;

pub fn handle_stop() {
    let _ = run();
}

fn run() -> Result<(), anyhow::Error> {
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin)?;
    let stdin = stdin.trim().to_string();
    if stdin.is_empty() {
        return Ok(());
    }

    // Stop payloads don't include cwd — read session_id and duration_ms directly.
    let v: serde_json::Value = match serde_json::from_str(&stdin) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let session_id = match v.get("session_id").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => return Ok(()),
    };
    let duration_ms: Option<f64> = v.get("duration_ms").and_then(|d| d.as_f64());

    let config = config::load().unwrap_or_default();
    let buffer_secs = config.buffer_mins as f64 * 60.0;
    let agent_duration_secs = duration_ms.unwrap_or(0.0) / 1000.0;
    let effective_duration_secs = buffer_secs + agent_duration_secs + buffer_secs;

    let conn = db::open()?;
    let open_turn = match db::last_open_turn(&conn, &session_id)? {
        Some(t) => t,
        None => return Ok(()),
    };

    let ended_at = Utc::now().to_rfc3339();
    db::stop_turn(
        &conn,
        open_turn.id,
        &db::TurnStop {
            ended_at,
            agent_duration_secs,
            effective_duration_secs,
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
