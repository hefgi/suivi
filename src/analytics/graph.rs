use anyhow::Result;
use colored::Colorize;
use std::collections::BTreeMap;

use crate::config;
use crate::db::{self, TurnRow};

use super::{agent_secs, format_duration, wall_clock_secs};

const BAR_WIDTH: usize = 20;

/// Pure renderer for the daily activity graph.
/// `days` is a sorted-ascending list of (YYYY-MM-DD, wall_clock_secs, agent_secs).
pub fn render(days: &[(String, f64, f64)]) -> String {
    let mut out = String::new();
    if days.is_empty() {
        out.push_str("No activity to graph.\n");
        return out;
    }

    out.push_str(&format!("{}\n", "Daily activity — last 30 days".bold()));
    out.push('\n');

    let max_secs = days
        .iter()
        .flat_map(|(_, w, a)| [*w, *a])
        .fold(0.0_f64, f64::max);

    let bar = |secs: f64| -> String {
        let filled = if max_secs > 0.0 {
            ((secs / max_secs) * BAR_WIDTH as f64).round() as usize
        } else {
            0
        };
        let filled = filled.min(BAR_WIDTH);
        "█".repeat(filled) + &"░".repeat(BAR_WIDTH - filled)
    };

    for (date, wall, agent) in days {
        let label = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map(|d| d.format("%b %d").to_string())
            .unwrap_or_else(|_| date.clone());

        out.push_str(&format!(
            "  {}  wall-clock  {}  {}\n",
            label.dimmed(),
            bar(*wall).green(),
            format_duration(*wall),
        ));
        out.push_str(&format!(
            "          agent time  {}  {}\n",
            bar(*agent).green(),
            format_duration(*agent),
        ));
    }

    out
}

pub fn run(since: Option<&str>, project: Option<&str>, agent_filter: Option<&str>) -> Result<()> {
    let conn = db::open()?;
    let turns = db::query_turns(&conn, since, None, project, agent_filter)?;
    let cfg = config::load().unwrap_or_default();
    let buffer = cfg.tracking.human_buffer_secs;

    let mut by_day: BTreeMap<String, Vec<TurnRow>> = BTreeMap::new();
    for turn in &turns {
        if let Some(date) = super::local_day_key(&turn.started_at) {
            by_day.entry(date).or_default().push(turn.clone());
        }
    }

    let days: Vec<(String, f64, f64)> = by_day
        .into_iter()
        .map(|(date, day_turns)| {
            let wall = wall_clock_secs(&day_turns, buffer);
            let agent = agent_secs(&day_turns);
            (date, wall, agent)
        })
        .collect();

    print!("{}", render(&days));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_empty() {
        let out = render(&[]);
        assert!(out.contains("No activity to graph."));
    }

    #[test]
    fn test_render_two_days_two_lines_each() {
        let days = vec![
            ("2024-06-01".to_string(), 4.0 * 3600.0, 6.0 * 3600.0),
            ("2024-06-02".to_string(), 2.0 * 3600.0, 2.0 * 3600.0),
        ];
        let out = render(&days);
        // Strip ANSI codes for substring assertions.
        let plain: String = out
            .chars()
            .scan(false, |in_esc, c| {
                if *in_esc {
                    if c == 'm' {
                        *in_esc = false;
                    }
                    Some(None)
                } else if c == '\u{1b}' {
                    *in_esc = true;
                    Some(None)
                } else {
                    Some(Some(c))
                }
            })
            .flatten()
            .collect();

        assert!(plain.contains("Daily activity — last 30 days"));
        assert!(plain.contains("Jun 01  wall-clock"));
        assert!(plain.contains("          agent time"));
        assert!(plain.contains("Jun 02  wall-clock"));
        assert!(plain.contains("4h 00m"));
        assert!(plain.contains("6h 00m"));
        assert!(plain.contains("2h 00m"));

        // Two lines per day → 4 data lines + header + blank line = 6 newlines minimum.
        let line_count = plain.lines().count();
        assert!(line_count >= 6, "got {} lines: {:?}", line_count, plain);
    }

    #[test]
    fn test_render_shared_scale_max_bar_full_width() {
        // The largest value across all days/series should fill the bar.
        let days = vec![("2024-06-01".to_string(), 1.0 * 3600.0, 10.0 * 3600.0)];
        let out = render(&days);
        // agent time has max value → its bar should be all full blocks.
        let full = "█".repeat(BAR_WIDTH);
        assert!(out.contains(&full));
    }
}
