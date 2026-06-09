use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

use crate::agents::HookDest;
use crate::{agents, config, db};

pub fn run() -> Result<()> {
    let cfg = config::load().unwrap_or_default();

    println!("{}", "suivi — Status".bold());
    println!("{}", "─".repeat(42));
    println!();

    // Config/DB paths + project count
    let config_path = config::config_path();
    let db_path = db::db_path();
    let project_count: usize = cfg
        .projects
        .iter()
        .map(|e| config::expand_globs(&e.path).len())
        .sum();

    println!("  {:<10} {}", "Config".dimmed(), config_path.display());
    println!("  {:<10} {}", "Database".dimmed(), db_path.display());
    println!(
        "  {:<10} {} tracked",
        "Projects".dimmed(),
        project_count
    );
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

    // Untracked activity (last 7 days)
    println!("{}", "Untracked activity (last 7 days)".underline());
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
    let untracked: Vec<_> = turns
        .iter()
        .filter(|t| t.ended_at.is_some() && t.project_path.is_none())
        .collect();

    if untracked.is_empty() {
        println!("  No untracked turns in the last 7 days.");
    } else {
        println!("  {} turns not attributed to any project", untracked.len());

        let mut by_cwd: HashMap<&str, usize> = HashMap::new();
        for t in &untracked {
            *by_cwd.entry(t.cwd.as_str()).or_default() += 1;
        }
        let mut cwd_counts: Vec<(&str, usize)> = by_cwd.into_iter().collect();
        cwd_counts.sort_by(|a, b| b.1.cmp(&a.1));

        println!("  Top untracked paths:");
        for (cwd, count) in cwd_counts.iter().take(5) {
            println!("    {:<40} {} turns", cwd, count);
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
