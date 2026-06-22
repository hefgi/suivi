use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

use crate::{config, db};

use super::{agent_secs, format_duration, graph_label_for, wall_clock_secs};

pub fn run(project_name: &str, since: Option<&str>, has_time_flag: bool) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let conn = db::open()?;
    let now = chrono::Utc::now();

    // Resolve the project_path that matches the given name/path fragment
    let all_turns = db::query_turns(&conn, None, None, None)?;
    let matched_path = resolve_project_path(&all_turns, project_name);

    println!("{} — Project view", project_name.bold());
    println!("{}", "─".repeat(53));
    println!();

    // Time windows
    let windows: Vec<(&str, Option<String>)> = if has_time_flag {
        vec![("Custom range", since.map(|s| s.to_string()))]
    } else {
        let today_start = super::local_today_start_rfc3339();
        let week_start = (now - chrono::Duration::days(7)).to_rfc3339();
        vec![
            ("Today", today_start),
            ("This week", Some(week_start)),
            ("All time", None),
        ]
    };

    for (label, win_since) in &windows {
        let turns = db::query_turns(&conn, win_since.as_deref(), matched_path.as_deref(), None)?;
        let turns = filter_by_name(&turns, project_name, matched_path.as_deref());
        let wall = wall_clock_secs(&turns, cfg.tracking.human_buffer_secs);
        let agent = agent_secs(&turns);
        println!(
            "  {:<12} wall-clock {:>8}   agent time {:>8}",
            label.bold(),
            format_duration(wall),
            format_duration(agent)
        );
    }
    println!();

    // Activity graph — follows the active flag's window when one is set,
    // otherwise defaults to a 30-day rolling view.
    let (graph_label, graph_since) = match (has_time_flag, since) {
        (true, Some(s)) => (graph_label_for(s, now), Some(s.to_string())),
        (true, None) => ("all time".to_string(), None),
        (false, _) => (
            "last 30 days".to_string(),
            Some((now - chrono::Duration::days(30)).to_rfc3339()),
        ),
    };
    let graph_turns =
        db::query_turns(&conn, graph_since.as_deref(), matched_path.as_deref(), None)?;
    let graph_turns = filter_by_name(&graph_turns, project_name, matched_path.as_deref());
    if !graph_turns.is_empty() {
        println!("  {}", format!("Activity ({})", graph_label).bold());
        render_mini_graph(&graph_turns, &cfg);
        println!();
    }

    // Agent breakdown
    let base_turns = db::query_turns(&conn, since, matched_path.as_deref(), None)?;
    let base_turns = filter_by_name(&base_turns, project_name, matched_path.as_deref());

    let mut by_agent: HashMap<String, f64> = HashMap::new();
    for t in &base_turns {
        if t.ended_at.is_none() {
            continue;
        }
        *by_agent.entry(t.agent.clone()).or_default() += t.agent_duration_secs.unwrap_or(0.0);
    }

    if !by_agent.is_empty() {
        let total: f64 = by_agent.values().sum();
        let mut agents: Vec<(String, f64)> = by_agent.into_iter().collect();
        agents.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        println!("  {}", "Agent breakdown".bold());
        let bar_width = 16usize;
        for (agent, secs) in &agents {
            let pct = if total > 0.0 {
                (secs / total * 100.0).round() as usize
            } else {
                0
            };
            let filled = if total > 0.0 {
                (secs / total * bar_width as f64).round() as usize
            } else {
                0
            }
            .min(bar_width);
            let bar = "█".repeat(filled) + &"░".repeat(bar_width - filled);
            println!(
                "  {:<20}  {:>8}  {}  {}%",
                agent,
                format_duration(*secs),
                bar,
                pct
            );
        }
        println!();
    }

    // Model breakdown
    let mut by_model: HashMap<String, f64> = HashMap::new();
    for t in &base_turns {
        if t.ended_at.is_none() {
            continue;
        }
        let model = t.model.clone().unwrap_or_else(|| "(unknown)".to_string());
        *by_model.entry(model).or_default() += t.agent_duration_secs.unwrap_or(0.0);
    }

    if !by_model.is_empty() {
        let mut models: Vec<(String, f64)> = by_model.into_iter().collect();
        models.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        println!("  {}", "Model breakdown".bold());
        for (model, secs) in &models {
            println!("  {:<30}  {:>8}", model, format_duration(*secs));
        }
    }

    Ok(())
}

fn resolve_project_path(turns: &[crate::db::TurnRow], name: &str) -> Option<String> {
    for t in turns {
        if let Some(pname) = &t.project_name {
            if pname.eq_ignore_ascii_case(name) {
                return t.project_path.clone();
            }
        }
        if let Some(ppath) = &t.project_path {
            if std::path::Path::new(ppath)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case(name))
                .unwrap_or(false)
            {
                return Some(ppath.clone());
            }
        }
    }
    None
}

fn filter_by_name(
    turns: &[crate::db::TurnRow],
    name: &str,
    matched_path: Option<&str>,
) -> Vec<crate::db::TurnRow> {
    turns
        .iter()
        .filter(|t| {
            if let Some(mp) = matched_path {
                t.project_path.as_deref() == Some(mp)
            } else {
                t.project_name
                    .as_deref()
                    .map(|n| n.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
            }
        })
        .cloned()
        .collect()
}

fn render_mini_graph(turns: &[crate::db::TurnRow], cfg: &config::Config) {
    let by_day = super::compute_daily_contributions(
        turns,
        &chrono::Local,
        cfg.tracking.human_buffer_secs,
    );

    let max_secs: f64 = by_day
        .values()
        .map(|(_, agent)| *agent)
        .fold(0.0_f64, f64::max);

    let bar_width = 24usize;
    for (date, (wall, agent)) in &by_day {
        let wall_filled = if max_secs > 0.0 {
            (wall / max_secs * bar_width as f64).round() as usize
        } else {
            0
        }
        .min(bar_width);
        let agent_filled = if max_secs > 0.0 {
            (agent / max_secs * bar_width as f64).round() as usize
        } else {
            0
        }
        .min(bar_width);

        println!(
            "  {}  wall-clock  {}  {}",
            date.dimmed(),
            format!(
                "{}{}",
                "█".repeat(wall_filled),
                "░".repeat(bar_width - wall_filled)
            )
            .green(),
            format_duration(*wall)
        );
        println!(
            "  {}  agent time  {}  {}",
            " ".repeat(10),
            format!(
                "{}{}",
                "█".repeat(agent_filled),
                "░".repeat(bar_width - agent_filled)
            )
            .cyan(),
            format_duration(*agent)
        );
    }
}
