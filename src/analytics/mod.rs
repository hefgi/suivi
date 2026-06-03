pub mod daily;
pub mod graph;
pub mod history;
pub mod projects;
pub mod stats;

use chrono::{DateTime, Duration, Utc};

use crate::db::TurnRow;

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
pub fn wall_clock_secs(turns: &[TurnRow], buffer_mins: u32) -> f64 {
    let buffer = Duration::minutes(buffer_mins as i64);
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

/// Compute accumulated seconds for a set of turns.
/// Sums effective_duration_secs for all completed turns.
pub fn accumulated_secs(turns: &[TurnRow]) -> f64 {
    let total: f64 = turns
        .iter()
        .filter(|t| t.ended_at.is_some())
        .filter_map(|t| t.effective_duration_secs)
        .sum();
    total.max(0.0)
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
    fn test_accumulated_sums_all_sessions() {
        // Five parallel 1-minute sessions = 5 minutes accumulated
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
                effective_duration_secs: Some(60.0),
            })
            .collect();
        assert_eq!(accumulated_secs(&turns), 300.0); // 5 × 60
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
}
