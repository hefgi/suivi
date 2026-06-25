use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

use crate::{config, db};

use super::{format_duration, sessions_column};

pub fn run(
    since: Option<&str>,
    until: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let conn = db::open()?;
    let turns = db::query_turns(&conn, since, until, None, agent_filter)?;

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
    projects.sort_by(|a, b| {
        let a_acc: f64 =
            a.1.iter()
                .filter_map(|&i| {
                    let t = &turns[i];
                    if t.ended_at.is_some() {
                        t.agent_duration_secs
                    } else {
                        None
                    }
                })
                .sum();
        let b_acc: f64 =
            b.1.iter()
                .filter_map(|&i| {
                    let t = &turns[i];
                    if t.ended_at.is_some() {
                        t.agent_duration_secs
                    } else {
                        None
                    }
                })
                .sum();
        b_acc
            .partial_cmp(&a_acc)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("{}", "suivi — Project comparison".bold());
    println!("{}", "─".repeat(90));

    let name_w = 20usize;
    let time_w = 11usize;
    println!(
        "{:<nw$}  {:>tw$}  {:>tw$}  {:>5}  {}",
        "Project".bold(),
        "Wall-clock".bold(),
        "Agent time".bold(),
        "Turns".bold(),
        "Sessions".bold(),
        nw = name_w,
        tw = time_w
    );
    println!("{}", "─".repeat(90));

    let mut total_wall = 0.0f64;
    let mut total_accum = 0.0f64;
    let mut total_turns = 0usize;

    for (name, indices) in &projects {
        let wall: f64 = {
            use chrono::Duration;
            let buffer = Duration::seconds(cfg.tracking.human_buffer_secs as i64);
            let intervals: Vec<_> = indices
                .iter()
                .filter_map(|&i| {
                    let t = &turns[i];
                    t.ended_at.as_ref()?;
                    let start = chrono::DateTime::parse_from_rfc3339(&t.started_at).ok()?;
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
                    t.agent_duration_secs
                } else {
                    None
                }
            })
            .sum();

        let turn_count = indices
            .iter()
            .filter(|&&i| turns[i].ended_at.is_some())
            .count();

        let sessions_str = sessions_column(&turns, indices);

        let display_name = if name.len() > name_w {
            format!("{}…", &name[..name_w - 1])
        } else {
            name.clone()
        };

        println!(
            "{:<nw$}  {:>tw$}  {:>tw$}  {:>5}  {}",
            display_name,
            format_duration(wall),
            format_duration(accum),
            turn_count,
            sessions_str,
            nw = name_w,
            tw = time_w
        );

        total_wall += wall;
        total_accum += accum;
        total_turns += turn_count;
    }

    println!("{}", "─".repeat(90));
    println!(
        "{:<nw$}  {:>tw$}  {:>tw$}  {:>5}",
        "Total".bold(),
        format_duration(total_wall).bold(),
        format_duration(total_accum).bold(),
        total_turns,
        nw = name_w,
        tw = time_w
    );

    Ok(())
}
