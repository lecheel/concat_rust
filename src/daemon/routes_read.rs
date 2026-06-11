use super::state::AppState;
use crate::cache::strip_repo_prefix;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

async fn collect_repo_ids(state: &AppState) -> Vec<String> {
    let reg = state.registry.lock().await;
    reg.repos.keys().cloned().collect()
}

pub fn build_response(status: StatusCode, headers: Vec<(&str, String)>, body: String) -> Response {
    let mut response = (status, body).into_response();
    for (key, value) in headers {
        if let Ok(name) = key.parse::<axum::http::header::HeaderName>() {
            if let Ok(val) = value.parse::<HeaderValue>() {
                response.headers_mut().insert(name, val);
            }
        }
    }
    response
}

#[derive(Deserialize, Debug)]
pub struct SkeletonQuery {
    pub repo: Option<String>,
}

#[derive(Serialize)]
pub struct BodyInfoResponse {
    pub hash: String,
    pub filepath: String,
    pub loc: usize,
    pub byte_size: usize,
}

#[derive(Serialize)]
pub struct FileInfoResponse {
    pub filepath: String,
    pub loc: usize,
    pub byte_size: usize,
    pub body_hashes: Vec<BodyInfoResponse>,
    pub source: String,
}

#[derive(Serialize)]
pub struct CatalogFileSummary {
    pub filepath: String,
    pub loc: usize,
    pub num_bodies: usize,
    pub top_hashes: Vec<BodyInfoResponse>,
    pub file_type: String,
    pub source: String,
}

#[derive(Serialize)]
pub struct CatalogResponse {
    pub files: Vec<CatalogFileSummary>,
    pub total_loc: usize,
    pub total_bodies: usize,
}

pub async fn get_skeleton(
    State(state): State<AppState>,
    Query(params): Query<SkeletonQuery>,
) -> Response {
    let db = state.cache.lock().await;
    let repo_ids = collect_repo_ids(&state).await;
    let repo_param = params.repo.unwrap_or_else(|| "all".to_string());
    let skeleton =
        db.assemble_full_skeleton_response(state.daemon_port, repo_param.as_str(), &repo_ids);
    let loc = skeleton.lines().count();
    build_response(
        StatusCode::OK,
        vec![("x-loc", loc.to_string()), ("x-repo", repo_param)],
        skeleton,
    )
}

pub async fn get_catalog(State(state): State<AppState>) -> Response {
    let db = state.cache.lock().await;
    let repo_ids = collect_repo_ids(&state).await;
    let mut files = Vec::new();
    let mut total_loc = 0;
    let mut total_bodies = 0;
    for (filepath, entry) in &db.files {
        total_loc += entry.loc;
        total_bodies += entry.body_hashes.len();
        let mut body_infos: Vec<BodyInfoResponse> = entry
            .body_hashes
            .iter()
            .filter_map(|h| {
                db.bodies.get(h).map(|e| BodyInfoResponse {
                    hash: h.clone(),
                    filepath: strip_repo_prefix(&e.meta.filepath, &repo_ids),
                    loc: e.meta.loc,
                    byte_size: e.meta.byte_size,
                })
            })
            .collect();
        body_infos.sort_by(|a, b| b.loc.cmp(&a.loc));
        body_infos.truncate(5);
        files.push(CatalogFileSummary {
            filepath: strip_repo_prefix(filepath, &repo_ids),
            loc: entry.loc,
            num_bodies: entry.body_hashes.len(),
            top_hashes: body_infos,
            file_type: "rust".to_string(),
            source: "cache".to_string(),
        });
    }
    files.sort_by(|a, b| b.loc.cmp(&a.loc));
    let resp = CatalogResponse {
        files,
        total_loc,
        total_bodies,
    };
    match serde_json::to_string_pretty(&resp) {
        Ok(json) => build_response(
            StatusCode::OK,
            vec![("content-type", "application/json".to_string())],
            json,
        ),
        Err(e) => build_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            vec![],
            format!("{{\"error\": \"{}\"}}", e),
        ),
    }
}

pub async fn get_body_info(Path(prefix): Path<String>, State(state): State<AppState>) -> Response {
    let db = state.cache.lock().await;
    let repo_ids = collect_repo_ids(&state).await;
    let hashes: Vec<&str> = prefix
        .split(|c| c == '+' || c == ',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let mut infos = Vec::new();
    for h in hashes {
        if let Some(entry) = db.bodies.get(h) {
            infos.push(BodyInfoResponse {
                hash: h.to_string(),
                filepath: strip_repo_prefix(&entry.meta.filepath, &repo_ids),
                loc: entry.meta.loc,
                byte_size: entry.meta.byte_size,
            });
        } else {
            let matches: Vec<_> = db
                .bodies
                .iter()
                .filter(|(hash, _)| hash.starts_with(h))
                .collect();
            match matches.len() {
                0 => {
                    return build_response(
                        StatusCode::NOT_FOUND,
                        vec![],
                        format!("No hash found matching '{}'", h),
                    );
                }
                1 => {
                    let (hash, entry) = matches[0];
                    infos.push(BodyInfoResponse {
                        hash: hash.clone(),
                        filepath: strip_repo_prefix(&entry.meta.filepath, &repo_ids),
                        loc: entry.meta.loc,
                        byte_size: entry.meta.byte_size,
                    });
                }
                _ => {
                    let list: Vec<String> = matches
                        .iter()
                        .map(|(h, e)| {
                            format!(
                                "  {}  {} LOC  ({})",
                                h,
                                e.meta.loc,
                                strip_repo_prefix(&e.meta.filepath, &repo_ids)
                            )
                        })
                        .collect();
                    return build_response(
                        StatusCode::CONFLICT,
                        vec![],
                        format!(
                            "Ambiguous prefix '{}' matches {} hashes:\n{}",
                            h,
                            matches.len(),
                            list.join("\n")
                        ),
                    );
                }
            }
        }
    }
    match serde_json::to_string_pretty(&infos) {
        Ok(json) => build_response(
            StatusCode::OK,
            vec![("content-type", "application/json".to_string())],
            json,
        ),
        Err(e) => build_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            vec![],
            format!("Error: {}", e),
        ),
    }
}

pub async fn get_file_info(Path(path): Path<String>, State(state): State<AppState>) -> Response {
    let db = state.cache.lock().await;
    let repo_ids = collect_repo_ids(&state).await;
    if path.ends_with(".rs") {
        if let Some(entry) = db.files.get(&path) {
            let body_infos: Vec<BodyInfoResponse> = entry
                .body_hashes
                .iter()
                .filter_map(|h| {
                    db.bodies.get(h).map(|e| BodyInfoResponse {
                        hash: h.clone(),
                        filepath: strip_repo_prefix(&e.meta.filepath, &repo_ids),
                        loc: e.meta.loc,
                        byte_size: e.meta.byte_size,
                    })
                })
                .collect();
            let resp = FileInfoResponse {
                filepath: strip_repo_prefix(&path, &repo_ids),
                loc: entry.loc,
                byte_size: entry.byte_size,
                body_hashes: body_infos,
                source: "cache".to_string(),
            };
            return match serde_json::to_string_pretty(&resp) {
                Ok(json) => build_response(
                    StatusCode::OK,
                    vec![("content-type", "application/json".to_string())],
                    json,
                ),
                Err(e) => build_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    vec![],
                    format!("Error: {}", e),
                ),
            };
        }
        return build_response(
            StatusCode::NOT_FOUND,
            vec![],
            format!(
                "Rust file {} not in cache",
                strip_repo_prefix(&path, &repo_ids)
            ),
        );
    }
    let full_path = state.central_dir.join(&path);
    if !full_path.starts_with(&state.central_dir) || !full_path.exists() {
        return build_response(
            StatusCode::NOT_FOUND,
            vec![],
            format!("File {} not found", strip_repo_prefix(&path, &repo_ids)),
        );
    }
    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            let resp = FileInfoResponse {
                filepath: strip_repo_prefix(&path, &repo_ids),
                loc: content.lines().count(),
                byte_size: content.len(),
                body_hashes: vec![],
                source: "disk".to_string(),
            };
            match serde_json::to_string_pretty(&resp) {
                Ok(json) => build_response(
                    StatusCode::OK,
                    vec![("content-type", "application/json".to_string())],
                    json,
                ),
                Err(e) => build_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    vec![],
                    format!("Error: {}", e),
                ),
            }
        }
        Err(e) => build_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            vec![],
            format!("Read error: {}", e),
        ),
    }
}

// === src/daemon/routes_read.rs — get_file function, two fixes ===

pub async fn get_file(Path(path): Path<String>, State(state): State<AppState>) -> Response {
    let repo_ids = collect_repo_ids(&state).await;
    let display_path = strip_repo_prefix(&path, &repo_ids);
    if path.ends_with(".rs") {
        let db = state.cache.lock().await;
        if let Some(entry) = db.files.get(&path) {
            return build_response(
                StatusCode::OK,
                vec![
                    ("content-type", "text/plain; charset=utf-8".to_string()),
                    ("x-loc", entry.loc.to_string()),
                    ("x-byte-size", entry.byte_size.to_string()),
                    ("x-source", "cache".to_string()),
                    ("x-filepath", display_path.clone()), // <-- .clone() added
                ],
                format!("//--+ file:///{}\n{}", display_path, entry.code),
            );
        }
        return build_response(
            StatusCode::NOT_FOUND,
            vec![],
            "Rust file not indexed yet. Run sync first.".to_string(),
        );
    }
    let full_path = state.central_dir.join(&path);
    if !full_path.starts_with(&state.central_dir) || !full_path.exists() {
        return build_response(
            StatusCode::NOT_FOUND,
            vec![],
            format!("File not found: {}", display_path),
        );
    }
    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            let loc = content.lines().count();
            let ext = path.rsplit('.').next().unwrap_or("txt");
            let content_type = match ext {
                "json" => "application/json",
                "yml" | "yaml" => "text/yaml",
                "toml" => "text/plain",
                "sql" => "text/plain",
                "md" => "text/markdown",
                _ => "text/plain",
            };
            build_response(
                StatusCode::OK,
                vec![
                    ("content-type", content_type.to_string()),
                    ("x-loc", loc.to_string()),
                    ("x-byte-size", content.len().to_string()),
                    ("x-source", "disk".to_string()),
                    ("x-filepath", display_path.clone()), // <-- .clone() added
                ],
                format!("//--+ file:///{}\n{}", display_path, content),
            )
        }
        Err(e) => build_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            vec![],
            format!("Failed to read {}: {}", display_path, e),
        ),
    }
}

pub async fn get_body(Path(prefix): Path<String>, State(state): State<AppState>) -> Response {
    let db = state.cache.lock().await;
    let repo_ids = collect_repo_ids(&state).await;
    let hashes: Vec<&str> = prefix
        .split(|c: char| c == '+' || c == ',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if hashes.is_empty() {
        return build_response(
            StatusCode::BAD_REQUEST,
            vec![],
            "No hashes provided".to_string(),
        );
    }
    if hashes.len() == 1 {
        let h = hashes[0];
        if let Some(entry) = db.bodies.get(h) {
            let dp = strip_repo_prefix(&entry.meta.filepath, &repo_ids);
            return build_response(
                StatusCode::OK,
                vec![
                    ("content-type", "text/plain; charset=utf-8".to_string()),
                    ("x-loc", entry.meta.loc.to_string()),
                    ("x-byte-size", entry.meta.byte_size.to_string()),
                    ("x-filepath", dp.clone()),
                    ("x-hash", h.to_string()),
                ],
                format!("//--+ file:///{}\n{}", dp, entry.body),
            );
        }
        let matches: Vec<_> = db
            .bodies
            .iter()
            .filter(|(hash, _)| hash.starts_with(h))
            .collect();
        return match matches.len() {
            0 => build_response(
                StatusCode::NOT_FOUND,
                vec![],
                format!("No hash found matching prefix '{}'", h),
            ),
            1 => {
                let (hash, entry) = matches[0];
                let dp = strip_repo_prefix(&entry.meta.filepath, &repo_ids);
                build_response(
                    StatusCode::OK,
                    vec![
                        ("content-type", "text/plain; charset=utf-8".to_string()),
                        ("x-loc", entry.meta.loc.to_string()),
                        ("x-byte-size", entry.meta.byte_size.to_string()),
                        ("x-filepath", dp.clone()),
                        ("x-hash", hash.clone()),
                    ],
                    format!("//--+ file:///{}\n// Hash: {}\n{}", dp, hash, entry.body),
                )
            }
            _ => {
                let list: Vec<String> = matches
                    .iter()
                    .map(|(h, e)| {
                        format!(
                            "  {} {} LOC ({})",
                            h,
                            e.meta.loc,
                            strip_repo_prefix(&e.meta.filepath, &repo_ids)
                        )
                    })
                    .collect();
                build_response(
                    StatusCode::CONFLICT,
                    vec![],
                    format!(
                        "Ambiguous prefix '{}' matches {} hashes:\n{}",
                        h,
                        matches.len(),
                        list.join("\n")
                    ),
                )
            }
        };
    }
    let mut results = Vec::new();
    let mut total_loc = 0usize;
    let mut not_found = Vec::new();
    for h in hashes {
        if let Some(entry) = db.bodies.get(h) {
            total_loc += entry.meta.loc;
            let dp = strip_repo_prefix(&entry.meta.filepath, &repo_ids);
            results.push(format!(
                "//--+ file:///{}\n// Hash: {}\n{}",
                dp, h, entry.body
            ));
            continue;
        }
        let matches: Vec<_> = db
            .bodies
            .iter()
            .filter(|(hash, _)| hash.starts_with(h))
            .collect();
        match matches.len() {
            0 => not_found.push(h.to_string()),
            1 => {
                let (hash, entry) = matches[0];
                total_loc += entry.meta.loc;
                let dp = strip_repo_prefix(&entry.meta.filepath, &repo_ids);
                results.push(format!(
                    "//--+ file:///{}\n// Hash: {}\n{}",
                    dp, hash, entry.body
                ));
            }
            _ => {
                not_found.push(format!("{} (ambiguous)", h));
            }
        }
    }
    if !not_found.is_empty() {
        return build_response(
            StatusCode::BAD_REQUEST,
            vec![],
            format!("Hashes not found: {}", not_found.join(", ")),
        );
    }
    build_response(
        StatusCode::OK,
        vec![
            ("content-type", "text/plain; charset=utf-8".to_string()),
            ("x-loc", total_loc.to_string()),
        ],
        results.join("\n\n"),
    )
}
