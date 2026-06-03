use anyhow::Result;
use colored::Colorize;

use crate::db;

use super::format_duration;

pub fn run(
    since: Option<&str>,
    project: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<()> {
    let conn = db::open()?;
    let turns = db::query_turns(&conn, since, project, agent_filter)?;

    if turns.is_empty() {
        println!("No turns found.");
        return Ok(());
    }

    println!("{}", "History".bold());
    println!();

    for turn in &turns {
        let started = chrono::DateTime::parse_from_rfc3339(&turn.started_at)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|_| turn.started_at.clone());

        let duration = turn
            .effective_duration_secs
            .map(format_duration)
            .unwrap_or_else(|| "(open)".to_string());

        let project_label = turn
            .project_name
            .as_deref()
            .or_else(|| {
                turn.project_path.as_ref().and_then(|p| {
                    std::path::Path::new(p)
                        .file_name()
                        .and_then(|n| n.to_str())
                })
            })
            .unwrap_or("(untracked)");

        let model_label = turn.model.as_deref().unwrap_or("-");

        println!(
            "  {}  {:<20}  {:<12}  {:<20}  {}",
            started.dimmed(),
            turn.agent.cyan(),
            duration,
            project_label,
            model_label.dimmed(),
        );
    }

    Ok(())
}
