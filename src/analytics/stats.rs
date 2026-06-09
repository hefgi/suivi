use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

use crate::cli::OutputFormat;
use crate::{config, db};

use super::{accumulated_secs, format_duration, sessions_column, wall_clock_secs};

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
        let today_start = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|dt| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc))
            .map(|dt| dt.to_rfc3339());
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
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                let mut entries = Vec::new();
                for (label, win_since) in &windows {
                    let turns =
                        db::query_turns(&conn, win_since.as_deref(), project, agent_filter)?;
                    let turn_count = turns.iter().filter(|t| t.ended_at.is_some()).count();
                    let wall = wall_clock_secs(&turns, cfg.tracking.human_buffer_secs);
                    let accum = accumulated_secs(&turns);
                    entries.push(serde_json::json!({
                        "window": label,
                        "turns": turn_count,
                        "wall_clock_secs": wall,
                        "accumulated_secs": accum,
                    }));
                }
                println!("{}", serde_json::to_string_pretty(&entries)?);
            }
        }
        OutputFormat::Csv => {
            println!("window,turns,wall_clock_secs,accumulated_secs");
            for (label, win_since) in &windows {
                let turns = db::query_turns(&conn, win_since.as_deref(), project, agent_filter)?;
                let turn_count = turns.iter().filter(|t| t.ended_at.is_some()).count();
                let wall = wall_clock_secs(&turns, cfg.tracking.human_buffer_secs);
                let accum = accumulated_secs(&turns);
                println!("{},{},{},{}", label, turn_count, wall, accum);
            }
        }
        OutputFormat::Text => {
            println!("{}", "suivi — Summary".bold());
            println!("{}", "─".repeat(53));
            println!();

            for (label, win_since) in &windows {
                let turns = db::query_turns(&conn, win_since.as_deref(), project, agent_filter)?;
                let completed: Vec<_> = turns.iter().filter(|t| t.ended_at.is_some()).collect();
                let wall = wall_clock_secs(&turns, cfg.tracking.human_buffer_secs);
                let accum = accumulated_secs(&turns);

                println!(
                    "  {:<12} wall-clock {:>8}   accumulated {:>8}",
                    label.bold(),
                    format_duration(wall),
                    format_duration(accum)
                );
                let _ = completed; // suppress unused warning
            }
            println!();

            // Top projects (this week) — Gap #3
            let week_since = (now - chrono::Duration::days(7)).to_rfc3339();
            let week_turns =
                db::query_turns(&conn, Some(&week_since), project, agent_filter)?;

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
                let a_acc: f64 = a
                    .1
                    .iter()
                    .filter_map(|&i| week_turns[i].effective_duration_secs)
                    .sum();
                let b_acc: f64 = b
                    .1
                    .iter()
                    .filter_map(|&i| week_turns[i].effective_duration_secs)
                    .sum();
                b_acc.partial_cmp(&a_acc).unwrap_or(std::cmp::Ordering::Equal)
            });

            println!("  {}", "Top projects (this week)".bold());
            println!(
                "  {}",
                "━".repeat(80)
            );
            println!(
                "  {:<16}  {:>10}  {:>11}  {:>5}  {}",
                "Project".bold(),
                "Wall-clock".bold(),
                "Accumulated".bold(),
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
                            let start =
                                chrono::DateTime::parse_from_rfc3339(&t.started_at).ok()?;
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
                    .filter_map(|&i| week_turns[i].effective_duration_secs)
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
                    turn.effective_duration_secs.unwrap_or(0.0);
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
