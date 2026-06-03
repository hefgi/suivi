use anyhow::Result;
use colored::Colorize;
use std::collections::BTreeMap;

use crate::db;

pub fn run(
    since: Option<&str>,
    project: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<()> {
    let conn = db::open()?;
    let turns = db::query_turns(&conn, since, project, agent_filter)?;

    // Group by date
    let mut by_day: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, turn) in turns.iter().enumerate() {
        let date = turn.started_at.get(..10).unwrap_or("").to_string();
        if !date.is_empty() {
            by_day.entry(date).or_default().push(i);
        }
    }

    if by_day.is_empty() {
        println!("No activity to graph.");
        return Ok(());
    }

    println!("{}", "Activity graph".bold());
    println!();

    // Find max for scaling
    let max_secs: f64 = by_day
        .values()
        .map(|indices| {
            indices
                .iter()
                .filter_map(|&i| {
                    let t = &turns[i];
                    if t.ended_at.is_some() {
                        t.effective_duration_secs
                    } else {
                        None
                    }
                })
                .sum::<f64>()
        })
        .fold(0.0_f64, f64::max);

    let bar_width = 30usize;

    for (date, indices) in &by_day {
        let secs: f64 = indices
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
        let filled = if max_secs > 0.0 {
            ((secs / max_secs) * bar_width as f64).round() as usize
        } else {
            0
        };
        let bar = "█".repeat(filled) + &"░".repeat(bar_width - filled);
        let dur = super::format_duration(secs);
        println!("  {}  {}  {}", date.dimmed(), bar.green(), dur);
    }

    Ok(())
}
