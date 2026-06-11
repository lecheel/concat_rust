//--+ src/registry.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RepoEntry {
    /// Short name used as subdirectory in central dir (e.g., "core", "api")
    pub id: String,
    /// Absolute path to the real repo on disk
    pub source_path: PathBuf,
    /// Whether this repo is actively synced
    pub active: bool,
    /// Last successful sync timestamp (unix epoch seconds)
    pub last_sync: Option<u64>,
    /// Git branch if detected
    pub git_branch: Option<String>,
    /// Number of files at last sync
    pub file_count: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RepoRegistry {
    pub repos: HashMap<String, RepoEntry>,
    /// Absolute path to central directory
    pub central_dir: PathBuf,
}

impl RepoRegistry {
    /// Load registry from disk, or create a default one.
    pub fn load_or_create(central_dir: &PathBuf) -> Self {
        let registry_path = central_dir.join(".registry.json");
        if registry_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&registry_path) {
                if let Ok(registry) = serde_json::from_str(&data) {
                    return registry;
                }
            }
        }
        Self {
            central_dir: central_dir.clone(),
            repos: HashMap::new(),
        }
    }

    /// Persist the registry to disk.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(&self.central_dir)?;
        let registry_path = self.central_dir.join(".registry.json");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(registry_path, json)?;
        Ok(())
    }

    /// Register a new repo, or update the source path of an existing one.
    /// Returns true if this was a new registration.
    pub fn add_repo(&mut self, id: &str, source_path: PathBuf) -> bool {
        let is_new = !self.repos.contains_key(id);
        self.repos.insert(
            id.to_string(),
            RepoEntry {
                id: id.to_string(),
                source_path,
                active: true,
                last_sync: None,
                git_branch: None,
                file_count: None,
            },
        );
        is_new
    }

    /// Unregister a repo.
    pub fn remove_repo(&mut self, id: &str) {
        self.repos.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_remove_repo() {
        let dir = tempfile::tempdir().unwrap();
        let central = dir.path().to_path_buf();
        let mut registry = RepoRegistry::load_or_create(&central);

        assert!(registry.add_repo("core", PathBuf::from("/tmp/core")));
        assert!(!registry.add_repo("core", PathBuf::from("/tmp/core2"))); // duplicate

        assert!(registry.repos.contains_key("core"));

        registry.remove_repo("core");
        assert!(!registry.repos.contains_key("core"));
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let central = dir.path().to_path_buf();

        let mut registry = RepoRegistry::load_or_create(&central);
        registry.add_repo("api", PathBuf::from("/tmp/api"));
        registry.save().unwrap();

        let loaded = RepoRegistry::load_or_create(&central);
        assert!(loaded.repos.contains_key("api"));
        assert_eq!(loaded.central_dir, central);
    }
}
