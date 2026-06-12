use super::state::{AppState, RequestLog};
use crate::cache::strip_repo_prefix;
use axum::http::Method;
use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct LogsResponse {
    logs: Vec<RequestLog>,
}

#[derive(Serialize)]
pub struct RepoStats {
    pub repo_id: String,
    pub total_files: usize,
    pub total_loc: usize,
    pub total_bytes: usize,
    pub total_file_requests: u64,
    pub top_files: Vec<FileStatEntry>,
    pub top_hashes: Vec<HashStatEntry>,
}

#[derive(Serialize)]
pub struct FileStatEntry {
    pub filepath: String,
    pub full_path: String,
    pub loc: usize,
    pub byte_size: usize,
    pub requests: u64,
}

#[derive(Serialize)]
pub struct HashStatEntry {
    pub hash: String,
    pub filepath: String,
    pub loc: usize,
    pub requests: u64,
}

/// Middleware that logs every request into AppState.request_log for the dashboard
pub async fn request_logging_middleware(
    method: Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let start = std::time::Instant::now();
    let path = uri.path().to_string();

    let response = next.run(request).await;

    let status = response.status();
    let elapsed = start.elapsed();

    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // Fixed timestamp to be a String instead of u64
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    let log_entry = RequestLog {
        method: method.to_string(),
        path: path.clone(),
        status: status.as_u16(),
        timestamp,
        duration_ms: elapsed.as_millis() as u64, // Added missing field
        user_agent,
    };

    // Fixed: use request_log instead of log_buffer
    {
        let mut logs = state.request_log.lock().await;
        logs.push(log_entry);
        if logs.len() > 200 {
            let drain_count = logs.len() - 200;
            logs.drain(0..drain_count);
        }
    }

    response
}

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

    let mut file_entry = db.files.get(&path);
    let mut actual_path = path.clone();

    if path.ends_with(".rs") {
        if file_entry.is_none() {
            for repo in &repo_ids {
                let candidate = format!("{}/{}", repo, path);
                if db.files.contains_key(&candidate) {
                    file_entry = db.files.get(&candidate);
                    actual_path = candidate;
                    break;
                }
            }
        }
        if let Some(entry) = file_entry {
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
                filepath: strip_repo_prefix(&actual_path, &repo_ids),
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
    // Non-Rust file: serve from central_dir
    let mut full_path = state.central_dir.join(&path);
    if !full_path.starts_with(&state.central_dir) || !full_path.exists() {
        for repo in &repo_ids {
            let candidate_path = state.central_dir.join(repo).join(&path);
            if candidate_path.exists() && candidate_path.starts_with(&state.central_dir) {
                full_path = candidate_path;
                break;
            }
        }
    }
    if !full_path.exists() {
        return build_response(
            StatusCode::NOT_FOUND,
            vec![],
            format!("File {} not found", strip_repo_prefix(&path, &repo_ids)),
        );
    }
    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            let byte_size = content.len();
            let loc = content.lines().count();
            let resp = FileInfoResponse {
                filepath: strip_repo_prefix(&path, &repo_ids),
                loc,
                byte_size,
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
            format!("Failed to read file: {}", e),
        ),
    }
}

pub async fn get_file(Path(path): Path<String>, State(state): State<AppState>) -> Response {
    let repo_ids = collect_repo_ids(&state).await;
    let display_path = strip_repo_prefix(&path, &repo_ids);
    if path.ends_with(".rs") {
        let mut db = state.cache.lock().await;
        let mut resolved_key = if db.files.contains_key(&path) {
            Some(path.clone())
        } else {
            None
        };

        if resolved_key.is_none() {
            for repo in &repo_ids {
                let candidate = format!("{}/{}", repo, path);
                if db.files.contains_key(&candidate) {
                    resolved_key = Some(candidate);
                    break;
                }
            }
        }
        if let Some(key) = resolved_key {
            if let Some(entry) = db.files.get(&key).cloned() {
                // Increment file hit counter
                let hits = db.file_hits.entry(key.clone()).or_insert(0);
                *hits += 1;
                let _ = db.save();

                return build_response(
                    StatusCode::OK,
                    vec![
                        ("content-type", "text/plain; charset=utf-8".to_string()),
                        ("x-loc", entry.loc.to_string()),
                        ("x-byte-size", entry.byte_size.to_string()),
                        ("x-source", "cache".to_string()),
                        ("x-filepath", strip_repo_prefix(&key, &repo_ids)),
                    ],
                    format!(
                        "//--+ file:///{}\n{}",
                        strip_repo_prefix(&key, &repo_ids),
                        entry.code
                    ),
                );
            }
        }
        return build_response(
            StatusCode::NOT_FOUND,
            vec![],
            "Rust file not indexed yet. Run sync first.".to_string(),
        );
    }
    // Non-Rust file
    let mut full_path = state.central_dir.join(&path);
    if !full_path.starts_with(&state.central_dir) || !full_path.exists() {
        for repo in &repo_ids {
            let candidate_path = state.central_dir.join(repo).join(&path);
            if candidate_path.exists() && candidate_path.starts_with(&state.central_dir) {
                full_path = candidate_path;
                break;
            }
        }
    }
    if !full_path.exists() {
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
                    ("x-filepath", display_path.clone()),
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
    let mut db = state.cache.lock().await;
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
        let mut resolved_hash = None;
        if db.bodies.contains_key(h) {
            resolved_hash = Some(h.to_string());
        } else {
            let matches: Vec<String> = db
                .bodies
                .keys()
                .filter(|hash| hash.starts_with(h))
                .cloned()
                .collect();
            if matches.len() == 1 {
                resolved_hash = Some(matches[0].clone());
            }
        }
        if let Some(ref rh) = resolved_hash {
            if let Some(entry) = db.bodies.get(rh).cloned() {
                // Increment hash hit counter
                let hits = db.hash_hits.entry(rh.clone()).or_insert(0);
                *hits += 1;
                let _ = db.save();

                let dp = strip_repo_prefix(&entry.meta.filepath, &repo_ids);
                return build_response(
                    StatusCode::OK,
                    vec![
                        ("content-type", "text/plain; charset=utf-8".to_string()),
                        ("x-loc", entry.meta.loc.to_string()),
                        ("x-byte-size", entry.meta.byte_size.to_string()),
                        ("x-filepath", dp.clone()),
                        ("x-hash", rh.clone()),
                    ],
                    format!("//--+ file:///{}\n{}", dp, entry.body),
                );
            }
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
    let mut matched_hashes = Vec::new();

    for h in hashes {
        if db.bodies.contains_key(h) {
            matched_hashes.push(h.to_string());
            continue;
        }
        let matches: Vec<String> = db
            .bodies
            .keys()
            .filter(|hash| hash.starts_with(h))
            .cloned()
            .collect();
        if matches.len() == 1 {
            matched_hashes.push(matches[0].clone());
        } else if matches.is_empty() {
            not_found.push(h.to_string());
        } else {
            not_found.push(format!("{} (ambiguous)", h));
        }
    }

    if !not_found.is_empty() {
        return build_response(
            StatusCode::BAD_REQUEST,
            vec![],
            format!("Hashes not found: {}", not_found.join(", ")),
        );
    }

    for rh in &matched_hashes {
        if let Some(entry) = db.bodies.get(rh).cloned() {
            total_loc += entry.meta.loc;
            let dp = strip_repo_prefix(&entry.meta.filepath, &repo_ids);
            results.push(format!(
                "//--+ file:///{}\n// Hash: {}\n{}",
                dp, rh, entry.body
            ));

            // Increment count
            let hits = db.hash_hits.entry(rh.clone()).or_insert(0);
            *hits += 1;
        }
    }
    let _ = db.save();

    build_response(
        StatusCode::OK,
        vec![
            ("content-type", "text/plain; charset=utf-8".to_string()),
            ("x-loc", total_loc.to_string()),
        ],
        results.join("\n\n"),
    )
}

pub async fn get_stats(State(state): State<AppState>) -> Response {
    let db = state.cache.lock().await;
    let repo_ids = collect_repo_ids(&state).await;

    let mut repo_stats_map: std::collections::HashMap<String, RepoStats> =
        std::collections::HashMap::new();

    for repo_id in &repo_ids {
        repo_stats_map.insert(
            repo_id.clone(),
            RepoStats {
                repo_id: repo_id.clone(),
                total_files: 0,
                total_loc: 0,
                total_bytes: 0,
                total_file_requests: 0,
                top_files: Vec::new(),
                top_hashes: Vec::new(),
            },
        );
    }

    for (filepath, entry) in &db.files {
        if let Some(repo_id) = filepath.split('/').next() {
            if let Some(stats) = repo_stats_map.get_mut(repo_id) {
                stats.total_files += 1;
                stats.total_loc += entry.loc;
                stats.total_bytes += entry.byte_size;

                let requests = db.file_hits.get(filepath).cloned().unwrap_or(0);
                stats.total_file_requests += requests;

                stats.top_files.push(FileStatEntry {
                    filepath: strip_repo_prefix(filepath, &repo_ids),
                    full_path: filepath.clone(),
                    loc: entry.loc,
                    byte_size: entry.byte_size,
                    requests,
                });
            }
        }
    }

    for (hash, entry) in &db.bodies {
        let filepath = &entry.meta.filepath;
        if let Some(repo_id) = filepath.split('/').next() {
            if let Some(stats) = repo_stats_map.get_mut(repo_id) {
                let requests = db.hash_hits.get(hash).cloned().unwrap_or(0);
                stats.top_hashes.push(HashStatEntry {
                    hash: hash.clone(),
                    filepath: strip_repo_prefix(filepath, &repo_ids),
                    loc: entry.meta.loc,
                    requests,
                });
            }
        }
    }

    let mut result_stats: Vec<RepoStats> = repo_stats_map.into_values().collect();
    for stats in &mut result_stats {
        stats
            .top_files
            .sort_by(|a, b| b.requests.cmp(&a.requests).then_with(|| b.loc.cmp(&a.loc)));
        stats.top_files.truncate(10);

        stats
            .top_hashes
            .sort_by(|a, b| b.requests.cmp(&a.requests).then_with(|| b.loc.cmp(&a.loc)));
        stats.top_hashes.truncate(10);
    }

    result_stats.sort_by(|a, b| a.repo_id.cmp(&b.repo_id));

    match serde_json::to_string_pretty(&result_stats) {
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

/// GET /logs
pub async fn get_logs(State(state): State<AppState>) -> Response {
    let logs = state.request_log.lock().await;
    let resp = LogsResponse { logs: logs.clone() };
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

/// Middleware to log non-log-view HTTP requests
pub async fn log_middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let uri = req.uri().clone();
    let path = uri.path().to_string();

    let user_agent = req
        .headers()
        .get(axum::http::header::USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("Unknown")
        .to_string();

    let is_log_req = path == "/logs" || path == "/dashboard" || path == "/";

    let start = std::time::Instant::now(); // ← needed for duration_ms

    let response = next.run(req).await;

    if !is_log_req {
        let status = response.status().as_u16();
        let timestamp = std::time::SystemTime::now() // ← Fix 1: String, not u64
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();

        let elapsed = start.elapsed(); // ← Fix 2: compute duration

        let log_entry = RequestLog {
            method,
            path,
            status,
            timestamp,
            duration_ms: elapsed.as_millis() as u64, // ← Fix 2: add missing field
            user_agent,
        };

        let mut logs = state.request_log.lock().await; // ← Fix 3&4: request_log, not log_buffer
        logs.push(log_entry);
        if logs.len() > 200 {
            let drain_count = logs.len() - 200;
            logs.drain(0..drain_count);
        }
    }

    response
}

/// GET /dashboard
pub async fn get_dashboard() -> Response {
    build_response(
        StatusCode::OK,
        vec![("content-type", "text/html; charset=utf-8".to_string())],
        super::html::DASHBOARD_HTML.to_string(),
    )
}
