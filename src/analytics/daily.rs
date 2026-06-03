use anyhow::Result;
use colored::Colorize;
use std::collections::BTreeMap;

use crate::{config, db};

use super::format_duration;

pub fn run(
    since: Option<&str>,
    project: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let conn = db::open()?;
    let turns = db::query_turns(&conn, since, project, agent_filter)?;

    let mut by_day: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, turn) in turns.iter().enumerate() {
        let date = turn.started_at.get(..10).unwrap_or("").to_string();
        if !date.is_empty() {
            by_day.entry(date).or_default().push(i);
        }
    }

    if by_day.is_empty() {
        println!("No daily data found.");
        return Ok(());
    }

    println!("{}", "Daily breakdown".bold());
    println!();
    println!(
        "{:<12}  {:>12}  {:>12}  {:>6}",
        "Date".bold(),
        "Wall-clock".bold(),
        "Accumulated".bold(),
        "Turns".bold()
    );
    println!("{}", "-".repeat(50));

    for (date, indices) in &by_day {
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

        let count = indices
            .iter()
            .filter(|&&i| turns[i].ended_at.is_some())
            .count();

        println!(
            "{:<12}  {:>12}  {:>12}  {:>6}",
            date,
            format_duration(wall),
            format_duration(accum),
            count
        );
    }

    Ok(())
}
