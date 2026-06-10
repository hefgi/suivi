use crate::agents::{Agent, AgentPayload, Env, HookDest, HookFile, HookTemplates};
use std::path::PathBuf;

pub struct ClaudeCodeAgent;

impl Agent for ClaudeCodeAgent {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn detect(&self, env: &Env) -> bool {
        // Claude Code sets `CLAUDECODE=1` and `CLAUDE_CODE_*` in the environment of
        // every spawned hook process. Parent process is the `claude` binary.
        env.vars.contains_key("CLAUDECODE")
            || env.vars.contains_key("CLAUDE_CODE_ENTRYPOINT")
            || env
                .parent_process_name
                .as_deref()
                .map(|n| n == "claude" || n.contains("claude-code"))
                .unwrap_or(false)
    }

    fn parse_payload(&self, raw: &str) -> Option<AgentPayload> {
        let v: serde_json::Value = serde_json::from_str(raw).ok()?;
        let session_id = v.get("session_id")?.as_str()?.to_string();
        let cwd = v.get("cwd")?.as_str()?.to_string();
        let model = v
            .get("model")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string());
        Some(AgentPayload {
            session_id,
            cwd,
            model,
        })
    }

    fn hook_templates(&self) -> HookTemplates {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
        let dest = home.join(".claude").join("settings.json");

        HookTemplates {
            files: vec![
                HookFile {
                    dest: HookDest::JsonMerge(dest.clone()),
                    filename: "user_prompt_submit.json".to_string(),
                    content: include_str!("hooks/user_prompt_submit.json").to_string(),
                },
                HookFile {
                    dest: HookDest::JsonMerge(dest),
                    filename: "stop.json".to_string(),
                    content: include_str!("hooks/stop.json").to_string(),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Env;
    use std::collections::HashMap;

    fn env_with(vars: &[(&str, &str)]) -> Env {
        let mut map = HashMap::new();
        for (k, v) in vars {
            map.insert(k.to_string(), v.to_string());
        }
        Env {
            vars: map,
            parent_process_name: None,
        }
    }

    fn env_with_parent(parent: &str) -> Env {
        Env {
            vars: HashMap::new(),
            parent_process_name: Some(parent.to_string()),
        }
    }

    #[test]
    fn test_detect_by_env_var() {
        let agent = ClaudeCodeAgent;
        assert!(agent.detect(&env_with(&[("CLAUDECODE", "1")])));
        assert!(agent.detect(&env_with(&[("CLAUDE_CODE_ENTRYPOINT", "cli")])));
        assert!(!agent.detect(&env_with(&[("OTHER_VAR", "value")])));
    }

    #[test]
    fn test_detect_by_parent_process() {
        let agent = ClaudeCodeAgent;
        assert!(agent.detect(&env_with_parent("claude")));
        assert!(!agent.detect(&env_with_parent("vim")));
    }

    #[test]
    fn test_parse_payload_valid() {
        let agent = ClaudeCodeAgent;
        let json = r#"{"session_id":"sess1","cwd":"/home/user","model":"claude-3-5-sonnet","hook_event_name":"UserPromptSubmit"}"#;
        let payload = agent.parse_payload(json);
        assert!(payload.is_some());
        let p = payload.unwrap();
        assert_eq!(p.session_id, "sess1");
        assert_eq!(p.cwd, "/home/user");
        assert_eq!(p.model.as_deref(), Some("claude-3-5-sonnet"));
    }

    #[test]
    fn test_parse_payload_malformed() {
        let agent = ClaudeCodeAgent;
        assert!(agent.parse_payload("not json").is_none());
        assert!(agent.parse_payload("{}").is_none()); // missing session_id
        assert!(agent.parse_payload("").is_none());
    }

    #[test]
    fn test_parse_payload_no_model() {
        let agent = ClaudeCodeAgent;
        let json = r#"{"session_id":"sess2","cwd":"/tmp"}"#;
        let payload = agent.parse_payload(json);
        assert!(payload.is_some());
        assert!(payload.unwrap().model.is_none());
    }

    #[test]
    fn test_parse_payload_stop_event() {
        let agent = ClaudeCodeAgent;
        // Stop event has no cwd — should return None since cwd is required
        let json = r#"{"session_id":"sess3","hook_event_name":"Stop","duration_ms":30000}"#;
        // No cwd means parse_payload returns None — that's OK, stop handler reads from last turn
        let payload = agent.parse_payload(json);
        assert!(payload.is_none());
    }

    #[test]
    fn test_hook_templates_not_empty() {
        let agent = ClaudeCodeAgent;
        let templates = agent.hook_templates();
        assert!(!templates.files.is_empty());
        assert_eq!(templates.files.len(), 2);
    }

    #[test]
    fn test_id_and_display_name() {
        let agent = ClaudeCodeAgent;
        assert_eq!(agent.id(), "claude-code");
        assert_eq!(agent.display_name(), "Claude Code");
    }
}
