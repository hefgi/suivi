use crate::agents::claude_code::transcript;
use crate::{config, db};
use chrono::{DateTime, Utc};
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
    let max_turn_secs = config.tracking.max_turn_secs as f64;

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

    let now = Utc::now();
    let gap_threshold = config.tracking.transcript_gap_threshold_secs;
    let started_dt = DateTime::parse_from_rfc3339(&open_turn.started_at)
        .ok()
        .map(|d| d.with_timezone(&Utc));

    // For Claude Code turns, the JSONL transcript has a per-event timestamp
    // stream — we can find the real ended_at by walking forward from
    // started_at and truncating at the first gap larger than the threshold.
    // This replaces the cap-based clamping (which was fabricating hours of
    // phantom activity for suspended sessions). Other agents don't emit
    // transcripts; they fall through to `fallback_ended_at` (cap logic).
    let (ended_at, agent_duration_secs, skip_buffer) =
        match (transcript_path, started_dt) {
            (Some(path), Some(started)) => {
                match transcript::last_activity_ended_at(
                    std::path::Path::new(path),
                    started,
                    None, // no next-turn bound at write time
                    gap_threshold,
                ) {
                    Some(real_end) => {
                        let secs = ((real_end - started).num_milliseconds() as f64 / 1000.0)
                            .max(0.0);
                        // Signal-less turn (no in-window activity events):
                        // record zero duration and skip the human buffer.
                        let skip = secs == 0.0;
                        (real_end.to_rfc3339(), secs, skip)
                    }
                    None => {
                        // Transcript unreadable — cap-based fallback.
                        debug!(
                            transcript_path = path,
                            "transcript unreadable; using cap-based fallback for ended_at",
                        );
                        let (e, s) = fallback_ended_at(
                            &open_turn.started_at,
                            duration_ms,
                            now,
                            max_turn_secs,
                            &session_id,
                        );
                        (e, s, false)
                    }
                }
            }
            _ => {
                // No transcript (non-Claude-Code agent, or unparseable started_at).
                let (e, s) = fallback_ended_at(
                    &open_turn.started_at,
                    duration_ms,
                    now,
                    max_turn_secs,
                    &session_id,
                );
                (e, s, false)
            }
        };
    let effective_duration_secs = if skip_buffer {
        0.0
    } else {
        buffer_secs + agent_duration_secs + buffer_secs
    };
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

/// Cap-based `ended_at` used when we have no transcript signal — either
/// the agent doesn't emit a transcript (Codex/Pi/OpenCode), or Claude
/// Code's transcript was unreadable. Returns `(ended_at RFC3339,
/// agent_duration_secs)`. When the raw duration exceeds `max_turn_secs`,
/// both fields are clamped: duration to the cap, ended_at to
/// `started_at + cap` so wall-clock stops crediting phantom idle time to
/// whatever project the turn started in.
fn fallback_ended_at(
    started_at: &str,
    duration_ms: Option<f64>,
    now: DateTime<Utc>,
    max_turn_secs: f64,
    session_id: &str,
) -> (String, f64) {
    let raw = agent_duration(duration_ms, started_at, now);
    let clamped = raw > max_turn_secs;
    let agent_duration_secs = if clamped {
        warn!(
            session_id = %session_id,
            raw_secs = raw,
            cap_secs = max_turn_secs,
            "agent_duration_secs exceeds max_turn_secs; clamping",
        );
        max_turn_secs
    } else {
        raw
    };
    let ended_at = if clamped {
        DateTime::parse_from_rfc3339(started_at)
            .ok()
            .map(|s| {
                (s.with_timezone(&Utc)
                    + chrono::Duration::seconds(agent_duration_secs as i64))
                .to_rfc3339()
            })
            .unwrap_or_else(|| now.to_rfc3339())
    } else {
        now.to_rfc3339()
    };
    (ended_at, agent_duration_secs)
}

/// Agent thinking time for a closing turn. No supported agent actually sends
/// `duration_ms` in its Stop payload (Claude Code and Codex send timestamps
/// only), so when it is absent fall back to wall time since the turn started.
fn agent_duration(duration_ms: Option<f64>, started_at: &str, now: DateTime<Utc>) -> f64 {
    if let Some(ms) = duration_ms {
        return (ms / 1000.0).max(0.0);
    }
    DateTime::parse_from_rfc3339(started_at)
        .map(|s| ((now - s.with_timezone(&Utc)).num_milliseconds() as f64 / 1000.0).max(0.0))
        .unwrap_or(0.0)
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

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn test_agent_duration_prefers_payload_field() {
        let d = agent_duration(
            Some(2500.0),
            "2024-01-01T10:00:00Z",
            utc("2024-01-01T10:01:00Z"),
        );
        assert_eq!(d, 2.5);
    }

    #[test]
    fn test_agent_duration_falls_back_to_timestamps() {
        let d = agent_duration(None, "2024-01-01T10:00:00Z", utc("2024-01-01T10:00:03Z"));
        assert_eq!(d, 3.0);
    }

    #[test]
    fn test_agent_duration_clock_skew_clamped_to_zero() {
        let d = agent_duration(None, "2024-01-01T10:00:10Z", utc("2024-01-01T10:00:00Z"));
        assert_eq!(d, 0.0);
    }

    #[test]
    fn test_agent_duration_unparseable_started_at() {
        let d = agent_duration(None, "garbage", utc("2024-01-01T10:00:00Z"));
        assert_eq!(d, 0.0);
    }

    #[test]
    fn test_fallback_ended_at_under_cap_uses_now() {
        // Turn took 60s, cap is 7200s → no clamping, ended_at = now.
        let now = utc("2024-01-01T10:01:00Z");
        let (ended, secs) = fallback_ended_at(
            "2024-01-01T10:00:00Z",
            Some(60_000.0),
            now,
            7200.0,
            "sess",
        );
        assert_eq!(secs, 60.0);
        // ended_at should equal now.
        assert_eq!(ended, now.to_rfc3339());
    }

    #[test]
    fn test_fallback_ended_at_over_cap_clamps_both_duration_and_end() {
        // Raw = 10h, cap = 2h → duration clamped to 7200, ended_at set
        // to started_at + 7200s (NOT now).
        let now = utc("2024-01-01T20:00:00Z");
        let (ended, secs) = fallback_ended_at(
            "2024-01-01T10:00:00Z",
            Some(36_000_000.0), // 10h in ms
            now,
            7200.0,
            "sess",
        );
        assert_eq!(secs, 7200.0);
        // ended_at should be started_at + 2h = 12:00:00Z.
        let ended_dt = chrono::DateTime::parse_from_rfc3339(&ended)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(ended_dt, utc("2024-01-01T12:00:00Z"));
    }

    #[test]
    fn test_fallback_ended_at_no_duration_ms_falls_back_to_wall() {
        // No duration_ms → derived from `now - started_at` = 3h → clamps at 2h.
        let now = utc("2024-01-01T13:00:00Z");
        let (_ended, secs) = fallback_ended_at(
            "2024-01-01T10:00:00Z",
            None,
            now,
            7200.0,
            "sess",
        );
        assert_eq!(secs, 7200.0);
    }
}
