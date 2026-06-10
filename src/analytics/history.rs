use anyhow::Result;
use colored::Colorize;

use crate::cli::OutputFormat;
use crate::db::{self, TurnRow};

use super::format_duration;

pub fn turns_to_json(turns: &[TurnRow]) -> Result<String> {
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
    Ok(serde_json::to_string_pretty(&entries)?)
}

pub fn turns_to_csv(turns: &[TurnRow]) -> String {
    let mut out = String::new();
    out.push_str(
        "started_at,ended_at,agent,model,project_name,effective_duration_secs,session_id\n",
    );
    for t in turns {
        out.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            t.started_at,
            t.ended_at.as_deref().unwrap_or(""),
            t.agent,
            t.model.as_deref().unwrap_or(""),
            t.project_name.as_deref().unwrap_or(""),
            t.effective_duration_secs
                .map(|s| s.to_string())
                .unwrap_or_default(),
            t.session_id,
        ));
    }
    out
}

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
            println!("{}", turns_to_json(&turns)?);
        }
        OutputFormat::Csv => {
            // strip trailing newline so println! adds exactly one
            print!("{}", turns_to_csv(&turns));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<TurnRow> {
        vec![
            TurnRow {
                id: 1,
                session_id: "s1".to_string(),
                started_at: "2024-06-01T10:00:00Z".to_string(),
                ended_at: Some("2024-06-01T10:05:00Z".to_string()),
                project_path: Some("/proj/a".to_string()),
                project_name: Some("a".to_string()),
                cwd: "/proj/a".to_string(),
                agent: "claude-code".to_string(),
                model: Some("sonnet".to_string()),
                agent_duration_secs: Some(30.0),
                effective_duration_secs: Some(630.0),
            },
            TurnRow {
                id: 2,
                session_id: "s2".to_string(),
                started_at: "2024-06-01T11:00:00Z".to_string(),
                ended_at: None,
                project_path: None,
                project_name: None,
                cwd: "/scratch".to_string(),
                agent: "codex".to_string(),
                model: None,
                agent_duration_secs: None,
                effective_duration_secs: None,
            },
        ]
    }

    #[test]
    fn test_turns_to_json_snapshot() {
        let out = turns_to_json(&fixture()).unwrap();
        let expected = r#"[
  {
    "agent": "claude-code",
    "effective_duration_secs": 630.0,
    "ended_at": "2024-06-01T10:05:00Z",
    "model": "sonnet",
    "project_name": "a",
    "project_path": "/proj/a",
    "session_id": "s1",
    "started_at": "2024-06-01T10:00:00Z"
  },
  {
    "agent": "codex",
    "effective_duration_secs": null,
    "ended_at": null,
    "model": null,
    "project_name": null,
    "project_path": null,
    "session_id": "s2",
    "started_at": "2024-06-01T11:00:00Z"
  }
]"#;
        assert_eq!(out, expected);
    }

    #[test]
    fn test_turns_to_csv_snapshot() {
        let out = turns_to_csv(&fixture());
        let expected =
            "started_at,ended_at,agent,model,project_name,effective_duration_secs,session_id\n\
2024-06-01T10:00:00Z,2024-06-01T10:05:00Z,claude-code,sonnet,a,630,s1\n\
2024-06-01T11:00:00Z,,codex,,,,s2\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn test_turns_to_csv_empty() {
        let out = turns_to_csv(&[]);
        assert_eq!(
            out,
            "started_at,ended_at,agent,model,project_name,effective_duration_secs,session_id\n"
        );
    }
}
