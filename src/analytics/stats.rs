use anyhow::Result;
use colored::Colorize;
use serde::Serialize;
use std::collections::HashMap;

use crate::cli::OutputFormat;
use crate::db::TurnRow;
use crate::{config, db};

use super::{agent_secs, format_duration, sessions_column, wall_clock_secs};

#[derive(Debug, Clone, Serialize)]
pub struct StatsSummaryRow {
    pub window: String,
    pub turns: usize,
    pub wall_clock_secs: f64,
    pub agent_secs: f64,
}

pub fn summary_to_json(rows: &[StatsSummaryRow]) -> Result<String> {
    Ok(serde_json::to_string_pretty(rows)?)
}

pub fn summary_to_csv(rows: &[StatsSummaryRow]) -> String {
    let mut out = String::from("window,turns,wall_clock_secs,agent_secs\n");
    for r in rows {
        out.push_str(&format!(
            "{},{},{},{}\n",
            r.window, r.turns, r.wall_clock_secs, r.agent_secs
        ));
    }
    out
}

fn collect_summary_rows(
    conn: &rusqlite::Connection,
    windows: &[(&str, Option<String>)],
    project: Option<&str>,
    agent_filter: Option<&str>,
    buffer_secs: u32,
) -> Result<Vec<StatsSummaryRow>> {
    let mut rows = Vec::with_capacity(windows.len());
    for (label, win_since) in windows {
        let turns = db::query_turns(conn, win_since.as_deref(), project, agent_filter)?;
        let turn_count = turns.iter().filter(|t| t.ended_at.is_some()).count();
        let wall = wall_clock_secs(&turns, buffer_secs);
        let agent = agent_secs(&turns);
        rows.push(StatsSummaryRow {
            window: label.to_string(),
            turns: turn_count,
            wall_clock_secs: wall,
            agent_secs: agent,
        });
    }
    Ok(rows)
}

pub fn turns_to_json_full(turns: &[TurnRow]) -> Result<String> {
    let entries: Vec<serde_json::Value> = turns
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "session_id": t.session_id,
                "started_at": t.started_at,
                "ended_at": t.ended_at,
                "project_path": t.project_path,
                "project_name": t.project_name,
                "cwd": t.cwd,
                "agent": t.agent,
                "model": t.model,
                "agent_duration_secs": t.agent_duration_secs,
                "effective_duration_secs": t.effective_duration_secs,
            })
        })
        .collect();
    Ok(serde_json::to_string_pretty(&entries)?)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    all_time: bool,
    today: bool,
    week: bool,
    month: bool,
    since: Option<&str>,
    project: Option<&str>,
    agent_filter: Option<&str>,
    format: &OutputFormat,
) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let conn = db::open()?;

    let now = chrono::Utc::now();

    // Determine time windows for the summary rows.
    let windows: Vec<(&str, Option<String>)> = if all_time {
        vec![("All time", None)]
    } else if today || week || month || since.is_some() {
        // User specified an explicit time flag — show just that window.
        let label = if today {
            "Today"
        } else if week {
            "Last 7 days"
        } else if month {
            "Last 30 days"
        } else {
            "Custom range"
        };
        vec![(label, since.map(|s| s.to_string()))]
    } else {
        // Default: three windows
        let today_start = super::local_today_start_rfc3339();
        let week_start = (now - chrono::Duration::days(7)).to_rfc3339();
        vec![
            ("Today", today_start),
            ("This week", Some(week_start)),
            ("All time", None),
        ]
    };

    match format {
        OutputFormat::Json => {
            // Full turn-level export when --all, summary rows otherwise (Gap #11)
            if all_time {
                let turns = db::query_turns(&conn, None, project, agent_filter)?;
                println!("{}", turns_to_json_full(&turns)?);
            } else {
                let rows = collect_summary_rows(
                    &conn,
                    &windows,
                    project,
                    agent_filter,
                    cfg.tracking.human_buffer_secs,
                )?;
                println!("{}", summary_to_json(&rows)?);
            }
        }
        OutputFormat::Csv => {
            let rows = collect_summary_rows(
                &conn,
                &windows,
                project,
                agent_filter,
                cfg.tracking.human_buffer_secs,
            )?;
            print!("{}", summary_to_csv(&rows));
        }
        OutputFormat::Text => {
            println!("{}", "suivi — Summary".bold());
            println!("{}", "─".repeat(53));
            println!();

            for (label, win_since) in &windows {
                let turns = db::query_turns(&conn, win_since.as_deref(), project, agent_filter)?;
                let completed: Vec<_> = turns.iter().filter(|t| t.ended_at.is_some()).collect();
                let wall = wall_clock_secs(&turns, cfg.tracking.human_buffer_secs);
                let agent = agent_secs(&turns);

                println!(
                    "  {:<12} wall-clock {:>8}   agent time {:>8}",
                    label.bold(),
                    format_duration(wall),
                    format_duration(agent)
                );
                let _ = completed; // suppress unused warning
            }
            println!();

            // Top projects (this week) — Gap #3
            let week_since = (now - chrono::Duration::days(7)).to_rfc3339();
            let week_turns = db::query_turns(&conn, Some(&week_since), project, agent_filter)?;

            if !week_turns.iter().any(|t| t.ended_at.is_some()) {
                // No data this week — skip the sections
                return Ok(());
            }

            // Group by project
            let mut by_project: HashMap<String, Vec<usize>> = HashMap::new();
            for (i, turn) in week_turns.iter().enumerate() {
                if turn.ended_at.is_none() {
                    continue;
                }
                let key = turn
                    .project_name
                    .clone()
                    .or_else(|| {
                        turn.project_path.as_ref().and_then(|p| {
                            std::path::Path::new(p)
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                        })
                    })
                    .unwrap_or_else(|| "(untracked)".to_string());
                by_project.entry(key).or_default().push(i);
            }

            let mut projects: Vec<(String, Vec<usize>)> = by_project.into_iter().collect();
            projects.sort_by(|a, b| {
                let a_acc: f64 =
                    a.1.iter()
                        .filter_map(|&i| week_turns[i].agent_duration_secs)
                        .sum();
                let b_acc: f64 =
                    b.1.iter()
                        .filter_map(|&i| week_turns[i].agent_duration_secs)
                        .sum();
                b_acc
                    .partial_cmp(&a_acc)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            println!("  {}", "Top projects (this week)".bold());
            println!("  {}", "━".repeat(80));
            println!(
                "  {:<16}  {:>10}  {:>11}  {:>5}  {}",
                "Project".bold(),
                "Wall-clock".bold(),
                "Agent time".bold(),
                "Turns".bold(),
                "Sessions".bold()
            );

            for (name, indices) in projects.iter().take(5) {
                let wall: f64 = {
                    use chrono::Duration;
                    let buffer = Duration::seconds(cfg.tracking.human_buffer_secs as i64);
                    let intervals: Vec<_> = indices
                        .iter()
                        .filter_map(|&i| {
                            let t = &week_turns[i];
                            t.ended_at.as_ref()?;
                            let start = chrono::DateTime::parse_from_rfc3339(&t.started_at).ok()?;
                            let end =
                                chrono::DateTime::parse_from_rfc3339(t.ended_at.as_ref()?).ok()?;
                            Some((
                                start.with_timezone(&chrono::Utc) - buffer,
                                end.with_timezone(&chrono::Utc) + buffer,
                            ))
                        })
                        .collect();
                    super::merge_intervals(intervals)
                };

                let accum: f64 = indices
                    .iter()
                    .filter_map(|&i| week_turns[i].agent_duration_secs)
                    .sum();

                let turn_count = indices.len();
                let sessions = sessions_column(&week_turns, indices);

                let display_name = if name.len() > 16 {
                    format!("{}…", &name[..15])
                } else {
                    name.clone()
                };

                println!(
                    "  {:<16}  {:>10}  {:>11}  {:>5}  {}",
                    display_name,
                    format_duration(wall),
                    format_duration(accum),
                    turn_count,
                    sessions
                );
            }
            println!();

            // Top agents (this week) — Gap #3
            let mut by_agent: HashMap<String, f64> = HashMap::new();
            for turn in &week_turns {
                if turn.ended_at.is_none() {
                    continue;
                }
                *by_agent.entry(turn.agent.clone()).or_default() +=
                    turn.agent_duration_secs.unwrap_or(0.0);
            }

            if by_agent.is_empty() {
                return Ok(());
            }

            let total_agent_secs: f64 = by_agent.values().sum();
            let mut agents: Vec<(String, f64)> = by_agent.into_iter().collect();
            agents.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            println!("  {}", "Top agents (this week)".bold());
            println!("  {}", "━".repeat(53));

            let bar_width = 20usize;
            for (agent, secs) in &agents {
                let pct = if total_agent_secs > 0.0 {
                    (secs / total_agent_secs * 100.0).round() as usize
                } else {
                    0
                };
                let filled = (secs / total_agent_secs * bar_width as f64).round() as usize;
                let filled = filled.min(bar_width);
                let bar = "█".repeat(filled) + &"░".repeat(bar_width - filled);
                println!(
                    "  {:<20}  {:>8}  {}  {}%",
                    agent,
                    format_duration(*secs),
                    bar,
                    pct
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn_fixture() -> Vec<TurnRow> {
        vec![TurnRow {
            id: 7,
            session_id: "s7".to_string(),
            started_at: "2024-06-01T10:00:00Z".to_string(),
            ended_at: Some("2024-06-01T10:05:00Z".to_string()),
            project_path: Some("/proj/a".to_string()),
            project_name: Some("a".to_string()),
            cwd: "/proj/a".to_string(),
            agent: "claude-code".to_string(),
            model: Some("sonnet".to_string()),
            agent_duration_secs: Some(30.0),
            effective_duration_secs: Some(630.0),
        }]
    }

    #[test]
    fn test_summary_to_json_snapshot() {
        let rows = vec![
            StatsSummaryRow {
                window: "Today".to_string(),
                turns: 3,
                wall_clock_secs: 100.0,
                agent_secs: 250.0,
            },
            StatsSummaryRow {
                window: "All time".to_string(),
                turns: 42,
                wall_clock_secs: 9999.5,
                agent_secs: 12345.5,
            },
        ];
        let out = summary_to_json(&rows).unwrap();
        let expected = r#"[
  {
    "window": "Today",
    "turns": 3,
    "wall_clock_secs": 100.0,
    "agent_secs": 250.0
  },
  {
    "window": "All time",
    "turns": 42,
    "wall_clock_secs": 9999.5,
    "agent_secs": 12345.5
  }
]"#;
        assert_eq!(out, expected);
    }

    #[test]
    fn test_summary_to_csv_snapshot() {
        let rows = vec![StatsSummaryRow {
            window: "Today".to_string(),
            turns: 3,
            wall_clock_secs: 100.0,
            agent_secs: 250.0,
        }];
        let out = summary_to_csv(&rows);
        assert_eq!(
            out,
            "window,turns,wall_clock_secs,agent_secs\nToday,3,100,250\n"
        );
    }

    #[test]
    fn test_turns_to_json_full_snapshot() {
        let out = turns_to_json_full(&turn_fixture()).unwrap();
        let expected = r#"[
  {
    "agent": "claude-code",
    "agent_duration_secs": 30.0,
    "cwd": "/proj/a",
    "effective_duration_secs": 630.0,
    "ended_at": "2024-06-01T10:05:00Z",
    "id": 7,
    "model": "sonnet",
    "project_name": "a",
    "project_path": "/proj/a",
    "session_id": "s7",
    "started_at": "2024-06-01T10:00:00Z"
  }
]"#;
        assert_eq!(out, expected);
    }
}
