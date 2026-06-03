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
        Self { vars, parent_process_name }
    }

    fn detect_parent() -> Option<String> {
        let ppid = std::env::var("PPID").ok()?;
        let output = std::process::Command::new("ps")
            .args(["-p", &ppid, "-o", "comm="])
            .output()
            .ok()?;
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if name.is_empty() { None } else { Some(name) }
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
    vec![]
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
}
