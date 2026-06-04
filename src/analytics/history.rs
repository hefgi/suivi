use anyhow::Result;
use colored::Colorize;

use crate::cli::OutputFormat;
use crate::db;

use super::format_duration;

pub fn run(
    since: Option<&str>,
    project: Option<&str>,
    agent_filter: Option<&str>,
    format: &OutputFormat,
) -> Result<()> {
    let conn = db::open()?;
    let turns = db::query_turns(&conn, since, project, agent_filter)?;

    match format {
        OutputFormat::Json => {
            let entries: Vec<serde_json::Value> = turns
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "started_at": t.started_at,
                        "ended_at": t.ended_at,
                        "agent": t.agent,
                        "model": t.model,
                        "project_name": t.project_name,
                        "project_path": t.project_path,
                        "effective_duration_secs": t.effective_duration_secs,
                        "session_id": t.session_id,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&entries)?);
        }
        OutputFormat::Csv => {
            println!(
                "started_at,ended_at,agent,model,project_name,effective_duration_secs,session_id"
            );
            for t in &turns {
                println!(
                    "{},{},{},{},{},{},{}",
                    t.started_at,
                    t.ended_at.as_deref().unwrap_or(""),
                    t.agent,
                    t.model.as_deref().unwrap_or(""),
                    t.project_name.as_deref().unwrap_or(""),
                    t.effective_duration_secs
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    t.session_id,
                );
            }
        }
        OutputFormat::Text => {
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
                            std::path::Path::new(p).file_name().and_then(|n| n.to_str())
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
        }
    }

    Ok(())
}
