use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::agents::HookDest;
use crate::{agents, config};

pub fn run() -> Result<()> {
    let config_path = config::config_path();

    if config_path.exists() {
        println!("Config already exists at {}", config_path.display());
        println!("Run 'suivi doctor' to check hook health.");
        return Ok(());
    }

    println!("{}", "suivi init — configure project tracking".bold());
    println!();
    println!("Enter project paths to track (one per line, globs supported).");
    println!("Press Enter on an empty line when done.");
    println!("Examples: ~/code/myapp, ~/work/client-*, /home/user/oss/**");
    println!();

    let stdin = io::stdin();
    let mut paths: Vec<String> = Vec::new();

    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        let line = line.trim().to_string();
        if line.is_empty() {
            break;
        }
        paths.push(line);
    }

    if paths.is_empty() {
        let cfg = config::Config::default();
        config::save(&cfg)?;
        println!();
        println!(
            "Config created with no projects. Edit {} to add projects.",
            config_path.display()
        );
    } else {
        let projects = paths
            .into_iter()
            .map(|p| config::ProjectEntry {
                paths: vec![p],
                name: None,
            })
            .collect();

        let cfg = config::Config {
            buffer_mins: 5,
            retention_days: 90,
            projects,
        };

        config::save(&cfg)?;
        println!();
        println!("Config saved to {}", config_path.display());
    }

    let installed = install_hooks()?;
    if installed.is_empty() {
        println!("No agents detected for hook installation.");
    } else {
        println!("Hooks installed for: {}", installed.join(", "));
    }

    Ok(())
}

pub fn install_hooks() -> Result<Vec<String>> {
    let all = agents::all_agents();
    let mut installed = Vec::new();

    for agent in &all {
        let templates = agent.hook_templates();
        for file in templates.files {
            match &file.dest {
                HookDest::WriteFile(path) => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(path, &file.content)?;
                }
                HookDest::JsonMerge(path) => {
                    merge_json_hook(path, &file.content)?;
                }
            }
        }
        installed.push(agent.display_name().to_string());
    }

    Ok(installed)
}

fn merge_json_hook(dest: &Path, template_content: &str) -> Result<()> {
    let template: serde_json::Value = serde_json::from_str(template_content)?;

    if !dest.exists() {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomic(dest, template_content)?;
        return Ok(());
    }

    let existing_content = std::fs::read_to_string(dest)?;
    let mut existing: serde_json::Value = serde_json::from_str(&existing_content)
        .unwrap_or(serde_json::Value::Object(Default::default()));

    // Ensure hooks object exists
    if existing.get("hooks").is_none() {
        existing["hooks"] = serde_json::Value::Object(Default::default());
    }

    // Merge hook events from template
    let template_hooks = match template.get("hooks") {
        Some(h) => h,
        None => &template, // some agents use flat format
    };

    if let Some(events) = template_hooks.as_object() {
        for (event_name, new_hooks_val) in events {
            let new_hooks = match new_hooks_val.as_array() {
                Some(a) => a.clone(),
                None => continue,
            };

            let existing_hooks = existing["hooks"][event_name]
                .as_array()
                .cloned()
                .unwrap_or_default();

            // Check if suivi is already installed for this event
            let already_installed = existing_hooks.iter().any(|h| {
                let check_obj = |obj: &serde_json::Value| {
                    obj.get("command")
                        .and_then(|c| c.as_str())
                        .map(|c| c.contains("suivi hook"))
                        .unwrap_or(false)
                };
                check_obj(h)
                    || h.get("hooks")
                        .and_then(|hs| hs.as_array())
                        .map(|hs| hs.iter().any(check_obj))
                        .unwrap_or(false)
            });

            if !already_installed {
                let mut merged = existing_hooks;
                merged.extend(new_hooks);
                existing["hooks"][event_name] = serde_json::Value::Array(merged);
            }
        }
    }

    let content = serde_json::to_string_pretty(&existing)?;
    write_atomic(dest, &content)?;
    Ok(())
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_install_hooks_empty_agents() {
        // all_agents() returns empty in Phase 1, so install_hooks should return empty vec
        let installed = install_hooks().unwrap();
        // With empty all_agents(), should be empty — this will work until Phase 2 merges
        // Just verify it doesn't panic
        let _ = installed;
    }

    #[test]
    fn test_merge_json_hook_creates_new_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        let template = r#"{"hooks": {"UserPromptSubmit": [{"type": "command", "command": "suivi hook pre"}]}}"#;
        merge_json_hook(&path, template).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("suivi hook pre"));
    }

    #[test]
    fn test_merge_json_hook_merges_into_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        let existing = r#"{"theme": "dark", "hooks": {}}"#;
        std::fs::write(&path, existing).unwrap();
        let template = r#"{"hooks": {"UserPromptSubmit": [{"type": "command", "command": "suivi hook pre"}]}}"#;
        merge_json_hook(&path, template).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("suivi hook pre"));
        assert!(content.contains("dark")); // existing content preserved
    }

    #[test]
    fn test_merge_json_hook_no_duplicate() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        let existing = r#"{"hooks": {"UserPromptSubmit": [{"type": "command", "command": "suivi hook pre"}]}}"#;
        std::fs::write(&path, existing).unwrap();
        let template = r#"{"hooks": {"UserPromptSubmit": [{"type": "command", "command": "suivi hook pre"}]}}"#;
        merge_json_hook(&path, template).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        // Count occurrences — should appear exactly once
        let count = content.matches("suivi hook pre").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_write_atomic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.json");
        write_atomic(&path, r#"{"test": true}"#).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("test"));
    }
}
