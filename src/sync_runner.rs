use crate::registry::{RepoEntry, RepoRegistry};
use crate::sync;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::task::JoinSet;

pub async fn sync_all_repos(
    registry: Arc<Mutex<RepoRegistry>>,
    central_dir: PathBuf,
) -> Vec<String> {
    let reg = registry.lock().await;
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

    while let Some(result) = set.join_next().await {
        if let Ok((id, _entry, changed, file_count)) = result {
            all_changed.extend(changed);
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

/// Sync a single repo by ID, regardless of its mode.
pub async fn sync_single_repo(
    registry: Arc<Mutex<RepoRegistry>>,
    central_dir: PathBuf,
    repo_id: &str,
) -> Result<Vec<String>, String> {
    let reg = registry.lock().await;
    let entry = reg.repos.get(repo_id).cloned();
    drop(reg);

    let entry = entry.ok_or_else(|| format!("Repo '{}' not found", repo_id))?;
    if !entry.active {
        return Err(format!("Repo '{}' is not active", repo_id));
    }

    let id = entry.id.clone();
    let result = tokio::task::spawn_blocking(move || sync::sync_repo(&entry, &central_dir))
        .await
        .map_err(|e| format!("Sync task panicked for '{}': {}", repo_id, e))?
        .map_err(|e| format!("Sync failed for '{}': {}", repo_id, e))?;

    let mut changed = Vec::new();
    changed.extend(result.copied.iter().map(|p| format!("{}/{}", id, p)));
    changed.extend(result.updated.iter().map(|p| format!("{}/{}", id, p)));
    changed.extend(result.removed.iter().map(|p| format!("{}/{}", id, p)));

    let file_count = result.total_files_in_source;

    let mut reg = registry.lock().await;
    if let Some(repo_entry) = reg.repos.get_mut(repo_id) {
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

    println!(
        "✅ Synced {}: {} new, {} updated, {} removed ({} total in source)",
        repo_id,
        result.copied.len(),
        result.updated.len(),
        result.removed.len(),
        file_count,
    );

    Ok(changed)
}

fn detect_git_branch(path: &std::path::PathBuf) -> Option<String> {
    let head = path.join(".git/HEAD");
    let content = std::fs::read_to_string(head).ok()?;
    let line = content.lines().next()?;
    line.strip_prefix("ref: refs/heads/").map(|s| s.to_string())
}
