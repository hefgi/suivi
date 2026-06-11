pub mod agent_view;
pub mod daily;
pub mod graph;
pub mod history;
pub mod project_view;
pub mod projects;
pub mod stats;

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};

use crate::db::TurnRow;

// ── Timezone handling ─────────────────────────────────────────────────────────
//
// Timestamps are stored as UTC RFC3339, but "a day" means a day in the user's
// timezone: bucketing by the raw UTC date prefix shifts evening work onto the
// wrong day for anyone west of UTC and after-midnight work for anyone east.

/// YYYY-MM-DD of an RFC3339 timestamp, evaluated in `tz`.
pub fn day_key_in<Tz: TimeZone>(rfc3339: &str, tz: &Tz) -> Option<String> {
    let dt = DateTime::parse_from_rfc3339(rfc3339).ok()?;
    Some(
        dt.with_timezone(tz)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string(),
    )
}

/// YYYY-MM-DD of an RFC3339 timestamp in the local timezone.
pub fn local_day_key(rfc3339: &str) -> Option<String> {
    day_key_in(rfc3339, &chrono::Local)
}

/// Midnight of `date` in `tz`, returned as UTC RFC3339 (for query bounds).
pub fn day_start_in<Tz: TimeZone>(date: NaiveDate, tz: &Tz) -> Option<String> {
    let naive = date.and_hms_opt(0, 0, 0)?;
    // A DST gap can make local midnight nonexistent; take the earliest
    // valid instant of the day instead.
    let local = tz.from_local_datetime(&naive).earliest()?;
    Some(local.with_timezone(&Utc).to_rfc3339())
}

/// Start of today's *local* day as UTC RFC3339.
pub fn local_today_start_rfc3339() -> Option<String> {
    day_start_in(chrono::Local::now().date_naive(), &chrono::Local)
}

/// Local midnight of the given date as UTC RFC3339 (used by `--since`).
pub fn local_date_start_rfc3339(date: NaiveDate) -> Option<String> {
    day_start_in(date, &chrono::Local)
}

/// Merge overlapping intervals and return total duration in seconds.
/// Input: list of (start, end) pairs as UTC DateTimes.
pub fn merge_intervals(mut intervals: Vec<(DateTime<Utc>, DateTime<Utc>)>) -> f64 {
    if intervals.is_empty() {
        return 0.0;
    }
    intervals.sort_by_key(|(start, _)| *start);
    let mut merged: Vec<(DateTime<Utc>, DateTime<Utc>)> = vec![intervals[0]];
    for (start, end) in intervals.into_iter().skip(1) {
        let last = merged.last_mut().unwrap();
        if start <= last.1 {
            if end > last.1 {
                last.1 = end;
            }
        } else {
            merged.push((start, end));
        }
    }
    merged
        .iter()
        .map(|(s, e)| (*e - *s).num_seconds() as f64)
        .sum()
}

/// Compute wall-clock seconds for a set of turns.
/// Expands each completed turn to [started_at - B, ended_at + B], merges overlaps, sums.
/// Only uses started_at / ended_at columns.
pub fn wall_clock_secs(turns: &[TurnRow], buffer_secs: u32) -> f64 {
    let buffer = Duration::seconds(buffer_secs as i64);
    let intervals: Vec<(DateTime<Utc>, DateTime<Utc>)> = turns
        .iter()
        .filter(|t| t.ended_at.is_some())
        .filter_map(|t| {
            let start = DateTime::parse_from_rfc3339(&t.started_at).ok()?;
            let end = DateTime::parse_from_rfc3339(t.ended_at.as_ref()?).ok()?;
            Some((
                start.with_timezone(&Utc) - buffer,
                end.with_timezone(&Utc) + buffer,
            ))
        })
        .collect();
    merge_intervals(intervals)
}

/// Compute agent seconds for a set of turns: machine effort, summed as each
/// completed turn's real duration (`agent_duration_secs`, i.e. prompt
/// submitted → response finished). Parallel sessions legitimately add up —
/// five agents running one minute each is five agent-minutes.
pub fn agent_secs(turns: &[TurnRow]) -> f64 {
    let total: f64 = turns
        .iter()
        .filter(|t| t.ended_at.is_some())
        .filter_map(|t| t.agent_duration_secs)
        .sum();
    total.max(0.0)
}

/// Build the Sessions column string: "claude-code ×2  pi ×3  │ 12 total"
/// `indices` are the turn-slice indices belonging to a single project (or all turns for agents view).
pub fn sessions_column(turns: &[TurnRow], indices: &[usize]) -> String {
    use std::collections::{HashMap, HashSet};

    let mut by_agent: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut total_sessions: HashSet<&str> = HashSet::new();

    for &i in indices {
        let t = &turns[i];
        if t.ended_at.is_none() {
            continue;
        }
        by_agent
            .entry(t.agent.as_str())
            .or_default()
            .insert(t.session_id.as_str());
        total_sessions.insert(t.session_id.as_str());
    }

    if by_agent.is_empty() {
        return "│ 0 total".to_string();
    }

    let mut agents: Vec<(&str, usize)> = by_agent
        .iter()
        .map(|(&a, sessions)| (a, sessions.len()))
        .collect();
    agents.sort_by_key(|(a, _)| *a);

    let parts: Vec<String> = agents
        .iter()
        .map(|(agent, count)| format!("{} ×{}", agent, count))
        .collect();

    format!("{}  │ {} total", parts.join("  "), total_sessions.len())
}

/// Format seconds as human-readable duration: "1h 23m" or "45m" or "< 1m"
pub fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        return "< 1m".to_string();
    }
    let total_mins = (secs / 60.0).round() as u64;
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if hours > 0 {
        format!("{}h {:02}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn test_merge_intervals_empty() {
        assert_eq!(merge_intervals(vec![]), 0.0);
    }

    #[test]
    fn test_merge_intervals_single() {
        let intervals = vec![(dt("2024-01-01T10:00:00Z"), dt("2024-01-01T10:01:00Z"))];
        assert_eq!(merge_intervals(intervals), 60.0);
    }

    #[test]
    fn test_merge_intervals_non_overlapping() {
        let intervals = vec![
            (dt("2024-01-01T10:00:00Z"), dt("2024-01-01T10:01:00Z")),
            (dt("2024-01-01T10:02:00Z"), dt("2024-01-01T10:03:00Z")),
        ];
        assert_eq!(merge_intervals(intervals), 120.0);
    }

    #[test]
    fn test_merge_intervals_overlapping() {
        let intervals = vec![
            (dt("2024-01-01T10:00:00Z"), dt("2024-01-01T10:02:00Z")),
            (dt("2024-01-01T10:01:00Z"), dt("2024-01-01T10:03:00Z")),
        ];
        // Should merge to one 3-minute interval
        assert_eq!(merge_intervals(intervals), 180.0);
    }

    #[test]
    fn test_merge_intervals_adjacent() {
        let intervals = vec![
            (dt("2024-01-01T10:00:00Z"), dt("2024-01-01T10:01:00Z")),
            (dt("2024-01-01T10:01:00Z"), dt("2024-01-01T10:02:00Z")),
        ];
        // Touching intervals merge (start of second == end of first)
        assert_eq!(merge_intervals(intervals), 120.0);
    }

    #[test]
    fn test_wall_clock_parallel_sessions() {
        // Five parallel 1-minute sessions = 1 minute wall-clock (with buffer)
        // All start/end at the same time
        let mut turns = Vec::new();
        for i in 0..5u64 {
            turns.push(crate::db::TurnRow {
                id: i as i64,
                session_id: format!("sess{}", i),
                started_at: "2024-01-01T10:00:00Z".to_string(),
                ended_at: Some("2024-01-01T10:01:00Z".to_string()),
                project_path: None,
                project_name: None,
                cwd: "/tmp".to_string(),
                agent: "claude-code".to_string(),
                model: None,
                agent_duration_secs: Some(60.0),
                effective_duration_secs: Some(660.0),
            });
        }
        // Buffer = 0 for this test to simplify
        let wall = wall_clock_secs(&turns, 0);
        assert_eq!(wall, 60.0);
    }

    #[test]
    fn test_agent_secs_sums_all_sessions() {
        // Five parallel 1-minute sessions = 5 minutes of agent time
        let turns: Vec<crate::db::TurnRow> = (0..5)
            .map(|i| crate::db::TurnRow {
                id: i,
                session_id: format!("sess{}", i),
                started_at: "2024-01-01T10:00:00Z".to_string(),
                ended_at: Some("2024-01-01T10:01:00Z".to_string()),
                project_path: None,
                project_name: None,
                cwd: "/tmp".to_string(),
                agent: "claude-code".to_string(),
                model: None,
                agent_duration_secs: Some(60.0),
                // effective carries buffers; agent_secs must ignore it
                effective_duration_secs: Some(660.0),
            })
            .collect();
        assert_eq!(agent_secs(&turns), 300.0); // 5 × 60
    }

    #[test]
    fn test_format_duration_under_1m() {
        assert_eq!(format_duration(30.0), "< 1m");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(90.0), "2m");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3690.0), "1h 02m");
    }

    #[test]
    fn test_day_key_in_shifts_across_midnight() {
        let paris = chrono::FixedOffset::east_opt(2 * 3600).unwrap(); // UTC+2
                                                                      // 23:30 UTC is already the next day in Paris…
        assert_eq!(
            day_key_in("2026-06-10T23:30:00Z", &paris).as_deref(),
            Some("2026-06-11")
        );
        // …but 21:30 UTC is still the same day.
        assert_eq!(
            day_key_in("2026-06-10T21:30:00Z", &paris).as_deref(),
            Some("2026-06-10")
        );
        let la = chrono::FixedOffset::west_opt(8 * 3600).unwrap(); // UTC-8
                                                                   // 02:00 UTC is the previous evening in Los Angeles.
        assert_eq!(
            day_key_in("2026-06-11T02:00:00Z", &la).as_deref(),
            Some("2026-06-10")
        );
    }

    #[test]
    fn test_day_key_in_invalid_timestamp() {
        let tz = chrono::FixedOffset::east_opt(0).unwrap();
        assert!(day_key_in("not a timestamp", &tz).is_none());
    }

    #[test]
    fn test_day_start_in_converts_to_utc() {
        let paris = chrono::FixedOffset::east_opt(2 * 3600).unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 6, 11).unwrap();
        // Local midnight in UTC+2 is 22:00 UTC the previous day.
        assert_eq!(
            day_start_in(date, &paris).as_deref(),
            Some("2026-06-10T22:00:00+00:00")
        );
    }

    #[test]
    fn test_local_helpers_no_panic() {
        // Local-tz results depend on the host; just exercise the paths.
        assert!(local_today_start_rfc3339().is_some());
        let _ = local_day_key("2026-06-10T23:30:00Z");
    }
}
