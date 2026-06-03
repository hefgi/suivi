use anyhow::Result;
use colored::Colorize;

use crate::cli::OutputFormat;
use crate::{config, db};

use super::{accumulated_secs, format_duration, wall_clock_secs};

pub fn run(
    all_time: bool,
    project: Option<&str>,
    agent_filter: Option<&str>,
    format: &OutputFormat,
) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let conn = db::open()?;

    let now = chrono::Utc::now();

    // Define time windows
    let windows: Vec<(&str, Option<String>)> = if all_time {
        vec![("All time", None)]
    } else {
        let today_start = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|dt| {
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc)
            })
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
            let mut entries = Vec::new();
            for (label, since) in &windows {
                let turns = db::query_turns(&conn, since.as_deref(), project, agent_filter)?;
                let turn_count = turns.iter().filter(|t| t.ended_at.is_some()).count();
                let wall = wall_clock_secs(&turns, cfg.buffer_mins);
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
        OutputFormat::Csv => {
            println!("window,turns,wall_clock_secs,accumulated_secs");
            for (label, since) in &windows {
                let turns = db::query_turns(&conn, since.as_deref(), project, agent_filter)?;
                let turn_count = turns.iter().filter(|t| t.ended_at.is_some()).count();
                let wall = wall_clock_secs(&turns, cfg.buffer_mins);
                let accum = accumulated_secs(&turns);
                println!("{},{},{},{}", label, turn_count, wall, accum);
            }
        }
        OutputFormat::Text => {
            println!("{}", "suivi stats".bold());
            println!();
            for (label, since) in &windows {
                let turns = db::query_turns(&conn, since.as_deref(), project, agent_filter)?;
                let completed: Vec<_> = turns.iter().filter(|t| t.ended_at.is_some()).collect();
                let turn_count = completed.len();
                let wall = wall_clock_secs(&turns, cfg.buffer_mins);
                let accum = accumulated_secs(&turns);

                println!("{}", label.bold());
                println!("  Turns:       {}", turn_count);
                println!("  Wall-clock:  {}", format_duration(wall));
                println!("  Accumulated: {}", format_duration(accum));
                println!();
            }
        }
    }

    Ok(())
}
