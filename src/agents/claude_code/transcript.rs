//! Claude Code transcript scanner.
//!
//! Every Claude Code session writes a JSONL transcript to
//! `~/.claude/projects/{slug(cwd)}/{session_id}.jsonl`. Each event carries an
//! ISO-8601 UTC `timestamp`. By walking a turn's events forward from
//! `started_at` and truncating at the first inter-event gap larger than a
//! threshold, we recover the real `ended_at` — which the Stop hook alone
//! can't give us, because Stop fires whenever control returns to the
//! terminal (potentially hours after the agent actually finished).
//!
//! This replaces the hard `max_turn_secs` cap for Claude Code turns. The
//! cap remains in place as a fallback for other agents (Codex/Pi/OpenCode)
//! that don't emit transcripts.

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// Live-work event types in a Claude Code JSONL transcript.
///
/// Deliberately narrow. Claude Code writes many outer `type` values —
/// `mode`, `permission-mode`, `ai-title`, `pr-link`, `queue-operation`,
/// `hook_success`, `edited_text_file`, `queued_command`, `task_reminder`,
/// `agent-name`, `file-history-snapshot`, `skill_listing`, `date_change`,
/// `plan_mode`, `agent_listing_delta`, `deferred_tools_delta`, `last-prompt`,
/// ... — several of which carry timestamps but can fire outside a live
/// session (metadata updates, background hook results). Including them
/// would falsely extend the tail. Validated against 27 real turns across
/// the user's DB; if a new live signal type appears in a future Claude
/// Code version, add it here.
const ACTIVITY_TYPES: &[&str] = &["user", "assistant", "system", "attachment"];

/// Return the timestamp of the last live-work event inside
/// `[started_at, next_turn_start)`, truncating on any inter-event gap
/// larger than `gap_threshold_secs`.
///
/// Best-effort: file I/O errors return `None` (caller falls back to the
/// cap-based logic in the Stop hook). Malformed JSON lines are silently
/// skipped, matching the shape of `read_last_assistant_model` in stop.rs.
///
/// If the transcript is readable but contains no live-work events in the
/// window, returns `Some(started_at)` — a signal-less turn, which the
/// caller records as zero duration.
pub fn last_activity_ended_at(
    path: &Path,
    started_at: DateTime<Utc>,
    next_turn_start: Option<DateTime<Utc>>,
    gap_threshold_secs: u32,
) -> Option<DateTime<Utc>> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut events: Vec<DateTime<Utc>> = content
        .lines()
        .filter_map(|line| parse_event(line, started_at, next_turn_start))
        .collect();

    if events.is_empty() {
        // Transcript readable, no in-window activity: signal-less turn.
        return Some(started_at);
    }
    events.sort();

    let threshold = chrono::Duration::seconds(gap_threshold_secs as i64);
    let mut end = events[0];
    for w in events.windows(2) {
        if w[1] - w[0] > threshold {
            break;
        }
        end = w[1];
    }
    Some(end)
}

fn parse_event(
    line: &str,
    started_at: DateTime<Utc>,
    next_turn_start: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let t = v.get("type").and_then(|t| t.as_str())?;
    if !ACTIVITY_TYPES.contains(&t) {
        return None;
    }
    let ts = v.get("timestamp").and_then(|t| t.as_str())?;
    let dt = DateTime::parse_from_rfc3339(ts).ok()?.with_timezone(&Utc);
    if dt < started_at {
        return None;
    }
    if let Some(bound) = next_turn_start {
        if dt >= bound {
            return None;
        }
    }
    Some(dt)
}

/// Reconstruct the transcript path for a session given its `session_id` and
/// original `cwd`. Wraps `locate_transcript_in` with the real
/// `~/.claude/projects` root; returns `None` if `$HOME` is unavailable.
pub fn locate_transcript(session_id: &str, cwd: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let projects = home.join(".claude").join("projects");
    locate_transcript_in(&projects, session_id, cwd)
}

/// Testable variant. Claude Code slugs the cwd by replacing every `/` with
/// `-` (leading `/` becomes leading `-`), so
/// `/Users/fja/Desktop/Hefgi/Rubbr` → `-Users-fja-Desktop-Hefgi-Rubbr`.
///
/// Falls back to scanning `projects_root/*/{session_id}.jsonl` if the
/// primary slug misses — covers slug edge cases (worktrees, unusual
/// characters, future Claude Code slug rule changes).
pub fn locate_transcript_in(
    projects_root: &Path,
    session_id: &str,
    cwd: &str,
) -> Option<PathBuf> {
    let slug = cwd.replace('/', "-");
    let primary = projects_root
        .join(&slug)
        .join(format!("{}.jsonl", session_id));
    if primary.exists() {
        return Some(primary);
    }

    // Glob fallback: the session_id is a UUID; look under every subdir.
    let entries = std::fs::read_dir(projects_root).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join(format!("{}.jsonl", session_id));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};

    fn write_transcript(lines: &[&str]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        f.flush().unwrap();
        f
    }

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn event(kind: &str, timestamp: &str) -> String {
        format!(r#"{{"type":"{}","timestamp":"{}"}}"#, kind, timestamp)
    }

    #[test]
    fn empty_transcript_returns_started_at() {
        let f = write_transcript(&[]);
        let started = ts("2024-01-01T10:00:00Z");
        let out = last_activity_ended_at(f.path(), started, None, 300).unwrap();
        assert_eq!(out, started);
    }

    #[test]
    fn all_events_before_started_at_returns_started_at() {
        let f = write_transcript(&[
            &event("user", "2024-01-01T09:00:00Z"),
            &event("assistant", "2024-01-01T09:30:00Z"),
        ]);
        let started = ts("2024-01-01T10:00:00Z");
        let out = last_activity_ended_at(f.path(), started, None, 300).unwrap();
        assert_eq!(out, started);
    }

    #[test]
    fn single_cluster_returns_last_event() {
        let f = write_transcript(&[
            &event("user", "2024-01-01T10:00:05Z"),
            &event("assistant", "2024-01-01T10:00:10Z"),
            &event("assistant", "2024-01-01T10:00:15Z"),
        ]);
        let started = ts("2024-01-01T10:00:00Z");
        let out = last_activity_ended_at(f.path(), started, None, 300).unwrap();
        assert_eq!(out, ts("2024-01-01T10:00:15Z"));
    }

    #[test]
    fn two_clusters_split_by_gap_returns_end_of_first() {
        let f = write_transcript(&[
            &event("user", "2024-01-01T10:00:00Z"),
            &event("assistant", "2024-01-01T10:00:05Z"),
            // 10-minute gap here (> 5-minute threshold)
            &event("user", "2024-01-01T10:10:05Z"),
            &event("assistant", "2024-01-01T10:10:10Z"),
        ]);
        let started = ts("2024-01-01T10:00:00Z");
        let out = last_activity_ended_at(f.path(), started, None, 300).unwrap();
        assert_eq!(out, ts("2024-01-01T10:00:05Z"));
    }

    #[test]
    fn next_turn_bound_excludes_events_past_it() {
        let f = write_transcript(&[
            &event("user", "2024-01-01T10:00:05Z"),
            &event("assistant", "2024-01-01T10:00:10Z"),
            // Second turn's events; should be excluded by the bound.
            &event("user", "2024-01-01T10:00:20Z"),
            &event("assistant", "2024-01-01T10:00:25Z"),
        ]);
        let started = ts("2024-01-01T10:00:00Z");
        let bound = Some(ts("2024-01-01T10:00:15Z"));
        let out = last_activity_ended_at(f.path(), started, bound, 300).unwrap();
        assert_eq!(out, ts("2024-01-01T10:00:10Z"));
    }

    #[test]
    fn non_activity_types_ignored() {
        let f = write_transcript(&[
            &event("user", "2024-01-01T10:00:05Z"),
            &event("mode", "2024-01-01T10:00:10Z"),
            &event("pr-link", "2024-01-01T10:00:15Z"),
            &event("hook_success", "2024-01-01T10:00:20Z"),
            &event("assistant", "2024-01-01T10:00:25Z"),
        ]);
        let started = ts("2024-01-01T10:00:00Z");
        let out = last_activity_ended_at(f.path(), started, None, 300).unwrap();
        // Only the two activity events count; assistant at :25 is the last.
        assert_eq!(out, ts("2024-01-01T10:00:25Z"));
    }

    #[test]
    fn malformed_json_lines_skipped() {
        let f = write_transcript(&[
            "not json",
            &event("user", "2024-01-01T10:00:05Z"),
            "{malformed",
            &event("assistant", "2024-01-01T10:00:10Z"),
        ]);
        let started = ts("2024-01-01T10:00:00Z");
        let out = last_activity_ended_at(f.path(), started, None, 300).unwrap();
        assert_eq!(out, ts("2024-01-01T10:00:10Z"));
    }

    #[test]
    fn missing_file_returns_none() {
        let started = ts("2024-01-01T10:00:00Z");
        let out = last_activity_ended_at(Path::new("/no/such/file.jsonl"), started, None, 300);
        assert_eq!(out, None);
    }

    #[test]
    fn gap_exactly_at_threshold_does_not_split() {
        // Threshold = 300s. Gap = exactly 300s: '>' comparison means it
        // does NOT split, so we should walk past it.
        let f = write_transcript(&[
            &event("user", "2024-01-01T10:00:00Z"),
            &event("assistant", "2024-01-01T10:05:00Z"), // 300s after
            &event("assistant", "2024-01-01T10:05:05Z"),
        ]);
        let started = ts("2024-01-01T10:00:00Z");
        let out = last_activity_ended_at(f.path(), started, None, 300).unwrap();
        assert_eq!(out, ts("2024-01-01T10:05:05Z"));
    }

    #[test]
    fn gap_just_over_threshold_splits() {
        // Same as above but +1s past the threshold. Should truncate.
        let f = write_transcript(&[
            &event("user", "2024-01-01T10:00:00Z"),
            &event("assistant", "2024-01-01T10:05:01Z"), // 301s after
        ]);
        let started = ts("2024-01-01T10:00:00Z");
        let out = last_activity_ended_at(f.path(), started, None, 300).unwrap();
        // First event only — walked no further because the gap fired.
        assert_eq!(out, ts("2024-01-01T10:00:00Z"));
    }

    #[test]
    fn events_out_of_order_still_walked_in_time_order() {
        let f = write_transcript(&[
            &event("assistant", "2024-01-01T10:00:15Z"),
            &event("user", "2024-01-01T10:00:05Z"),
            &event("assistant", "2024-01-01T10:00:10Z"),
        ]);
        let started = ts("2024-01-01T10:00:00Z");
        let out = last_activity_ended_at(f.path(), started, None, 300).unwrap();
        assert_eq!(out, ts("2024-01-01T10:00:15Z"));
    }

    #[test]
    fn locate_transcript_in_primary_hit() {
        let root = TempDir::new().unwrap();
        let slug_dir = root.path().join("-Users-fja-code-project");
        std::fs::create_dir_all(&slug_dir).unwrap();
        let session_id = "abc-123";
        let file = slug_dir.join(format!("{}.jsonl", session_id));
        std::fs::write(&file, "").unwrap();

        let found = locate_transcript_in(root.path(), session_id, "/Users/fja/code/project");
        assert_eq!(found, Some(file));
    }

    #[test]
    fn locate_transcript_in_glob_fallback() {
        // File lives under a slug that doesn't match cwd.replace('/', '-').
        // Glob fallback should still find it by session_id.
        let root = TempDir::new().unwrap();
        let unexpected = root.path().join("some-other-dir");
        std::fs::create_dir_all(&unexpected).unwrap();
        let session_id = "abc-123";
        let file = unexpected.join(format!("{}.jsonl", session_id));
        std::fs::write(&file, "").unwrap();

        let found = locate_transcript_in(root.path(), session_id, "/Users/fja/code/project");
        assert_eq!(found, Some(file));
    }

    #[test]
    fn locate_transcript_in_returns_none_when_missing() {
        let root = TempDir::new().unwrap();
        let found = locate_transcript_in(root.path(), "no-such-session", "/whatever");
        assert_eq!(found, None);
    }
}
