use crate::agents::{Agent, AgentPayload, Env, HookDest, HookFile, HookTemplates};
use std::path::PathBuf;

pub struct CodexAgent;

impl Agent for CodexAgent {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn detect(&self, env: &Env) -> bool {
        env.vars.contains_key("CODEX_SESSION")
            || env
                .parent_process_name
                .as_deref()
                .map(|n| n.contains("codex"))
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
        let dest = home.join(".codex").join("hooks.json");

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
        let agent = CodexAgent;
        assert!(agent.detect(&env_with(&[("CODEX_SESSION", "abc")])));
        assert!(!agent.detect(&env_with(&[("OTHER_VAR", "value")])));
    }

    #[test]
    fn test_detect_by_parent_process() {
        let agent = CodexAgent;
        assert!(agent.detect(&env_with_parent("codex")));
        assert!(!agent.detect(&env_with_parent("vim")));
    }

    #[test]
    fn test_parse_payload_valid() {
        let agent = CodexAgent;
        let json = r#"{"session_id":"codex_abc","cwd":"/home/user","model":"o4-mini"}"#;
        let payload = agent.parse_payload(json);
        assert!(payload.is_some());
        let p = payload.unwrap();
        assert_eq!(p.session_id, "codex_abc");
        assert_eq!(p.cwd, "/home/user");
        assert_eq!(p.model.as_deref(), Some("o4-mini"));
    }

    #[test]
    fn test_parse_payload_malformed() {
        let agent = CodexAgent;
        assert!(agent.parse_payload("not json").is_none());
        assert!(agent.parse_payload("{}").is_none());
        assert!(agent.parse_payload("").is_none());
    }

    #[test]
    fn test_parse_payload_no_model() {
        let agent = CodexAgent;
        let json = r#"{"session_id":"codex_abc","cwd":"/tmp"}"#;
        let payload = agent.parse_payload(json);
        assert!(payload.is_some());
        assert!(payload.unwrap().model.is_none());
    }

    #[test]
    fn test_hook_templates_not_empty() {
        let agent = CodexAgent;
        let templates = agent.hook_templates();
        assert!(!templates.files.is_empty());
        assert_eq!(templates.files.len(), 2);
    }

    #[test]
    fn test_id_and_display_name() {
        let agent = CodexAgent;
        assert_eq!(agent.id(), "codex");
        assert_eq!(agent.display_name(), "Codex");
    }
}
