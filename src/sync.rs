//--+ src/sync.rs

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::registry::RepoEntry;

/// Patterns to ALWAYS skip when syncing
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".next",
    "dist",
    "build",
    "__pycache__",
    ".idea",
    ".vscode",
    ".DS_Store",
];

const SKIP_EXTENSIONS: &[&str] = &[
    "lock", "pyc", "o", "so", "dylib", "dll", "exe", "pdb", "class", "jar", "wasm",
];

pub struct SyncResult {
    pub copied: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
    pub total_files_in_source: usize,
}

/// Sync one repo from its source into the central dir.
/// Returns what changed so the caller can decide what to reindex.
pub fn sync_repo(
    entry: &RepoEntry,
    central_dir: &Path,
) -> Result<SyncResult, Box<dyn std::error::Error + Send + Sync>> {
    let source = &entry.source_path;
    let dest = central_dir.join(&entry.id);

    if !source.is_dir() {
        return Err(format!("Source path does not exist: {}", source.display()).into());
    }

    std::fs::create_dir_all(&dest)?;

    // Phase 1: Walk source, copy new/changed files
    let source_files = walk_source(source)?;
    let mut copied = Vec::new();
    let mut updated = Vec::new();

    for rel_path in &source_files {
        let src_file = source.join(rel_path);
        let dst_file = dest.join(rel_path);

        let existed_before = dst_file.exists();
        let needs_copy = if existed_before {
            files_differ(&src_file, &dst_file)
        } else {
            true
        };

        if needs_copy {
            if let Some(parent) = dst_file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src_file, &dst_file)?;

            if existed_before {
                updated.push(rel_path.to_string_lossy().to_string());
            } else {
                copied.push(rel_path.to_string_lossy().to_string());
            }
        }
    }

    // Phase 2: Walk dest, remove files that no longer exist in source
    let dest_files = walk_dest(&dest)?;
    let source_set: HashSet<String> = source_files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    let mut removed = Vec::new();
    for rel_path in &dest_files {
        if !source_set.contains(&rel_path.to_string_lossy().to_string()) {
            let dst_file = dest.join(rel_path);
            let _ = std::fs::remove_file(&dst_file);
            removed.push(rel_path.to_string_lossy().to_string());
        }
    }

    // Phase 3: Clean up empty directories in dest
    remove_empty_dirs(&dest)?;

    Ok(SyncResult {
        copied,
        updated,
        removed,
        total_files_in_source: source_files.len(),
    })
}

/// Walk source repo, collecting relative paths of files to sync
fn walk_source(source: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error + Send + Sync>> {
    let mut files = Vec::new();
    walk_source_recursive(source, source, &mut files)?;
    Ok(files)
}

fn walk_source_recursive(
    base: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for entry in std::fs::read_dir(current)? {
        let path = entry?.path();

        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if SKIP_DIRS.contains(&name) {
                    continue;
                }
            }
            walk_source_recursive(base, &path, files)?;
            continue;
        }

        // Skip by extension
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if SKIP_EXTENSIONS.contains(&ext) {
                continue;
            }
        }

        // Skip hidden files (dot prefix) except .env and .env.example
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') && name != ".env" && name != ".env.example" {
                continue;
            }
        }

        // File size sanity check — skip huge files
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > 5_000_000 {
                continue;
            }
        }

        let rel = path.strip_prefix(base)?.to_path_buf();
        files.push(rel);
    }
    Ok(())
}

/// Walk the destination (central copy) to find what's there
fn walk_dest(dest: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error + Send + Sync>> {
    let mut files = Vec::new();
    walk_dest_recursive(dest, dest, &mut files)?;
    Ok(files)
}

fn walk_dest_recursive(
    base: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for entry in std::fs::read_dir(current)? {
        let path = entry?.path();
        if path.is_dir() {
            walk_dest_recursive(base, &path, files)?;
        } else {
            let rel = path.strip_prefix(base)?.to_path_buf();
            files.push(rel);
        }
    }
    Ok(())
}

/// Compare two files — mtime first, then size, then assume changed.
fn files_differ(a: &Path, b: &Path) -> bool {
    let size_a = std::fs::metadata(a).map(|m| m.len()).unwrap_or(0);
    let size_b = std::fs::metadata(b).map(|m| m.len()).unwrap_or(0);
    if size_a != size_b {
        return true;
    }

    let mtime_a = get_mtime(a);
    let mtime_b = get_mtime(b);

    // If source is newer, they differ
    match (mtime_a, mtime_b) {
        (Some(a), Some(b)) => a > b,
        _ => true, // Can't determine, copy to be safe
    }
}

fn get_mtime(path: &Path) -> Option<u128> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_nanos())
}

/// Remove empty directories bottom-up
fn remove_empty_dirs(dir: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut dirs_to_check = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            remove_empty_dirs(&path)?;
            dirs_to_check.push(path);
        }
    }

    for d in dirs_to_check {
        let is_empty = std::fs::read_dir(&d)?.next().is_none();
        if is_empty {
            std::fs::remove_dir(&d)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_sync_copies_files_and_skips_junk() {
        let source_dir = tempfile::tempdir().unwrap();
        let central_dir = tempfile::tempdir().unwrap();

        // Create source structure
        let src = source_dir.path();
        fs::create_dir_all(src.join("src")).unwrap();
        fs::create_dir_all(src.join("target")).unwrap();

        let mut main_rs = File::create(src.join("src/main.rs")).unwrap();
        writeln!(main_rs, "fn main() {{}}").unwrap();

        let mut lock = File::create(src.join("Cargo.lock")).unwrap();
        writeln!(lock, "lock data").unwrap();

        let mut target_file = File::create(src.join("target/debug.bin")).unwrap();
        writeln!(target_file, "binary").unwrap();

        let entry = RepoEntry {
            id: "test_repo".to_string(),
            source_path: src.to_path_buf(),
            active: true,
            last_sync: None,
            git_branch: None,
            file_count: None,
        };

        let result = sync_repo(&entry, central_dir.path()).unwrap();

        // Should copy exactly 1 file (src/main.rs)
        assert_eq!(
            result.total_files_in_source, 1,
            "Should only count valid files"
        );
        assert!(
            result.copied.contains(&"src/main.rs".to_string()),
            "Should copy main.rs"
        );

        // Verify central dir has the file
        let central_file = central_dir.path().join("test_repo/src/main.rs");
        assert!(central_file.exists(), "File should exist in central mirror");

        // Verify junk wasn't copied
        assert!(!central_dir.path().join("test_repo/Cargo.lock").exists());
        assert!(!central_dir.path().join("test_repo/target").exists());
    }

    #[test]
    fn test_sync_deletes_removed_files() {
        let source_dir = tempfile::tempdir().unwrap();
        let central_dir = tempfile::tempdir().unwrap();
        let src = source_dir.path();

        // 1. Initial sync with two files
        fs::create_dir_all(src.join("src")).unwrap();
        File::create(src.join("src/main.rs")).unwrap();
        File::create(src.join("src/old.rs")).unwrap();

        let entry = RepoEntry {
            id: "repo".to_string(),
            source_path: src.to_path_buf(),
            active: true,
            last_sync: None,
            git_branch: None,
            file_count: None,
        };

        sync_repo(&entry, central_dir.path()).unwrap();
        assert!(central_dir.path().join("repo/src/old.rs").exists());

        // 2. Delete old.rs from source and re-sync
        fs::remove_file(src.join("src/old.rs")).unwrap();
        let result = sync_repo(&entry, central_dir.path()).unwrap();

        assert!(result.removed.contains(&"src/old.rs".to_string()));
        assert!(
            !central_dir.path().join("repo/src/old.rs").exists(),
            "Should be deleted from mirror"
        );
    }
}
