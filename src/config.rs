use crate::error::SuiviError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub paths: Vec<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_buffer_mins")]
    pub buffer_mins: u32,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
}

fn default_buffer_mins() -> u32 {
    5
}
fn default_retention_days() -> u32 {
    90
}

impl Default for Config {
    fn default() -> Self {
        Self {
            buffer_mins: default_buffer_mins(),
            retention_days: default_retention_days(),
            projects: vec![],
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
        .join("suivi")
        .join("config.toml")
}

pub fn load() -> Result<Config, SuiviError> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }
    let content = std::fs::read_to_string(&path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

pub fn load_from(path: &Path) -> Result<Config, SuiviError> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let content = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
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

pub fn expand_globs(entry: &ProjectEntry) -> Vec<PathBuf> {
    let mut results = Vec::new();
    for pattern in &entry.paths {
        let expanded = expand_tilde(pattern);
        if has_glob_chars(&expanded) {
            match glob::glob(&expanded) {
                Ok(paths) => {
                    for path in paths.flatten() {
                        results.push(path);
                    }
                }
                Err(_) => {
                    // If glob fails, treat as literal path
                    results.push(PathBuf::from(&expanded));
                }
            }
        } else {
            // No glob characters: treat as literal path regardless of existence
            results.push(PathBuf::from(&expanded));
        }
    }
    results.sort();
    results.dedup();
    results
}

pub fn find_project<'a>(config: &'a Config, cwd: &str) -> Option<(&'a ProjectEntry, PathBuf)> {
    let cwd_path = Path::new(cwd);
    let mut best: Option<(&'a ProjectEntry, PathBuf)> = None;

    for entry in &config.projects {
        let expanded = expand_globs(entry);
        for path in expanded {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.buffer_mins, 5);
        assert_eq!(config.retention_days, 90);
        assert!(config.projects.is_empty());
    }

    #[test]
    fn test_load_missing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.toml");
        let config = load_from(&path).unwrap();
        assert_eq!(config.buffer_mins, 5);
        assert_eq!(config.retention_days, 90);
    }

    #[test]
    fn test_load_and_save_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let original = Config {
            buffer_mins: 10,
            retention_days: 180,
            projects: vec![ProjectEntry {
                paths: vec!["/home/user/project".to_string()],
                name: Some("My Project".to_string()),
            }],
        };
        save_to(&original, &path).unwrap();
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.buffer_mins, 10);
        assert_eq!(loaded.retention_days, 180);
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name.as_deref(), Some("My Project"));
    }

    #[test]
    fn test_find_project_exact_match() {
        let config = Config {
            buffer_mins: 5,
            retention_days: 90,
            projects: vec![ProjectEntry {
                paths: vec!["/home/user/project".to_string()],
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
            buffer_mins: 5,
            retention_days: 90,
            projects: vec![ProjectEntry {
                paths: vec![dir.path().to_str().unwrap().to_string()],
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
            buffer_mins: 5,
            retention_days: 90,
            projects: vec![
                ProjectEntry {
                    paths: vec![parent.to_str().unwrap().to_string()],
                    name: Some("Parent".to_string()),
                },
                ProjectEntry {
                    paths: vec![child.to_str().unwrap().to_string()],
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
            buffer_mins: 5,
            retention_days: 90,
            projects: vec![ProjectEntry {
                paths: vec!["/home/user/project".to_string()],
                name: None,
            }],
        };
        let result = find_project(&config, "/other/path");
        assert!(result.is_none());
    }

    #[test]
    fn test_expand_globs_tilde() {
        let entry = ProjectEntry {
            paths: vec!["~/".to_string()],
            name: None,
        };
        let expanded = expand_globs(&entry);
        // Should expand tilde — the home dir itself should match
        if let Some(home) = dirs::home_dir() {
            assert!(expanded.iter().any(|p| p == &home));
        }
    }

    #[test]
    fn test_find_project_name_optional() {
        let dir = TempDir::new().unwrap();
        let config = Config {
            buffer_mins: 5,
            retention_days: 90,
            projects: vec![ProjectEntry {
                paths: vec![dir.path().to_str().unwrap().to_string()],
                name: None,
            }],
        };
        let result = find_project(&config, dir.path().to_str().unwrap());
        assert!(result.is_some());
        let (entry, _) = result.unwrap();
        assert!(entry.name.is_none());
    }
}
