use anyhow::Result;
use colored::Colorize;
use std::collections::{HashMap, HashSet};

use crate::{config, db};

use super::format_duration;

pub fn run(since: Option<&str>, agent_filter: Option<&str>) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let conn = db::open()?;
    let turns = db::query_turns(&conn, since, None, agent_filter)?;

    // Group by project name / path / untracked
    let mut by_project: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, turn) in turns.iter().enumerate() {
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
    // Sort by accumulated time descending
    projects.sort_by(|a, b| {
        let a_acc: f64 = a
            .1
            .iter()
            .filter_map(|&i| {
                let t = &turns[i];
                if t.ended_at.is_some() {
                    t.effective_duration_secs
                } else {
                    None
                }
            })
            .sum();
        let b_acc: f64 = b
            .1
            .iter()
            .filter_map(|&i| {
                let t = &turns[i];
                if t.ended_at.is_some() {
                    t.effective_duration_secs
                } else {
                    None
                }
            })
            .sum();
        b_acc
            .partial_cmp(&a_acc)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("{}", "Projects".bold());
    println!();

    let name_w = 24usize;
    let time_w = 12usize;
    println!(
        "{:<width$}  {:>tw$}  {:>tw$}  {}",
        "Project".bold(),
        "Wall-clock".bold(),
        "Accumulated".bold(),
        "Sessions".bold(),
        width = name_w,
        tw = time_w
    );
    println!("{}", "-".repeat(name_w + time_w * 2 + 20));

    for (name, indices) in &projects {
        // Compute wall-clock inline (avoid Clone requirement)
        let wall: f64 = {
            use chrono::Duration;
            let buffer = Duration::minutes(cfg.buffer_mins as i64);
            let intervals: Vec<_> = indices
                .iter()
                .filter_map(|&i| {
                    let t = &turns[i];
                    if t.ended_at.is_none() {
                        return None;
                    }
                    let start =
                        chrono::DateTime::parse_from_rfc3339(&t.started_at).ok()?;
                    let end = chrono::DateTime::parse_from_rfc3339(t.ended_at.as_ref()?).ok()?;
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
            .filter_map(|&i| {
                let t = &turns[i];
                if t.ended_at.is_some() {
                    t.effective_duration_secs
                } else {
                    None
                }
            })
            .sum();

        let total_sessions: usize = indices
            .iter()
            .map(|&i| turns[i].session_id.as_str())
            .collect::<HashSet<_>>()
            .len();

        let sessions_str = format!("×{}", total_sessions);

        let display_name = if name.len() > name_w {
            format!("{}…", &name[..name_w - 1])
        } else {
            name.clone()
        };

        println!(
            "{:<width$}  {:>tw$}  {:>tw$}  {}",
            display_name,
            format_duration(wall),
            format_duration(accum),
            sessions_str,
            width = name_w,
            tw = time_w
        );
    }

    Ok(())
}
