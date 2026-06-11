//--+ src/config.rs

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum FileKind {
    Rust,       // Full AST compression, goes into skeleton
    Structured, // Strip comments, collapse whitespace, cache optional
    Raw,        // Serve verbatim from disk
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileTypeRule {
    pub extension: String,
    pub kind: FileKind,
    pub include_in_skeleton: bool, // ONLY Rust gets true
    pub strip_comments: bool,
    pub collapse_whitespace: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScanConfig {
    pub rules: Vec<FileTypeRule>,
    pub skip_dirs: Vec<String>,
    pub skip_patterns: Vec<String>,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            rules: vec![
                // ── Rust: full compression ──
                FileTypeRule {
                    extension: "rs".into(),
                    kind: FileKind::Rust,
                    include_in_skeleton: true,
                    strip_comments: false, // handled by rust pipeline
                    collapse_whitespace: false,
                },
                // ── Structured: strip comments, preserve structure ──
                FileTypeRule {
                    extension: "toml".into(),
                    kind: FileKind::Structured,
                    include_in_skeleton: false,
                    strip_comments: true,
                    collapse_whitespace: true,
                },
                FileTypeRule {
                    extension: "yml".into(),
                    kind: FileKind::Structured,
                    include_in_skeleton: false,
                    strip_comments: true,
                    collapse_whitespace: true,
                },
                FileTypeRule {
                    extension: "yaml".into(),
                    kind: FileKind::Structured,
                    include_in_skeleton: false,
                    strip_comments: true,
                    collapse_whitespace: true,
                },
                FileTypeRule {
                    extension: "json".into(),
                    kind: FileKind::Structured,
                    include_in_skeleton: false,
                    strip_comments: false,
                    collapse_whitespace: true,
                },
                FileTypeRule {
                    extension: "sql".into(),
                    kind: FileKind::Structured,
                    include_in_skeleton: false,
                    strip_comments: true,
                    collapse_whitespace: true,
                },
                FileTypeRule {
                    extension: "proto".into(),
                    kind: FileKind::Structured,
                    include_in_skeleton: false,
                    strip_comments: true,
                    collapse_whitespace: true,
                },
                FileTypeRule {
                    extension: "env".into(),
                    kind: FileKind::Structured,
                    include_in_skeleton: false,
                    strip_comments: true,
                    collapse_whitespace: true,
                },
                // ── Raw: serve as-is ──
                FileTypeRule {
                    extension: "md".into(),
                    kind: FileKind::Raw,
                    include_in_skeleton: false,
                    strip_comments: false,
                    collapse_whitespace: false,
                },
                FileTypeRule {
                    extension: "sh".into(),
                    kind: FileKind::Raw,
                    include_in_skeleton: false,
                    strip_comments: true,
                    collapse_whitespace: true,
                },
            ],
            skip_dirs: vec![
                "target".into(),
                "node_modules".into(),
                ".git".into(),
                "dist".into(),
                "build".into(),
                "__pycache__".into(),
                ".idea".into(),
                ".vscode".into(),
            ],
            skip_patterns: vec!["*.lock".into(), "*.min.js".into(), "*.min.css".into()],
        }
    }
}

impl ScanConfig {
    pub fn rule_for(&self, path: &Path) -> Option<&FileTypeRule> {
        let ext = path.extension()?.to_str()?;
        self.rules.iter().find(|r| r.extension == ext)
    }

    pub fn should_skip_dir(&self, dir_name: &str) -> bool {
        self.skip_dirs.contains(&dir_name.to_string())
    }
}
