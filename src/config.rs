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
                    e.paths
                        .into_iter()
                        .enumerate()
                        .map(move |(i, p)| ProjectEntry {
                            path: p,
                            name: if i == 0 { name.clone() } else { None },
                        })
                })
                .collect(),
        }
    }
}

// ── Paths ─────────────────────────────────────────────────────────────────────

/// Returns `$XDG_CONFIG_HOME/suivi/config.toml` if set, otherwise `~/.config/suivi/config.toml`.
/// PRD specifies XDG paths on every OS; we don't follow `dirs::config_dir()` on macOS because
/// it returns `~/Library/Application Support`, which violates the spec.
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("suivi").join("config.toml")
}

/// One-shot migration from the legacy macOS path
/// (`~/Library/Application Support/suivi/config.toml`) to the XDG-spec'd location.
/// Runs at most once: if the XDG target already exists, does nothing.
/// No-op on non-macOS systems.
pub fn migrate_legacy_macos_config() {
    if cfg!(not(target_os = "macos")) {
        return;
    }
    let target = config_path();
    if target.exists() {
        return;
    }
    let Some(home) = dirs::home_dir() else { return };
    let legacy = home
        .join("Library")
        .join("Application Support")
        .join("suivi")
        .join("config.toml");
    if !legacy.exists() {
        return;
    }
    if let Some(parent) = target.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let _ = std::fs::rename(&legacy, &target);
}

// ── Load / Save ───────────────────────────────────────────────────────────────

pub fn load() -> Result<Config, SuiviError> {
    migrate_legacy_macos_config();
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

pub fn expand_tilde(s: &str) -> String {
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
                // Projects are directories; a stray file matching the glob
                // (e.g. org/README.md for org/*) must not become a project.
                for p in paths.flatten().filter(|p| p.is_dir()) {
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
    fn test_expand_globs_skips_files() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("real-project")).unwrap();
        std::fs::write(dir.path().join("stray-file.md"), "x").unwrap();
        let pattern = format!("{}/*", dir.path().display());
        let expanded = expand_globs(&pattern);
        assert_eq!(expanded, vec![dir.path().join("real-project")]);
    }

    #[test]
    fn test_find_project_overlapping_siblings_no_double_match() {
        // Scenario: parent project plus two siblings under it.
        //   ~/Enzyme
        //   ~/Enzyme/onyx
        //   ~/Enzyme/myso
        // A cwd under onyx must resolve to onyx, never to both onyx + Enzyme.
        let dir = TempDir::new().unwrap();
        let enzyme = dir.path().join("Enzyme");
        let onyx = enzyme.join("onyx");
        let myso = enzyme.join("myso");
        let onyx_deep = onyx.join("deep").join("subdir");
        let myso_deep = myso.join("sub");
        let enzyme_only = enzyme.join("just-a-file");
        std::fs::create_dir_all(&onyx_deep).unwrap();
        std::fs::create_dir_all(&myso_deep).unwrap();
        std::fs::create_dir_all(&enzyme_only).unwrap();

        let config = Config {
            tracking: Tracking::default(),
            projects: vec![
                ProjectEntry {
                    path: enzyme.to_str().unwrap().to_string(),
                    name: Some("Enzyme".to_string()),
                },
                ProjectEntry {
                    path: onyx.to_str().unwrap().to_string(),
                    name: Some("onyx".to_string()),
                },
                ProjectEntry {
                    path: myso.to_str().unwrap().to_string(),
                    name: Some("myso".to_string()),
                },
            ],
        };

        // cwd deep under onyx → onyx wins (deepest ancestor)
        let (entry, path) = find_project(&config, onyx_deep.to_str().unwrap()).unwrap();
        assert_eq!(entry.name.as_deref(), Some("onyx"));
        assert_eq!(path, onyx);

        // cwd deep under myso → myso wins
        let (entry, _) = find_project(&config, myso_deep.to_str().unwrap()).unwrap();
        assert_eq!(entry.name.as_deref(), Some("myso"));

        // cwd under Enzyme but NOT under onyx or myso → Enzyme wins
        let (entry, _) = find_project(&config, enzyme_only.to_str().unwrap()).unwrap();
        assert_eq!(entry.name.as_deref(), Some("Enzyme"));

        // cwd exactly at Enzyme → Enzyme wins
        let (entry, _) = find_project(&config, enzyme.to_str().unwrap()).unwrap();
        assert_eq!(entry.name.as_deref(), Some("Enzyme"));
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
