use anyhow::Result;
use colored::Colorize;

use crate::agents::HookDest;
use crate::{agents, db};

pub fn run() -> Result<()> {
    println!("{}", "suivi status".bold());
    println!();

    // Hook health
    println!("{}", "Hooks".underline());
    let all = agents::all_agents();
    if all.is_empty() {
        println!("  No agents registered.");
    } else {
        for agent in &all {
            let templates = agent.hook_templates();
            let health = check_hook_health(&templates);
            let status_str = match health {
                HookHealth::Ok => "Ok".green().to_string(),
                HookHealth::Missing => "Missing".red().to_string(),
                HookHealth::Outdated => "Outdated".yellow().to_string(),
            };
            if agent.id() == "pi" {
                println!(
                    "  {:<20} {}  (experimental)",
                    agent.display_name(),
                    status_str
                );
            } else {
                println!("  {:<20} {}", agent.display_name(), status_str);
            }
        }
    }

    println!();

    // Recent untracked activity (last 7 days)
    println!("{}", "Recent activity".underline());
    let conn = match db::open() {
        Ok(c) => c,
        Err(_) => {
            println!("  (database not accessible)");
            return Ok(());
        }
    };

    let since = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(7))
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();

    let turns = db::query_turns(&conn, Some(&since), None, None).unwrap_or_default();
    let completed: Vec<_> = turns.iter().filter(|t| t.ended_at.is_some()).collect();
    if completed.is_empty() {
        println!("  No completed turns in the last 7 days.");
    } else {
        println!("  {} turns recorded in the last 7 days.", completed.len());
        // Show count per agent (completed only)
        let mut by_agent: std::collections::HashMap<&str, usize> = Default::default();
        for t in &completed {
            *by_agent.entry(t.agent.as_str()).or_default() += 1;
        }
        let mut agent_counts: Vec<_> = by_agent.iter().collect();
        agent_counts.sort_by_key(|(a, _)| *a);
        for (agent, count) in agent_counts {
            println!("    {:<20} {} turns", agent, count);
        }
    }

    Ok(())
}

enum HookHealth {
    Ok,
    Missing,
    Outdated,
}

fn check_hook_health(templates: &crate::agents::HookTemplates) -> HookHealth {
    for file in &templates.files {
        match &file.dest {
            HookDest::WriteFile(path) => {
                if !path.exists() {
                    return HookHealth::Missing;
                }
                // Check if content matches
                match std::fs::read_to_string(path) {
                    Ok(existing) if existing == file.content => {}
                    Ok(_) => return HookHealth::Outdated,
                    Err(_) => return HookHealth::Missing,
                }
            }
            HookDest::JsonMerge(path) => {
                if !path.exists() {
                    return HookHealth::Missing;
                }
                // Check if suivi command is present in the file
                match std::fs::read_to_string(path) {
                    Ok(content) if content.contains("suivi hook") => {}
                    Ok(_) => return HookHealth::Missing,
                    Err(_) => return HookHealth::Missing,
                }
            }
        }
    }
    HookHealth::Ok
}
