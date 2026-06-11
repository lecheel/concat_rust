//--+ src/scanner.rs

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cache::BodyMeta;
use crate::compress::compress_code;
use crate::config::{FileKind, ScanConfig};
use crate::rustfmt::run_rustfmt;
use crate::strip::{remove_empty_lines, remove_rust_comments, remove_test_modules};
use crate::strip_generic::{collapse_whitespace, strip_comments};

// ── Data Structures ──────────────────────────────────────────

pub struct ProcessedFile {
    pub rel_path: String,
    pub extension: String,
    pub kind: FileKind,
    pub loc: usize,
    pub byte_size: usize,
    pub code: String,
    /// For Rust files: compressed skeleton segment
    pub skeleton_segment: Option<String>,
    /// For Rust files: extracted bodies (hash, meta, body)
    pub bodies: Vec<(String, BodyMeta, String)>,
}

pub struct ScanResult {
    pub files: Vec<ProcessedFile>,
    pub total_loc: usize,
    pub by_type: HashMap<String, usize>,
}

// ── Public API ───────────────────────────────────────────────

/// Scan a directory and process all known file types.
/// Returns processed files ready to be inserted into the daemon cache.
pub fn scan_directory(
    dir: &Path,
    config: &ScanConfig,
    no_format: bool,
    max_width: i32,
) -> ScanResult {
    let all_files = walk_directory(dir, config);

    let mut files = Vec::new();
    let mut total_loc = 0usize;
    let mut by_type: HashMap<String, usize> = HashMap::new();

    for fp in &all_files {
        let rel = fp.strip_prefix(dir).unwrap_or(fp.as_path());
        let rel_str = rel.to_string_lossy().to_string();

        let rule = match config.rule_for(fp) {
            Some(r) => r,
            None => continue, // Unknown file type — skip
        };

        let raw_code = match fs::read_to_string(fp) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Warning: could not read {}: {}", fp.display(), e);
                continue;
            }
        };

        // Skip very large non-code files (images, binaries accidentally matched)
        if raw_code.len() > 5_000_000 {
            eprintln!("Skipping large file {}: {} bytes", rel_str, raw_code.len());
            continue;
        }

        *by_type.entry(rule.extension.clone()).or_insert(0) += 1;

        let processed = process_file(&raw_code, &rel_str, rule, no_format, max_width);

        total_loc += processed.loc;
        files.push(processed);
    }

    ScanResult {
        files,
        total_loc,
        by_type,
    }
}

// ── Internal Processing ──────────────────────────────────────

fn process_file(
    raw_code: &str,
    rel_path: &str,
    rule: &crate::config::FileTypeRule,
    no_format: bool,
    max_width: i32,
) -> ProcessedFile {
    let ext = rule.extension.clone();
    let kind = rule.kind.clone();

    match rule.kind {
        FileKind::Rust => process_rust(raw_code, rel_path, no_format, max_width, ext, kind),
        FileKind::Structured => process_structured(raw_code, rel_path, rule, ext, kind),
        FileKind::Raw => process_raw(raw_code, rel_path, ext, kind),
    }
}

fn process_rust(
    raw_code: &str,
    rel_path: &str,
    no_format: bool,
    max_width: i32,
    extension: String,
    kind: FileKind,
) -> ProcessedFile {
    // 1. Format in RAM (never writes to disk)
    let formatted = if !no_format {
        run_rustfmt(raw_code, max_width)
    } else {
        raw_code.to_string()
    };

    // 2. Strip comments, tests, and empty lines in RAM
    let no_comments = remove_rust_comments(&formatted);
    let no_tests = remove_test_modules(&no_comments);
    let cleaned = remove_empty_lines(&no_tests);

    // 3. Compress into skeleton segment and bodies
    let (hashes, skeleton_segment) = compress_code(&cleaned, rel_path);

    let loc = cleaned.lines().count();
    let byte_size = cleaned.len();

    let bodies: Vec<(String, BodyMeta, String)> = hashes
        .into_iter()
        .map(|(hash, filepath, body, body_loc)| {
            (
                hash,
                BodyMeta {
                    filepath,
                    loc: body_loc,
                    byte_size: body.len(),
                },
                body,
            )
        })
        .collect();

    ProcessedFile {
        rel_path: rel_path.to_string(),
        extension,
        kind,
        loc,
        byte_size,
        code: cleaned,
        skeleton_segment: Some(skeleton_segment),
        bodies,
    }
}

fn process_structured(
    raw_code: &str,
    rel_path: &str,
    rule: &crate::config::FileTypeRule,
    extension: String,
    kind: FileKind,
) -> ProcessedFile {
    let mut code = raw_code.to_string();

    if rule.strip_comments {
        code = strip_comments(&code, &rule.extension);
    }

    if rule.collapse_whitespace {
        code = collapse_whitespace(&code);
    }

    let loc = code.lines().count();
    let byte_size = code.len();

    // Structured files are NOT included in skeleton and have NO bodies extracted
    ProcessedFile {
        rel_path: rel_path.to_string(),
        extension,
        kind,
        loc,
        byte_size,
        code,
        skeleton_segment: None,
        bodies: Vec::new(),
    }
}

fn process_raw(raw_code: &str, rel_path: &str, extension: String, kind: FileKind) -> ProcessedFile {
    let loc = raw_code.lines().count();
    let byte_size = raw_code.len();

    ProcessedFile {
        rel_path: rel_path.to_string(),
        extension,
        kind,
        loc,
        byte_size,
        code: raw_code.to_string(),
        skeleton_segment: None,
        bodies: Vec::new(),
    }
}

// ── Directory Walker ─────────────────────────────────────────

fn walk_directory(dir: &Path, config: &ScanConfig) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_directory_recursive(dir, dir, config, &mut files);
    files.sort();
    files
}

fn walk_directory_recursive(
    base: &Path,
    current: &Path,
    config: &ScanConfig,
    files: &mut Vec<PathBuf>,
) {
    if let Ok(entries) = fs::read_dir(current) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                // Skip blacklisted directories
                if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                    if config.should_skip_dir(dir_name) {
                        continue;
                    }
                }
                walk_directory_recursive(base, &path, config, files);
            } else if config.rule_for(&path).is_some() {
                files.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_scan_processes_rust_and_toml() {
        let dir = tempfile::tempdir().unwrap();
        let config = ScanConfig::default();

        // Create fake files
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let mut rs_file = File::create(src_dir.join("main.rs")).unwrap();
        writeln!(rs_file, "fn main() {{ /* comment */ }}").unwrap();

        let mut toml_file = File::create(dir.path().join("Cargo.toml")).unwrap();
        writeln!(toml_file, "[package]\nname = \"test\" # my crate").unwrap();

        // Ignore unknown files
        let mut lock_file = File::create(dir.path().join("Cargo.lock")).unwrap();
        writeln!(lock_file, "lock data").unwrap();

        let result = scan_directory(dir.path(), &config, false, 350);

        // Should find exactly 2 files (1 .rs, 1 .toml)
        assert_eq!(
            result.files.len(),
            2,
            "Should process 2 files. Got: {}",
            result.files.len()
        );
        assert!(result.by_type.contains_key("rs"), "Should find .rs");
        assert!(result.by_type.contains_key("toml"), "Should find .toml");
        assert!(!result.by_type.contains_key("lock"), "Should ignore .lock");

        // Rust file should have skeleton and bodies
        let rs_processed = result.files.iter().find(|f| f.extension == "rs").unwrap();
        assert!(
            rs_processed.skeleton_segment.is_some(),
            "Rust must have skeleton"
        );
        assert_eq!(
            rs_processed.bodies.len(),
            1,
            "Rust must have 1 body (fn main)"
        );

        // Toml file should have no skeleton/bodies, but comments stripped
        let toml_processed = result.files.iter().find(|f| f.extension == "toml").unwrap();
        assert!(
            toml_processed.skeleton_segment.is_none(),
            "Toml must not have skeleton"
        );
        assert!(
            toml_processed.bodies.is_empty(),
            "Toml must not have bodies"
        );
        assert!(
            !toml_processed.code.contains("# my crate"),
            "Toml should strip comments. Got: {}",
            toml_processed.code
        );
    }

    #[test]
    fn test_skips_target_dir() {
        let dir = tempfile::tempdir().unwrap();
        let config = ScanConfig::default();

        // Create a file inside target/
        let target_dir = dir.path().join("target");
        fs::create_dir_all(&target_dir).unwrap();
        let mut rs_file = File::create(target_dir.join("build.rs")).unwrap();
        writeln!(rs_file, "fn build() {{}}").unwrap();

        let result = scan_directory(dir.path(), &config, false, 350);
        assert!(result.files.is_empty(), "Should skip target/ directory");
    }
}
