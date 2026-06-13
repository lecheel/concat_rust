use crate::fingerprint::FileFingerprint;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_META_PROMPT: &str = "\n===\n\
     Your process:\n\
     Analyze the feature requirements and determine which files, structs, traits, \
     and functions you need to see the full implementation of.\n\
     Prefer asking for whole files rather than individual hashes.\n\
     If a file is too large, ask for specific impl blocks or struct definitions by their HASH.\n\
     List exactly what you need in a clear, numbered list. For each item, include:\n\
     - The file path as shown in the skeleton header (e.g., src/main.rs).\n\
     - If you need a specific block, include its HASH (e.g., /* HASH:1a12fb93 [183 LOC] */).\n\
     - A brief reason (e.g., “to know the fields of AppState”, “to see how sync is implemented”).\n\
     \n\
     please ASKing using 'cli' like this in single for all\n\
     cli <path1> <path2> hash1 hash2         → fetch all in once\n\
     Do not guess or stub missing implementations.\n\
     Do not proceed until you have received all requested code.\n\
     ===";

/// Strips a known repo-id prefix from a path (e.g., "grab/src/main.rs" → "src/main.rs").
/// If no known repo prefix matches, returns the path unchanged.
pub fn strip_repo_prefix(path: &str, repo_ids: &[String]) -> String {
    for repo in repo_ids {
        let prefix = format!("{}/", repo);
        if let Some(stripped) = path.strip_prefix(&prefix) {
            return stripped.to_string();
        }
    }
    path.to_string()
}

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
    pub bodies: HashMap<String, BodyEntry>,
    pub files: HashMap<String, FileEntry>,
    #[serde(default)]
    pub skeleton_segments: HashMap<String, String>,
    #[serde(default)]
    pub file_order: Vec<String>,
    #[serde(default)]
    pub fingerprints: HashMap<String, FileFingerprint>,
    #[serde(default)]
    pub meta_prompt: String,
    #[serde(default)]
    pub generation: u64,
    #[serde(default)]
    pub file_hits: HashMap<String, u64>,
    #[serde(default)]
    pub hash_hits: HashMap<String, u64>,
    #[serde(skip)]
    pub cache_path: String,
}

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

    pub fn evict_file(&mut self, path: &str) {
        if let Some(old_entry) = self.files.remove(path) {
            for hash in &old_entry.body_hashes {
                self.bodies.remove(hash);
                self.hash_hits.remove(hash);
            }
        }
        self.skeleton_segments.remove(path);
        self.file_order.retain(|p| p != path);
        self.file_hits.remove(path);
    }

    pub fn assemble_skeleton_for_repos(
        &self,
        repo_filter: Option<&str>,
        repo_ids: &[String],
    ) -> String {
        let mut parts = Vec::new();
        for filepath in &self.file_order {
            if let Some(repo) = repo_filter {
                if !filepath.starts_with(&format!("{}/", repo)) {
                    continue;
                }
            }
            if !filepath.ends_with(".rs") {
                continue;
            }
            if let Some(segment) = self.skeleton_segments.get(filepath) {
                let file_entry = self.files.get(filepath);
                let loc = file_entry.map(|e| e.loc).unwrap_or(0);
                let body_count = file_entry.map(|e| e.body_hashes.len()).unwrap_or(0);
                let display_path = strip_repo_prefix(filepath, repo_ids);
                parts.push(format!(
                    "//--+ file:///{} [{} LOC | {} bodies]\n{}",
                    display_path, loc, body_count, segment
                ));
            }
        }
        parts.join("\n")
    }

    /// Returns the custom `meta_prompt` if configured, or falls back to the static default.
    pub fn effective_meta_prompt(&self) -> String {
        if !self.meta_prompt.is_empty() {
            self.meta_prompt.clone()
        } else {
            DEFAULT_META_PROMPT.to_string()
        }
    }

    pub fn assemble_full_skeleton_response(
        &self,
        _daemon_port: u16,
        repo_param: &str,
        repo_ids: &[String],
    ) -> String {
        let header = format!("// === SKELETON MODE (COMPRESSED) ===");
        let repo_filter = if repo_param == "all" {
            None
        } else {
            Some(repo_param)
        };
        let skeleton = self.assemble_skeleton_for_repos(repo_filter, repo_ids);
        let meta_prompt = self.effective_meta_prompt();
        format!("{}{}{}", header, skeleton, meta_prompt)
    }
}
