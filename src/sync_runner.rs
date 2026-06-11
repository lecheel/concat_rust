//--+ src/sync_runner.rs

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;
use tokio::task::JoinSet;

use crate::registry::{RepoEntry, RepoRegistry};
use crate::sync;

/// Run one sync pass for all active repos concurrently.
/// Returns a list of repo-prefixed paths that changed (e.g., "core/src/main.rs").
pub async fn sync_all_repos(
    registry: Arc<Mutex<RepoRegistry>>,
    central_dir: PathBuf,
) -> Vec<String> {
    let reg = registry.lock().await;

    // Use .values() to get RepoEntry directly, then clone them
    let active_repos: Vec<RepoEntry> = reg.repos.values().filter(|e| e.active).cloned().collect();

    drop(reg);

    let mut set = JoinSet::new();
    let mut all_changed = Vec::new();

    for entry in active_repos {
        let id = entry.id.clone();
        let central = central_dir.clone();

        set.spawn(async move {
            let entry_clone = entry.clone();
            let result =
                tokio::task::spawn_blocking(move || sync::sync_repo(&entry_clone, &central)).await;

            match result {
                Ok(Ok(sync_result)) => {
                    let mut changed = Vec::new();
                    changed.extend(sync_result.copied.iter().map(|p| format!("{}/{}", id, p)));
                    changed.extend(sync_result.updated.iter().map(|p| format!("{}/{}", id, p)));
                    changed.extend(sync_result.removed.iter().map(|p| format!("{}/{}", id, p)));

                    println!(
                        "✅ Synced {}: {} new, {} updated, {} removed ({} total in source)",
                        id,
                        sync_result.copied.len(),
                        sync_result.updated.len(),
                        sync_result.removed.len(),
                        sync_result.total_files_in_source,
                    );

                    (id, entry, changed, sync_result.total_files_in_source)
                }
                Ok(Err(e)) => {
                    eprintln!("❌ Sync failed for {}: {}", id, e);
                    (id, entry, Vec::new(), 0)
                }
                Err(e) => {
                    eprintln!("❌ Sync task panicked for {}: {}", id, e);
                    (id, entry, Vec::new(), 0)
                }
            }
        });
    }

    // Collect results and update registry timestamps
    while let Some(result) = set.join_next().await {
        if let Ok((id, _entry, changed, file_count)) = result {
            all_changed.extend(changed);

            // Update registry timestamps
            let mut reg = registry.lock().await;
            if let Some(repo_entry) = reg.repos.get_mut(&id) {
                repo_entry.last_sync = Some(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                );
                repo_entry.file_count = Some(file_count);
                repo_entry.git_branch = detect_git_branch(&repo_entry.source_path);
            }
            let _ = reg.save();
        }
    }

    all_changed
}

fn detect_git_branch(path: &std::path::PathBuf) -> Option<String> {
    let head = path.join(".git/HEAD");
    let content = std::fs::read_to_string(head).ok()?;
    let line = content.lines().next()?;
    line.strip_prefix("ref: refs/heads/").map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn test_sync_all_repos() {
        let central_dir = tempfile::tempdir().unwrap();
        let central_path = central_dir.path().to_path_buf();

        let mut registry = RepoRegistry::load_or_create(&central_path);

        // Create two fake source repos
        let repo1_dir = tempfile::tempdir().unwrap();
        let repo2_dir = tempfile::tempdir().unwrap();

        fs::create_dir_all(repo1_dir.path().join("src")).unwrap();
        fs::write(repo1_dir.path().join("src/main.rs"), "fn main() {}").unwrap();

        fs::create_dir_all(repo2_dir.path().join("src")).unwrap();
        fs::write(repo2_dir.path().join("src/lib.rs"), "fn lib() {}").unwrap();

        registry.add_repo("repo1", repo1_dir.path().to_path_buf());
        registry.add_repo("repo2", repo2_dir.path().to_path_buf());
        registry.save().unwrap();

        let reg_arc = Arc::new(Mutex::new(registry));

        let changed = sync_all_repos(reg_arc.clone(), central_path.clone()).await;

        // Should report 2 files changed (copied)
        assert_eq!(changed.len(), 2, "Should find 2 changed files");
        assert!(changed.contains(&"repo1/src/main.rs".to_string()));
        assert!(changed.contains(&"repo2/src/lib.rs".to_string()));

        // Verify files exist in central
        assert!(central_dir.path().join("repo1/src/main.rs").exists());
        assert!(central_dir.path().join("repo2/src/lib.rs").exists());

        // Verify registry was updated
        let reg = reg_arc.lock().await;
        assert!(reg.repos.get("repo1").unwrap().last_sync.is_some());
    }
}
