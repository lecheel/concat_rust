use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::fingerprint::FileFingerprint;

// ── Data Structures ──────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BodyMeta {
    pub filepath: String,
    pub loc: usize,
    pub byte_size: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BodyEntry {
    pub meta: BodyMeta,
    pub body: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileEntry {
    pub loc: usize,
    pub byte_size: usize,
    pub body_hashes: Vec<String>,
    pub code: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DaemonCache {
    /// hash → body entry
    pub bodies: HashMap<String, BodyEntry>,

    /// filepath → file entry (RUST FILES ONLY)
    pub files: HashMap<String, FileEntry>,

    /// Per-file skeleton segments — assembled on demand by the API (RUST FILES ONLY)
    #[serde(default)]
    pub skeleton_segments: HashMap<String, String>,

    /// Ordered list of file paths (deterministic assembly order)
    #[serde(default)]
    pub file_order: Vec<String>,

    /// File fingerprints for incremental dirty detection
    #[serde(default)]
    pub fingerprints: HashMap<String, FileFingerprint>,

    /// The meta-prompt appended to skeleton responses
    #[serde(default)]
    pub meta_prompt: String,

    /// Version stamp — incremented on each index run
    #[serde(default)]
    pub generation: u64,

    /// Not serialized — set at runtime
    #[serde(skip)]
    pub cache_path: String,
}

// ── Implementation ───────────────────────────────────────────

impl DaemonCache {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let data = std::fs::read_to_string(path)?;
        let mut cache: DaemonCache = serde_json::from_str(&data)?;
        cache.cache_path = path.to_string();
        Ok(cache)
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&self.cache_path, json)?;
        Ok(())
    }

    /// Evict all cache data related to a specific file path.
    /// Used before reindexing a changed file.
    pub fn evict_file(&mut self, path: &str) {
        if let Some(old_entry) = self.files.remove(path) {
            for hash in &old_entry.body_hashes {
                self.bodies.remove(hash);
            }
        }
        self.skeleton_segments.remove(path);
        self.file_order.retain(|p| p != path);
    }

    /// Assemble the skeleton for a specific repo, or all repos.
    /// ONLY includes .rs files.
    pub fn assemble_skeleton_for_repos(&self, repo_filter: Option<&str>) -> String {
        let mut parts = Vec::new();

        for filepath in &self.file_order {
            // Skip files not matching the repo filter
            if let Some(repo) = repo_filter {
                if !filepath.starts_with(&format!("{}/", repo)) {
                    continue;
                }
            }

            // Skeleton is Rust ONLY
            if !filepath.ends_with(".rs") {
                continue;
            }

            if let Some(segment) = self.skeleton_segments.get(filepath) {
                let file_entry = self.files.get(filepath);
                let loc = file_entry.map(|e| e.loc).unwrap_or(0);
                let body_count = file_entry.map(|e| e.body_hashes.len()).unwrap_or(0);

                parts.push(format!(
                    "//--+ file:///{} [{} LOC | {} bodies]\n{}",
                    filepath, loc, body_count, segment
                ));
            }
        }

        parts.join("\n")
    }

    /// Assemble the full skeleton HTTP response with headers and meta-prompt.
    pub fn assemble_full_skeleton_response(&self, daemon_port: u16, repo_param: &str) -> String {
        let header = format!(
            "// === SKELETON MODE (COMPRESSED) ===\n\
             // Hash fetch:     http://localhost:{}/<HASH>\n\
             // Multi-hash:     http://localhost:{}/<HASH1>+<HASH2>\n\
             // Whole file:     http://localhost:{}/file/<PATH>\n\
             // Skeleton:       http://localhost:{}/skeleton\n\
             // Catalog:        http://localhost:{}/catalog\n\
             // Body info:      http://localhost:{}/info/<HASH>\n\
             // File info:      http://localhost:{}/file-info/<PATH>\n\
             // Write file:     PUT http://localhost:{}/file/<PATH>\n\
             // Sync:           POST http://localhost:{}/sync\n\
             // ===================================\n\n",
            daemon_port,
            daemon_port,
            daemon_port,
            daemon_port,
            daemon_port,
            daemon_port,
            daemon_port,
            daemon_port,
            daemon_port
        );

        let repo_filter = if repo_param == "all" {
            None
        } else {
            Some(repo_param)
        };

        let skeleton = self.assemble_skeleton_for_repos(repo_filter);
        format!("{}{}{}", header, skeleton, self.meta_prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.cache");
        let path_str = path.to_string_lossy().to_string();

        let mut cache = DaemonCache::default();
        cache.cache_path = path_str.clone();
        cache.generation = 1;
        cache.file_order.push("core/src/main.rs".to_string());

        cache.bodies.insert(
            "abc123".to_string(),
            BodyEntry {
                meta: BodyMeta {
                    filepath: "core/src/main.rs".to_string(),
                    loc: 10,
                    byte_size: 50,
                },
                body: "fn main() {}".to_string(),
            },
        );

        cache.files.insert(
            "core/src/main.rs".to_string(),
            FileEntry {
                loc: 10,
                byte_size: 50,
                body_hashes: vec!["abc123".to_string()],
                code: "fn main() {}".to_string(),
            },
        );

        cache.skeleton_segments.insert(
            "core/src/main.rs".to_string(),
            "fn main() { /* HASH:abc123 [10 LOC] */ }".to_string(),
        );

        cache.save().unwrap();
        let loaded = DaemonCache::load(&path_str).unwrap();

        assert_eq!(loaded.generation, 1);
        assert_eq!(loaded.bodies.len(), 1);
        assert_eq!(loaded.files.len(), 1);
        assert!(loaded.bodies.contains_key("abc123"));
        assert_eq!(loaded.cache_path, path_str);
    }

    #[test]
    fn test_evict_file() {
        let mut cache = DaemonCache::default();
        cache.file_order.push("core/src/main.rs".to_string());
        cache.bodies.insert(
            "abc123".to_string(),
            BodyEntry {
                meta: BodyMeta {
                    filepath: "core/src/main.rs".to_string(),
                    loc: 10,
                    byte_size: 50,
                },
                body: "fn main() {}".to_string(),
            },
        );
        cache.files.insert(
            "core/src/main.rs".to_string(),
            FileEntry {
                loc: 10,
                byte_size: 50,
                body_hashes: vec!["abc123".to_string()],
                code: "fn main() {}".to_string(),
            },
        );
        cache
            .skeleton_segments
            .insert("core/src/main.rs".to_string(), "skeleton".to_string());

        cache.evict_file("core/src/main.rs");

        assert!(cache.bodies.is_empty());
        assert!(cache.files.is_empty());
        assert!(cache.skeleton_segments.is_empty());
        assert!(cache.file_order.is_empty());
    }

    #[test]
    fn test_assemble_skeleton_filters_by_repo() {
        let mut cache = DaemonCache::default();
        cache.file_order.push("core/src/main.rs".to_string());
        cache.file_order.push("api/src/handler.rs".to_string());
        cache.file_order.push("core/Cargo.toml".to_string()); // Not .rs

        cache
            .skeleton_segments
            .insert("core/src/main.rs".to_string(), "core skeleton".to_string());
        cache
            .skeleton_segments
            .insert("api/src/handler.rs".to_string(), "api skeleton".to_string());

        cache.files.insert(
            "core/src/main.rs".to_string(),
            FileEntry {
                loc: 10,
                byte_size: 50,
                body_hashes: vec![],
                code: String::new(),
            },
        );
        cache.files.insert(
            "api/src/handler.rs".to_string(),
            FileEntry {
                loc: 20,
                byte_size: 100,
                body_hashes: vec![],
                code: String::new(),
            },
        );

        // Filter: core only
        let core_skeleton = cache.assemble_skeleton_for_repos(Some("core"));
        assert!(core_skeleton.contains("core/src/main.rs"));
        assert!(!core_skeleton.contains("api/src/handler.rs"));
        assert!(!core_skeleton.contains("Cargo.toml")); // Never non-rs

        // Filter: all
        let all_skeleton = cache.assemble_skeleton_for_repos(None);
        assert!(all_skeleton.contains("core/src/main.rs"));
        assert!(all_skeleton.contains("api/src/handler.rs"));
    }
}
