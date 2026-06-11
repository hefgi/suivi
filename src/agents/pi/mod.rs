use crate::agents::{Agent, AgentPayload, Env, HookDest, HookFile, HookTemplates};
use std::path::PathBuf;

pub struct PiAgent;

impl Agent for PiAgent {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn display_name(&self) -> &'static str {
        "Pi (experimental)"
    }

    fn is_installed(&self) -> bool {
        crate::agents::home_dir_exists(".pi") || crate::agents::binary_on_path("pi")
    }

    fn detect(&self, env: &Env) -> bool {
        env.vars.contains_key("PI_SESSION")
            || env
                .parent_process_name
                .as_deref()
                .map(|n| n == "pi" || n.ends_with("/pi"))
                .unwrap_or(false)
    }

    fn parse_payload(&self, raw: &str) -> Option<AgentPayload> {
        let v: serde_json::Value = serde_json::from_str(raw).ok()?;
        let session_id = v.get("session_id")?.as_str()?.to_string();
        let cwd = v.get("cwd")?.as_str()?.to_string();
        // Pi does not provide model info
        Some(AgentPayload {
            session_id,
            cwd,
            model: None,
        })
    }

    fn hook_templates(&self) -> HookTemplates {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
        let dest = home
            .join(".pi")
            .join("agent")
            .join("extensions")
            .join("suivi.js");

        HookTemplates {
            files: vec![HookFile {
                dest: HookDest::WriteFile(dest),
                filename: "suivi.js".to_string(),
                content: include_str!("hooks/suivi.js").to_string(),
            }],
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
        let agent = PiAgent;
        assert!(agent.detect(&env_with(&[("PI_SESSION", "pi_sess")])));
        assert!(!agent.detect(&env_with(&[("OTHER_VAR", "value")])));
    }

    #[test]
    fn test_detect_by_parent_process() {
        let agent = PiAgent;
        assert!(agent.detect(&env_with_parent("pi")));
        assert!(!agent.detect(&env_with_parent("vim")));
        assert!(!agent.detect(&env_with_parent("pip")));
        assert!(!agent.detect(&env_with_parent("pipenv")));
    }

    #[test]
    fn test_parse_payload_valid() {
        let agent = PiAgent;
        let json = r#"{"session_id":"pi_sess_abc","cwd":"/home/user"}"#;
        let payload = agent.parse_payload(json);
        assert!(payload.is_some());
        let p = payload.unwrap();
        assert_eq!(p.session_id, "pi_sess_abc");
        assert_eq!(p.cwd, "/home/user");
        assert!(p.model.is_none());
    }

    #[test]
    fn test_parse_payload_malformed() {
        let agent = PiAgent;
        assert!(agent.parse_payload("not json").is_none());
        assert!(agent.parse_payload("{}").is_none());
        assert!(agent.parse_payload("").is_none());
    }

    #[test]
    fn test_parse_payload_no_model() {
        let agent = PiAgent;
        // Pi never provides model even if present in JSON — always None
        let json = r#"{"session_id":"pi_sess_abc","cwd":"/tmp","model":"some-model"}"#;
        let payload = agent.parse_payload(json);
        assert!(payload.is_some());
        assert!(payload.unwrap().model.is_none());
    }

    #[test]
    fn test_hook_templates_not_empty() {
        let agent = PiAgent;
        let templates = agent.hook_templates();
        assert!(!templates.files.is_empty());
        assert_eq!(templates.files.len(), 1);
    }

    #[test]
    fn test_id_and_display_name() {
        let agent = PiAgent;
        assert_eq!(agent.id(), "pi");
        assert_eq!(agent.display_name(), "Pi (experimental)");
    }
}
