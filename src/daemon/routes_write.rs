// === src/daemon/routes_write.rs ===
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};

use super::state::AppState;
use crate::config::ScanConfig;
use crate::scanner;
use crate::sync_runner;

#[derive(Serialize)]
pub struct RepoSummary {
    pub id: String,
    pub source_path: String,
    pub active: bool,
    pub last_sync: Option<u64>,
    pub git_branch: Option<String>,
    pub file_count: Option<usize>,
}

#[derive(Deserialize)]
pub struct AddRepoRequest {
    pub id: String,
    pub source_path: String,
}

/// GET /repos
pub async fn get_repos(State(state): State<AppState>) -> Response {
    let reg = state.registry.lock().await;
    let repos: Vec<RepoSummary> = reg
        .repos
        .values()
        .map(|e| RepoSummary {
            id: e.id.clone(),
            source_path: e.source_path.to_string_lossy().to_string(),
            active: e.active,
            last_sync: e.last_sync,
            git_branch: e.git_branch.clone(),
            file_count: e.file_count,
        })
        .collect();

    match serde_json::to_string_pretty(&repos) {
        Ok(json) => super::routes_read::build_response(
            StatusCode::OK,
            vec![("content-type", "application/json".to_string())],
            json,
        ),
        Err(e) => super::routes_read::build_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            vec![],
            format!("JSON error: {}", e),
        ),
    }
}

pub async fn post_repo_add(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<AddRepoRequest>,
) -> Response {
    let source = std::path::PathBuf::from(&req.source_path);
    let source = match source.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return super::routes_read::build_response(
                StatusCode::BAD_REQUEST,
                vec![],
                format!("Source path does not exist: {}", req.source_path),
            );
        }
    };
    if !source.is_dir() {
        return super::routes_read::build_response(
            StatusCode::BAD_REQUEST,
            vec![],
            format!("Source path is not a directory: {}", req.source_path),
        );
    }

    // check if source path is already registered under a different repo ID
    {
        let reg = state.registry.lock().await;
        for (existing_id, entry) in &reg.repos {
            if entry.source_path == source && existing_id != &req.id {
                return super::routes_read::build_response(
                    StatusCode::CONFLICT,
                    vec![],
                    format!(
                        "❌ Source path '{}' is already registered as repo '{}'. Remove it first if you want to re-register.",
                        source.display(),
                        existing_id
                    ),
                );
            }
        }
    }

    let mut reg = state.registry.lock().await;
    let is_new = reg.add_repo(&req.id, source.clone());

    if let Err(e) = reg.save() {
        return super::routes_read::build_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            vec![],
            format!("Failed to save registry: {}", e),
        );
    }

    drop(reg); // Release lock before sync

    // Auto-sync so files are immediately available
    let _ = sync_runner::sync_all_repos(state.registry.clone(), state.central_dir.clone()).await;

    // Reindex
    let mut db = state.cache.lock().await;
    let config = ScanConfig::default();
    let scan_result = scanner::scan_directory(
        &state.central_dir,
        &config,
        state.no_format,
        state.max_width,
    );

    let mut found_rel_paths = std::collections::HashSet::new();
    for file in scan_result.files {
        let rel_str = file.rel_path.clone();
        found_rel_paths.insert(rel_str.clone());

        if rel_str.ends_with(".rs") {
            let body_hashes: Vec<String> = file.bodies.iter().map(|(h, _, _)| h.clone()).collect();
            for (hash, meta, body) in file.bodies {
                db.bodies
                    .insert(hash, crate::cache::BodyEntry { meta, body });
            }
            db.files.insert(
                rel_str.clone(),
                crate::cache::FileEntry {
                    loc: file.loc,
                    byte_size: file.byte_size,
                    body_hashes,
                    code: file.code,
                },
            );
            if let Some(segment) = file.skeleton_segment {
                db.skeleton_segments.insert(rel_str.clone(), segment);
            }
            if !db.file_order.contains(&rel_str) {
                db.file_order.push(rel_str);
            }
        }
    }

    // Evict stale entries
    let stale_files: Vec<String> = db
        .files
        .keys()
        .filter(|k| !found_rel_paths.contains(*k))
        .cloned()
        .collect();
    for path in &stale_files {
        db.evict_file(path);
    }
    db.file_order.retain(|p| found_rel_paths.contains(p));
    db.generation += 1;
    let _ = db.save();

    let verb = if is_new { "Registered" } else { "Updated" };
    super::routes_read::build_response(
        StatusCode::OK,
        vec![],
        format!(
            "✅ {} & synced '{}' → {} ({} files)",
            verb,
            req.id,
            source.display(),
            found_rel_paths.len()
        ),
    )
}

/// DELETE /repos/:id
pub async fn delete_repo(Path(id): Path<String>, State(state): State<AppState>) -> Response {
    let mut reg = state.registry.lock().await;

    if !reg.repos.contains_key(&id) {
        return super::routes_read::build_response(
            StatusCode::NOT_FOUND,
            vec![],
            format!("Repo '{}' not found", id),
        );
    }

    reg.remove_repo(&id);

    if let Err(e) = reg.save() {
        return super::routes_read::build_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            vec![],
            format!("Failed to save registry: {}", e),
        );
    }

    super::routes_read::build_response(StatusCode::OK, vec![], format!("Removed repo '{}'", id))
}

/// POST /sync
pub async fn post_sync(State(state): State<AppState>) -> Response {
    let changed =
        sync_runner::sync_all_repos(state.registry.clone(), state.central_dir.clone()).await;

    let mut db = state.cache.lock().await;

    for path in &changed {
        db.evict_file(path);
    }

    let config = ScanConfig::default();
    let scan_result = scanner::scan_directory(
        &state.central_dir,
        &config,
        state.no_format,
        state.max_width,
    );

    let mut found_rel_paths = std::collections::HashSet::new();

    for file in scan_result.files {
        let rel_str = file.rel_path.clone();
        found_rel_paths.insert(rel_str.clone());

        if rel_str.ends_with(".rs") {
            let body_hashes: Vec<String> = file.bodies.iter().map(|(h, _, _)| h.clone()).collect();
            for (hash, meta, body) in file.bodies {
                db.bodies
                    .insert(hash, crate::cache::BodyEntry { meta, body });
            }
            db.files.insert(
                rel_str.clone(),
                crate::cache::FileEntry {
                    loc: file.loc,
                    byte_size: file.byte_size,
                    body_hashes,
                    code: file.code,
                },
            );
            if let Some(segment) = file.skeleton_segment {
                db.skeleton_segments.insert(rel_str.clone(), segment);
            }
            if !db.file_order.contains(&rel_str) {
                db.file_order.push(rel_str);
            }
        }
    }

    let stale_files: Vec<String> = db
        .files
        .keys()
        .filter(|k| !found_rel_paths.contains(*k))
        .cloned()
        .collect();
    for path in &stale_files {
        db.evict_file(path);
    }
    db.file_order.retain(|p| found_rel_paths.contains(p));

    db.generation += 1;
    let _ = db.save();

    super::routes_read::build_response(
        StatusCode::OK,
        vec![],
        format!(
            "Synced. {} changed, {} stale evicted (gen {})",
            changed.len(),
            stale_files.len(),
            db.generation
        ),
    )
}
