pub mod claude_code;
pub mod codex;
pub mod opencode;
pub mod pi;

use std::collections::HashMap;
use std::path::PathBuf;

pub struct Env {
    pub vars: HashMap<String, String>,
    pub parent_process_name: Option<String>,
}

impl Env {
    pub fn capture() -> Self {
        let vars: HashMap<String, String> = std::env::vars().collect();
        let parent_process_name = Self::detect_parent();
        Self {
            vars,
            parent_process_name,
        }
    }

    fn detect_parent() -> Option<String> {
        // The shell-set $PPID is unavailable when Claude Code (or any agent) spawns
        // `suivi hook pre|stop` directly without going through a shell. Use the
        // actual kernel-reported parent pid via libc::getppid().
        // SAFETY: getppid is always safe; it returns the calling process's parent pid.
        let ppid = unsafe { libc::getppid() };
        if ppid <= 1 {
            return None;
        }
        let output = std::process::Command::new("ps")
            .args(["-p", &ppid.to_string(), "-o", "comm="])
            .output()
            .ok()?;
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if name.is_empty() {
            None
        } else {
            // ps may return the full executable path; we just want the basename.
            let basename = std::path::Path::new(&name)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or(name);
            Some(basename)
        }
    }
}

pub struct AgentPayload {
    pub session_id: String,
    pub cwd: String,
    pub model: Option<String>,
}

pub enum HookDest {
    /// Merge into existing JSON file (Claude Code settings, Codex hooks.json)
    JsonMerge(PathBuf),
    /// Write standalone file (Pi/OpenCode JS plugins)
    WriteFile(PathBuf),
}

pub struct HookFile {
    pub dest: HookDest,
    #[allow(dead_code)]
    pub filename: String,
    pub content: String,
}

pub struct HookTemplates {
    pub files: Vec<HookFile>,
}

pub trait Agent: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn detect(&self, env: &Env) -> bool;
    fn parse_payload(&self, raw: &str) -> Option<AgentPayload>;
    fn hook_templates(&self) -> HookTemplates;
}

/// Returns all supported agents in detection priority order.
/// Detection order: ClaudeCode → Codex → OpenCode → Pi
pub fn all_agents() -> Vec<Box<dyn Agent>> {
    vec![
        Box::new(claude_code::ClaudeCodeAgent),
        Box::new(codex::CodexAgent),
        Box::new(opencode::OpenCodeAgent),
        Box::new(pi::PiAgent),
    ]
}

/// Look up an agent by its stable id (e.g. "claude-code").
pub fn find_by_id(id: &str) -> Option<Box<dyn Agent>> {
    all_agents().into_iter().find(|a| a.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_capture() {
        let env = Env::capture();
        // Should have at least PATH or HOME
        assert!(!env.vars.is_empty());
    }

    #[test]
    fn test_detect_parent_no_panic() {
        // Just verify it doesn't panic regardless of PPID availability
        let _ = Env::detect_parent();
    }

    #[test]
    fn test_find_by_id() {
        assert_eq!(
            find_by_id("claude-code").map(|a| a.id()),
            Some("claude-code")
        );
        assert_eq!(find_by_id("codex").map(|a| a.id()), Some("codex"));
        assert_eq!(find_by_id("opencode").map(|a| a.id()), Some("opencode"));
        assert_eq!(find_by_id("pi").map(|a| a.id()), Some("pi"));
        assert!(find_by_id("unknown-agent").is_none());
        assert!(find_by_id("Claude Code").is_none()); // display names don't resolve
    }
}
