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
    /// Whether this agent appears to be installed on this machine.
    /// `suivi init` only installs hooks for installed agents.
    fn is_installed(&self) -> bool;
}

/// True if the user's home directory contains `rel` as a directory
/// (e.g. ".claude" — agents create their config dir on first run).
pub fn home_dir_exists(rel: &str) -> bool {
    dirs::home_dir()
        .map(|h| h.join(rel).is_dir())
        .unwrap_or(false)
}

/// True if `bin` exists as a file in any entry of the PATH environment variable.
pub fn binary_on_path(bin: &str) -> bool {
    binary_in_path_var(bin, std::env::var_os("PATH").as_deref())
}

fn binary_in_path_var(bin: &str, path: Option<&std::ffi::OsStr>) -> bool {
    let Some(path) = path else { return false };
    std::env::split_paths(path).any(|dir| dir.join(bin).is_file())
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
    fn test_binary_in_path_var() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("some-agent"), "").unwrap();
        let path_var = std::env::join_paths([dir.path()]).unwrap();

        assert!(binary_in_path_var("some-agent", Some(&path_var)));
        assert!(!binary_in_path_var("absent-agent", Some(&path_var)));
        assert!(!binary_in_path_var("some-agent", None));
    }

    #[test]
    fn test_is_installed_no_panic() {
        // Result depends on the machine; just exercise every implementation.
        for agent in all_agents() {
            let _ = agent.is_installed();
        }
    }
}
