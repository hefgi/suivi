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
    println!("  {:<10} {} tracked", "Projects".dimmed(), project_count);
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
        cwd_counts.sort_by_key(|b| std::cmp::Reverse(b.1));

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
                    Ok(content) => match json_hook_health(&content, &file.content) {
                        HookHealth::Ok => {}
                        other => return other,
                    },
                    Err(_) => return HookHealth::Missing,
                }
            }
        }
    }
    HookHealth::Ok
}

/// Health of a JSON-merged hook file: Ok when every command in the template
/// is present verbatim, Outdated when suivi hooks exist but with a different
/// command (e.g. an install predating the --agent flag), Missing otherwise.
fn json_hook_health(existing: &str, template: &str) -> HookHealth {
    let commands = template_commands(template);
    if !commands.is_empty() && commands.iter().all(|c| existing.contains(c.as_str())) {
        return HookHealth::Ok;
    }
    if existing.contains("suivi hook") {
        return HookHealth::Outdated;
    }
    HookHealth::Missing
}

/// Collect every "command" string nested anywhere in a hook template.
fn template_commands(template: &str) -> Vec<String> {
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(map) => {
                if let Some(cmd) = map.get("command").and_then(|c| c.as_str()) {
                    out.push(cmd.to_string());
                }
                for val in map.values() {
                    walk(val, out);
                }
            }
            serde_json::Value::Array(arr) => {
                for val in arr {
                    walk(val, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(template) {
        walk(&v, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = r#"{"hooks": {"UserPromptSubmit": [{"hooks": [{"type": "command", "command": "suivi hook pre --agent claude-code"}]}]}}"#;

    #[test]
    fn test_json_hook_health_ok() {
        let existing = r#"{"hooks": {"UserPromptSubmit": [{"hooks": [{"command": "suivi hook pre --agent claude-code"}]}]}}"#;
        assert!(matches!(
            json_hook_health(existing, TEMPLATE),
            HookHealth::Ok
        ));
    }

    #[test]
    fn test_json_hook_health_outdated_old_command() {
        let existing =
            r#"{"hooks": {"UserPromptSubmit": [{"hooks": [{"command": "suivi hook pre"}]}]}}"#;
        assert!(matches!(
            json_hook_health(existing, TEMPLATE),
            HookHealth::Outdated
        ));
    }

    #[test]
    fn test_json_hook_health_missing() {
        let existing = r#"{"theme": "dark"}"#;
        assert!(matches!(
            json_hook_health(existing, TEMPLATE),
            HookHealth::Missing
        ));
    }

    #[test]
    fn test_template_commands_extracts_nested() {
        let cmds = template_commands(TEMPLATE);
        assert_eq!(cmds, vec!["suivi hook pre --agent claude-code"]);
    }
}
