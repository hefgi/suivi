use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::agents::{self, HookDest};
use crate::{config, db, logging};

pub fn run(purge: bool) -> Result<()> {
    println!("{}", "suivi uninstall — removing agent hooks".bold());
    println!();

    let mut removed_any = false;
    let mut seen_paths: Vec<std::path::PathBuf> = Vec::new();

    for agent in agents::all_agents() {
        for file in agent.hook_templates().files {
            let path = match &file.dest {
                HookDest::WriteFile(p) | HookDest::JsonMerge(p) => p.clone(),
            };
            // Claude Code / Codex register two templates against the same
            // settings file; process each destination once.
            if seen_paths.contains(&path) {
                continue;
            }
            seen_paths.push(path.clone());

            let outcome = match &file.dest {
                HookDest::WriteFile(p) => remove_plugin_file(p)?,
                HookDest::JsonMerge(p) => remove_json_hooks(p)?,
            };
            if let Some(desc) = outcome {
                println!("  {:<20} {}", agent.display_name(), desc);
                removed_any = true;
            }
        }
    }
    if !removed_any {
        println!("  No suivi hooks found in any agent config.");
    }

    println!();
    if purge {
        println!("{}", "Purging data".bold());
        purge_data();
    } else {
        println!(
            "Left in place (re-run with {} to remove):",
            "--purge".cyan()
        );
        println!("  Config    {}", config::config_path().display());
        println!("  Database  {}", db::db_path().display());
        println!("  Logs      {}", logging::log_dir().display());
    }

    Ok(())
}

/// Delete a standalone plugin file installed by `suivi init` (Pi/OpenCode JS).
/// Only deletes files that actually reference suivi, so an unrelated file at
/// the expected path is left alone.
fn remove_plugin_file(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path).unwrap_or_default();
    if !content.contains("suivi") {
        return Ok(Some(format!(
            "skipped {} (exists but does not reference suivi)",
            path.display()
        )));
    }
    std::fs::remove_file(path)?;
    Ok(Some(format!("deleted {}", path.display())))
}

/// Strip suivi's entries from a JSON-merged hook file (Claude Code settings,
/// Codex hooks.json), preserving everything else. Returns a description when
/// the file was modified.
fn remove_json_hooks(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)?;
    let mut value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => {
            return Ok(Some(format!(
                "skipped {} (not valid JSON; remove suivi entries manually)",
                path.display()
            )));
        }
    };

    let removed = strip_suivi_hooks(&mut value);
    if removed == 0 {
        return Ok(None);
    }

    write_atomic(path, &serde_json::to_string_pretty(&value)?)?;
    Ok(Some(format!(
        "removed {} hook entr{} from {}",
        removed,
        if removed == 1 { "y" } else { "ies" },
        path.display()
    )))
}

/// Remove every hook entry that invokes `suivi hook` from a settings value.
/// Handles both the flat shape ({"command": ...}) and the nested group shape
/// ({"hooks": [{"command": ...}]}). Event keys whose arrays become empty are
/// dropped. Returns the number of entries removed.
fn strip_suivi_hooks(value: &mut serde_json::Value) -> usize {
    let Some(events) = value.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return 0;
    };

    let mut removed = 0;
    let mut emptied_events: Vec<String> = Vec::new();

    for (event_name, entries) in events.iter_mut() {
        let Some(arr) = entries.as_array_mut() else {
            continue;
        };
        let before = arr.len();
        arr.retain(|h| !mentions_suivi_hook(h));
        removed += before - arr.len();
        if arr.is_empty() && before > 0 {
            emptied_events.push(event_name.clone());
        }
    }
    for event_name in emptied_events {
        events.remove(&event_name);
    }
    removed
}

/// True if a hook entry (flat, or a group with a nested `hooks` array)
/// invokes `suivi hook`.
fn mentions_suivi_hook(h: &serde_json::Value) -> bool {
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
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Delete suivi's own data directories: config, database (including WAL/SHM
/// siblings), and logs. All three live in suivi-specific directories.
fn purge_data() {
    let mut targets: Vec<std::path::PathBuf> = Vec::new();
    if let Some(dir) = config::config_path().parent() {
        targets.push(dir.to_path_buf());
    }
    if let Some(dir) = db::db_path().parent() {
        targets.push(dir.to_path_buf());
    }
    targets.push(logging::log_dir());

    for dir in targets {
        if !dir.exists() {
            continue;
        }
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => println!("  deleted {}", dir.display()),
            Err(e) => println!("  failed to delete {}: {}", dir.display(), e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_strip_suivi_hooks_nested_and_flat() {
        let mut v: serde_json::Value = serde_json::from_str(
            r#"{
            "theme": "dark",
            "hooks": {
                "UserPromptSubmit": [
                    {"hooks": [{"type": "command", "command": "suivi hook pre --agent claude-code"}]},
                    {"hooks": [{"type": "command", "command": "other-tool run"}]}
                ],
                "Stop": [
                    {"type": "command", "command": "suivi hook stop"}
                ],
                "PostToolUse": [
                    {"type": "command", "command": "fmt-on-save"}
                ]
            }
        }"#,
        )
        .unwrap();

        let removed = strip_suivi_hooks(&mut v);
        assert_eq!(removed, 2);
        // Foreign entries and unrelated settings are preserved.
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["hooks"]["UserPromptSubmit"].as_array().unwrap().len(), 1);
        assert_eq!(v["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
        // The emptied Stop event key is dropped.
        assert!(v["hooks"].get("Stop").is_none());
    }

    #[test]
    fn test_strip_suivi_hooks_idempotent() {
        let mut v: serde_json::Value =
            serde_json::from_str(r#"{"hooks": {"Stop": [{"command": "suivi hook stop"}]}}"#)
                .unwrap();
        assert_eq!(strip_suivi_hooks(&mut v), 1);
        assert_eq!(strip_suivi_hooks(&mut v), 0);
    }

    #[test]
    fn test_strip_suivi_hooks_no_hooks_key() {
        let mut v: serde_json::Value = serde_json::from_str(r#"{"theme": "dark"}"#).unwrap();
        assert_eq!(strip_suivi_hooks(&mut v), 0);
    }

    #[test]
    fn test_remove_json_hooks_writes_back_without_suivi() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"theme": "dark", "hooks": {"Stop": [{"hooks": [{"command": "suivi hook stop --agent claude-code"}]}]}}"#,
        )
        .unwrap();

        let outcome = remove_json_hooks(&path).unwrap();
        assert!(outcome.unwrap().contains("removed 1 hook entry"));
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("suivi"));
        assert!(content.contains("dark"));
    }

    #[test]
    fn test_remove_json_hooks_untouched_without_suivi_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        let original = r#"{"hooks": {"Stop": [{"command": "other-tool"}]}}"#;
        std::fs::write(&path, original).unwrap();

        let outcome = remove_json_hooks(&path).unwrap();
        assert!(outcome.is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_remove_json_hooks_missing_file() {
        let dir = TempDir::new().unwrap();
        let outcome = remove_json_hooks(&dir.path().join("absent.json")).unwrap();
        assert!(outcome.is_none());
    }

    #[test]
    fn test_remove_plugin_file_deletes_suivi_plugin() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("suivi.js");
        std::fs::write(&path, "// suivi plugin\n").unwrap();
        let outcome = remove_plugin_file(&path).unwrap();
        assert!(outcome.unwrap().starts_with("deleted"));
        assert!(!path.exists());
    }

    #[test]
    fn test_remove_plugin_file_skips_foreign_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("suivi.js");
        std::fs::write(&path, "// somebody else's file\n").unwrap();
        let outcome = remove_plugin_file(&path).unwrap();
        assert!(outcome.unwrap().starts_with("skipped"));
        assert!(path.exists());
    }
}
