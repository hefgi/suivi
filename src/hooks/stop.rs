use crate::{config, db};
use chrono::Utc;
use std::io::Read;
use tracing::{debug, info, instrument, warn};

pub fn handle_stop() {
    if let Err(e) = run() {
        warn!(error = %e, "hook stop failed");
    }
}

#[instrument(skip_all, name = "hook_stop")]
fn run() -> Result<(), anyhow::Error> {
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin)?;
    let stdin = stdin.trim().to_string();
    if stdin.is_empty() {
        debug!("empty stdin; no-op");
        return Ok(());
    }
    debug!(stdin = %stdin, "received payload");

    // Stop payloads don't include cwd — read session_id and duration_ms directly.
    let v: serde_json::Value = match serde_json::from_str(&stdin) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "stop payload is not valid JSON; dropping");
            return Ok(());
        }
    };
    let session_id = match v.get("session_id").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => {
            warn!("stop payload missing session_id; dropping");
            return Ok(());
        }
    };
    let duration_ms: Option<f64> = v.get("duration_ms").and_then(|d| d.as_f64());
    let transcript_path: Option<&str> = v.get("transcript_path").and_then(|s| s.as_str());

    let config = config::load().unwrap_or_default();
    let buffer_secs = config.tracking.human_buffer_secs as f64;
    let agent_duration_secs = duration_ms.unwrap_or(0.0) / 1000.0;
    let effective_duration_secs = buffer_secs + agent_duration_secs + buffer_secs;

    let conn = db::open()?;
    let open_turn = match db::last_open_turn(&conn, &session_id)? {
        Some(t) => t,
        None => {
            warn!(
                session_id = %session_id,
                "no open turn found for session; stop fired without matching pre?"
            );
            return Ok(());
        }
    };

    let ended_at = Utc::now().to_rfc3339();
    db::stop_turn(
        &conn,
        open_turn.id,
        &db::TurnStop {
            ended_at: ended_at.clone(),
            agent_duration_secs,
            effective_duration_secs,
        },
    )?;
    info!(
        turn_id = open_turn.id,
        session_id = %session_id,
        agent_duration_secs,
        effective_duration_secs,
        "turn closed"
    );

    // Claude Code's UserPromptSubmit payload omits the model, so we read it
    // from the transcript at Stop time. Best-effort: ignore parse failures.
    if let Some(path) = transcript_path {
        match read_last_assistant_model(path) {
            Some(model) => {
                if let Err(e) = db::set_model(&conn, open_turn.id, &model) {
                    warn!(error = %e, "failed to write model");
                } else {
                    debug!(turn_id = open_turn.id, model = %model, "model captured from transcript");
                }
            }
            None => {
                debug!(
                    transcript_path = path,
                    "no assistant model found in transcript"
                );
            }
        }
    }

    Ok(())
}

/// Scan a Claude Code transcript JSONL backwards for the most recent
/// `{type: "assistant", message: {model: "..."}}` line and return the model.
fn read_last_assistant_model(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        if let Some(model) = v
            .get("message")
            .and_then(|m| m.get("model"))
            .and_then(|m| m.as_str())
        {
            return Some(model.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn transcript_with(lines: &[&str]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        f.flush().unwrap();
        f
    }

    #[test]
    fn test_read_last_assistant_model_basic() {
        let f = transcript_with(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","model":"claude-opus-4-7","content":"hello"}}"#,
        ]);
        let model = read_last_assistant_model(f.path().to_str().unwrap());
        assert_eq!(model.as_deref(), Some("claude-opus-4-7"));
    }

    #[test]
    fn test_read_last_assistant_model_picks_latest() {
        let f = transcript_with(&[
            r#"{"type":"assistant","message":{"model":"claude-sonnet-4-6"}}"#,
            r#"{"type":"user","message":{"content":"again"}}"#,
            r#"{"type":"assistant","message":{"model":"claude-opus-4-7"}}"#,
        ]);
        let model = read_last_assistant_model(f.path().to_str().unwrap());
        assert_eq!(model.as_deref(), Some("claude-opus-4-7"));
    }

    #[test]
    fn test_read_last_assistant_model_missing_returns_none() {
        let f = transcript_with(&[
            r#"{"type":"user","message":{"content":"hi"}}"#,
            r#"{"type":"attachment"}"#,
        ]);
        let model = read_last_assistant_model(f.path().to_str().unwrap());
        assert!(model.is_none());
    }

    #[test]
    fn test_read_last_assistant_model_bad_lines_skipped() {
        let f = transcript_with(&[
            "not json",
            "",
            r#"{"type":"assistant","message":{"model":"claude-opus-4-7"}}"#,
            "garbage",
        ]);
        let model = read_last_assistant_model(f.path().to_str().unwrap());
        assert_eq!(model.as_deref(), Some("claude-opus-4-7"));
    }

    #[test]
    fn test_read_last_assistant_model_nonexistent_file() {
        let model = read_last_assistant_model("/tmp/does-not-exist-suivi-test.jsonl");
        assert!(model.is_none());
    }
}
