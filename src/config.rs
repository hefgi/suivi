use crate::error::SuiviError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── New schema (PRD-compliant) ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub path: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tracking {
    #[serde(default = "default_human_buffer_secs")]
    pub human_buffer_secs: u32,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub tracking: Tracking,
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
}

fn default_human_buffer_secs() -> u32 {
    300
}
fn default_retention_days() -> u32 {
    365
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tracking: Tracking {
                human_buffer_secs: default_human_buffer_secs(),
                retention_days: default_retention_days(),
            },
            projects: vec![],
        }
    }
}

// ── Legacy schema (v0) for transparent migration ──────────────────────────────

#[derive(Debug, Deserialize)]
struct ConfigV0 {
    #[serde(default = "default_buffer_mins_v0")]
    buffer_mins: u32,
    #[serde(default = "default_retention_days_v0")]
    retention_days: u32,
    #[serde(default)]
    projects: Vec<ProjectEntryV0>,
}

fn default_buffer_mins_v0() -> u32 {
    5
}
fn default_retention_days_v0() -> u32 {
    90
}

#[derive(Debug, Deserialize)]
struct ProjectEntryV0 {
    paths: Vec<String>,
    name: Option<String>,
}

impl From<ConfigV0> for Config {
    fn from(v0: ConfigV0) -> Self {
        Config {
            tracking: Tracking {
                human_buffer_secs: v0.buffer_mins * 60,
                retention_days: v0.retention_days,
            },
            projects: v0
                .projects
                .into_iter()
                .flat_map(|e| {
                    let name = e.name.clone();
                    e.paths.into_iter().enumerate().map(move |(i, p)| ProjectEntry {
                        path: p,
                        name: if i == 0 { name.clone() } else { None },
                    })
                })
                .collect(),
        }
    }
}

// ── Paths ─────────────────────────────────────────────────────────────────────

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
        .join("suivi")
        .join("config.toml")
}

// ── Load / Save ───────────────────────────────────────────────────────────────

pub fn load() -> Result<Config, SuiviError> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }
    load_from(&path)
}

pub fn load_from(path: &Path) -> Result<Config, SuiviError> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let content = std::fs::read_to_string(path)?;
    // Try new schema first.
    if let Ok(cfg) = toml::from_str::<Config>(&content) {
        // Distinguish new format from old: new format has [tracking] section OR no buffer_mins key.
        if !content.contains("buffer_mins") {
            return Ok(cfg);
        }
    }
    // Fall back to legacy schema.
    if let Ok(v0) = toml::from_str::<ConfigV0>(&content) {
        return Ok(Config::from(v0));
    }
    // Last resort: return the new-schema parse result (may have partial fields).
    let cfg: Config = toml::from_str(&content)?;
    Ok(cfg)
}

pub fn save(config: &Config) -> Result<(), SuiviError> {
    let path = config_path();
    save_to(config, &path)
}

pub fn save_to(config: &Config, path: &Path) -> Result<(), SuiviError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config).map_err(SuiviError::from)?;
    std::fs::write(path, content)?;
    Ok(())
}

// ── Path helpers ──────────────────────────────────────────────────────────────

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{}", home.display(), rest);
        }
    } else if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().to_string();
        }
    }
    s.to_string()
}

fn has_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

pub fn expand_globs(path: &str) -> Vec<PathBuf> {
    let expanded = expand_tilde(path);
    let mut results = Vec::new();
    if has_glob_chars(&expanded) {
        match glob::glob(&expanded) {
            Ok(paths) => {
                for p in paths.flatten() {
                    results.push(p);
                }
            }
            Err(_) => {
                results.push(PathBuf::from(&expanded));
            }
        }
    } else {
        results.push(PathBuf::from(&expanded));
    }
    results.sort();
    results.dedup();
    results
}

pub fn find_project<'a>(config: &'a Config, cwd: &str) -> Option<(&'a ProjectEntry, PathBuf)> {
    let cwd_path = Path::new(cwd);
    let mut best: Option<(&'a ProjectEntry, PathBuf)> = None;

    for entry in &config.projects {
        for path in expand_globs(&entry.path) {
            if cwd_path.starts_with(&path) {
                let is_better = match &best {
                    None => true,
                    Some((_, best_path)) => path.as_os_str().len() > best_path.as_os_str().len(),
                };
                if is_better {
                    best = Some((entry, path));
                }
            }
        }
    }

    best
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.tracking.human_buffer_secs, 300);
        assert_eq!(config.tracking.retention_days, 365);
        assert!(config.projects.is_empty());
    }

    #[test]
    fn test_load_missing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.toml");
        let config = load_from(&path).unwrap();
        assert_eq!(config.tracking.human_buffer_secs, 300);
        assert_eq!(config.tracking.retention_days, 365);
    }

    #[test]
    fn test_load_and_save_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let original = Config {
            tracking: Tracking {
                human_buffer_secs: 600,
                retention_days: 180,
            },
            projects: vec![ProjectEntry {
                path: "/home/user/project".to_string(),
                name: Some("My Project".to_string()),
            }],
        };
        save_to(&original, &path).unwrap();
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.tracking.human_buffer_secs, 600);
        assert_eq!(loaded.tracking.retention_days, 180);
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name.as_deref(), Some("My Project"));
    }

    #[test]
    fn test_legacy_migration() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let old = r#"
buffer_mins = 10
retention_days = 60

[[projects]]
paths = ["/home/user/proj"]
name = "TestProj"
"#;
        std::fs::write(&path, old).unwrap();
        let cfg = load_from(&path).unwrap();
        assert_eq!(cfg.tracking.human_buffer_secs, 600); // 10 * 60
        assert_eq!(cfg.tracking.retention_days, 60);
        assert_eq!(cfg.projects.len(), 1);
        assert_eq!(cfg.projects[0].path, "/home/user/proj");
        assert_eq!(cfg.projects[0].name.as_deref(), Some("TestProj"));
    }

    #[test]
    fn test_legacy_migration_multi_paths() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let old = r#"
buffer_mins = 5

[[projects]]
paths = ["/a", "/b", "/c"]
"#;
        std::fs::write(&path, old).unwrap();
        let cfg = load_from(&path).unwrap();
        assert_eq!(cfg.projects.len(), 3);
        assert_eq!(cfg.projects[0].path, "/a");
        assert_eq!(cfg.projects[1].path, "/b");
    }

    #[test]
    fn test_find_project_exact_match() {
        let config = Config {
            tracking: Tracking::default(),
            projects: vec![ProjectEntry {
                path: "/home/user/project".to_string(),
                name: Some("Test".to_string()),
            }],
        };
        let result = find_project(&config, "/home/user/project");
        assert!(result.is_some());
    }

    #[test]
    fn test_find_project_ancestor() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("sub").join("deep");
        std::fs::create_dir_all(&subdir).unwrap();

        let config = Config {
            tracking: Tracking::default(),
            projects: vec![ProjectEntry {
                path: dir.path().to_str().unwrap().to_string(),
                name: None,
            }],
        };
        let result = find_project(&config, subdir.to_str().unwrap());
        assert!(result.is_some());
    }

    #[test]
    fn test_find_project_most_specific() {
        let dir = TempDir::new().unwrap();
        let parent = dir.path().to_path_buf();
        let child = parent.join("child");
        std::fs::create_dir_all(&child).unwrap();
        let grandchild = child.join("grandchild");
        std::fs::create_dir_all(&grandchild).unwrap();

        let config = Config {
            tracking: Tracking::default(),
            projects: vec![
                ProjectEntry {
                    path: parent.to_str().unwrap().to_string(),
                    name: Some("Parent".to_string()),
                },
                ProjectEntry {
                    path: child.to_str().unwrap().to_string(),
                    name: Some("Child".to_string()),
                },
            ],
        };
        let result = find_project(&config, grandchild.to_str().unwrap());
        assert!(result.is_some());
        let (entry, _) = result.unwrap();
        assert_eq!(entry.name.as_deref(), Some("Child"));
    }

    #[test]
    fn test_find_project_no_match() {
        let config = Config {
            tracking: Tracking::default(),
            projects: vec![ProjectEntry {
                path: "/home/user/project".to_string(),
                name: None,
            }],
        };
        let result = find_project(&config, "/other/path");
        assert!(result.is_none());
    }

    #[test]
    fn test_expand_globs_tilde() {
        let expanded = expand_globs("~/");
        if let Some(home) = dirs::home_dir() {
            assert!(expanded.iter().any(|p| p == &home));
        }
    }

    #[test]
    fn test_find_project_name_optional() {
        let dir = TempDir::new().unwrap();
        let config = Config {
            tracking: Tracking::default(),
            projects: vec![ProjectEntry {
                path: dir.path().to_str().unwrap().to_string(),
                name: None,
            }],
        };
        let result = find_project(&config, dir.path().to_str().unwrap());
        assert!(result.is_some());
        let (entry, _) = result.unwrap();
        assert!(entry.name.is_none());
    }
}
