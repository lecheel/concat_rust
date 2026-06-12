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
        let db = state.cache.lock().await;
        let mut file_entry = db.files.get(&path);
        let mut actual_path = path.clone();

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
            return build_response(
                StatusCode::OK,
                vec![
                    ("content-type", "text/plain; charset=utf-8".to_string()),
                    ("x-loc", entry.loc.to_string()),
                    ("x-byte-size", entry.byte_size.to_string()),
                    ("x-source", "cache".to_string()),
                    ("x-filepath", strip_repo_prefix(&actual_path, &repo_ids)),
                ],
                format!(
                    "//--+ file:///{}\n{}",
                    strip_repo_prefix(&actual_path, &repo_ids),
                    entry.code
                ),
            );
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
        DASHBOARD_HTML.to_string(),
    )
}

const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en" class="h-full bg-slate-950 text-slate-100">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Concat Rust — Daemon Dashboard</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <style>
        ::-webkit-scrollbar {
            width: 6px;
            height: 6px;
        }
        ::-webkit-scrollbar-track {
            background: #0f172a;
        }
        ::-webkit-scrollbar-thumb {
            background: #334155;
            border-radius: 3px;
        }
        ::-webkit-scrollbar-thumb:hover {
            background: #475569;
        }
    </style>
</head>
<body class="h-full flex flex-col font-sans overflow-hidden">
    <!-- Top Nav Bar -->
    <header class="bg-slate-900 border-b border-slate-800 px-6 py-4 flex items-center justify-between shadow-md shrink-0">
        <div class="flex items-center space-x-3">
            <span class="text-2xl">⚡</span>
            <div>
                <h1 class="text-lg font-bold tracking-tight text-white">Concat Rust</h1>
                <p class="text-xs text-slate-400">Daemon Backend Dashboard</p>
            </div>
        </div>
        <div class="flex items-center space-x-4">
            <div id="connection-status" class="flex items-center space-x-2 text-xs font-semibold bg-emerald-950/50 text-emerald-400 border border-emerald-800/60 px-2.5 py-1 rounded-full">
                <span class="h-2 w-2 rounded-full bg-emerald-400 animate-pulse"></span>
                <span>Daemon Active</span>
            </div>
            <button onclick="syncAll()" class="bg-indigo-600 hover:bg-indigo-500 text-white font-medium text-xs px-3 py-1.5 rounded transition duration-150 shadow flex items-center space-x-1.5">
                <span id="sync-spinner" class="hidden animate-spin h-3.5 w-3.5 border-2 border-white border-t-transparent rounded-full"></span>
                <span>Sync & Reindex All</span>
            </button>
        </div>
    </header>

    <!-- Main Workspace -->
    <main class="flex-1 flex overflow-hidden">
        <!-- Sidebar (Tabs: Repos, Catalog, Activity Logs) -->
        <div class="w-96 border-r border-slate-800 bg-slate-900/40 flex flex-col overflow-hidden shrink-0">
            <!-- Tabs Header -->
            <div class="flex border-b border-slate-800 shrink-0">
                <button onclick="switchTab('repos')" id="tab-btn-repos" class="flex-1 py-3 text-xs font-semibold border-b-2 border-indigo-500 text-white transition">Repos</button>
                <button onclick="switchTab('catalog')" id="tab-btn-catalog" class="flex-1 py-3 text-xs font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition">Catalog</button>
                <button onclick="switchTab('logs')" id="tab-btn-logs" class="flex-1 py-3 text-xs font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition">Activity Logs</button>
            </div>

            <!-- Tab Content Area -->
            <div class="flex-1 overflow-y-auto p-4 space-y-4">
                <!-- REPOS TAB -->
                <div id="tab-content-repos" class="space-y-4">
                    <div class="bg-slate-900/80 rounded-lg p-4 border border-slate-800 space-y-3 shadow-inner">
                        <h3 class="text-xs font-bold uppercase tracking-wider text-slate-400">Add Git Repository</h3>
                        <form id="add-repo-form" onsubmit="handleAddRepo(event)" class="space-y-2.5">
                            <div>
                                <label class="block text-[10px] font-bold text-slate-400 uppercase mb-1">Repo ID (slug)</label>
                                <input type="text" id="repo-id" required placeholder="e.g. core-api" class="w-full bg-slate-950 border border-slate-800 rounded px-2.5 py-1.5 text-xs focus:outline-none focus:border-indigo-500 text-slate-200">
                            </div>
                            <div>
                                <label class="block text-[10px] font-bold text-slate-400 uppercase mb-1">Source Path (Absolute)</label>
                                <input type="text" id="repo-path" required placeholder="e.g. /Users/dev/projects/core-api" class="w-full bg-slate-950 border border-slate-800 rounded px-2.5 py-1.5 text-xs focus:outline-none focus:border-indigo-500 text-slate-200">
                            </div>
                            <button type="submit" class="w-full bg-indigo-600/20 hover:bg-indigo-600/30 text-indigo-400 border border-indigo-500/30 font-medium text-xs py-1.5 rounded transition">
                                Register Repo
                            </button>
                        </form>
                    </div>

                    <div class="space-y-3">
                        <h3 class="text-xs font-bold uppercase tracking-wider text-slate-400 px-1">Registered Mirror Repos</h3>
                        <div id="repos-list" class="space-y-2"></div>
                    </div>
                </div>

                <!-- CATALOG TAB -->
                <div id="tab-content-catalog" class="hidden space-y-3">
                    <div class="sticky top-0 bg-slate-950/90 py-1 shrink-0">
                        <input type="text" id="catalog-search" oninput="filterCatalog()" placeholder="Search catalog paths..." class="w-full bg-slate-950 border border-slate-800 rounded px-3 py-2 text-xs focus:outline-none focus:border-indigo-500 text-slate-200">
                    </div>
                    <div id="catalog-list" class="space-y-1.5"></div>
                </div>

                <!-- ACTIVITY LOGS TAB -->
                <div id="tab-content-logs" class="hidden space-y-3">
                    <div class="flex items-center justify-between px-1">
                        <h3 class="text-xs font-bold uppercase tracking-wider text-slate-400">Daemon Activity Logs</h3>
                        <span class="text-[9px] bg-indigo-950 text-indigo-400 border border-indigo-900 rounded px-1.5 py-0.5 font-semibold">Auto-refresh (2s)</span>
                    </div>
                    <div id="logs-list" class="space-y-2"></div>
                </div>
            </div>
        </div>

        <!-- Dashboard Content Pane (Right side) -->
        <div class="flex-1 flex flex-col overflow-hidden bg-slate-950">
            <!-- Search / Inspector bar -->
            <div class="bg-slate-900/60 border-b border-slate-800/80 px-6 py-3 shrink-0 flex items-center justify-between">
                <div class="flex items-center space-x-2 w-full max-w-lg">
                    <span class="text-xs font-bold text-slate-400 uppercase tracking-wider shrink-0">Hash Inspector:</span>
                    <input type="text" id="hash-search" placeholder="Type HASH prefix (e.g. 5d1f) + Enter..." onkeydown="handleHashSearch(event)" class="flex-1 bg-slate-950 border border-slate-800 rounded-md px-3 py-1.5 text-xs font-mono text-slate-200 focus:outline-none focus:border-indigo-500">
                    <button onclick="inspectHash()" class="bg-slate-800 hover:bg-slate-700 border border-slate-700/60 text-slate-300 px-3 py-1.5 text-xs rounded transition font-medium shrink-0">Inspect</button>
                </div>
                <div class="text-xs text-slate-500 font-medium font-mono" id="right-pane-stats">
                    Select a file or lookup a hash context to inspect
                </div>
            </div>

            <!-- Inspection Output Viewport -->
            <div class="flex-1 flex overflow-hidden">
                <div class="flex-1 flex flex-col overflow-hidden p-6 space-y-4">
                    <div id="metadata-header" class="hidden shrink-0 bg-slate-900/40 border border-slate-800/80 rounded-lg p-4 flex items-center justify-between">
                        <div class="space-y-1">
                            <div class="flex items-center space-x-2">
                                <span id="meta-icon" class="text-base">📄</span>
                                <h2 id="meta-title" class="text-sm font-bold font-mono text-indigo-400">path/to/file.rs</h2>
                            </div>
                            <p id="meta-subtitle" class="text-xs text-slate-400">File stats: ...</p>
                        </div>
                        <div class="flex space-x-2">
                            <button onclick="copyCurrentCode()" class="bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs px-3 py-1.5 rounded transition border border-slate-700">Copy Code</button>
                        </div>
                    </div>

                    <!-- Code block viewer -->
                    <div class="flex-1 bg-slate-900/20 border border-slate-800 rounded-lg overflow-hidden flex flex-col">
                        <div class="bg-slate-900/60 border-b border-slate-800 px-4 py-2 flex items-center justify-between text-xs text-slate-400 shrink-0 font-medium">
                            <span id="code-viewer-title">No content selected</span>
                            <span id="code-viewer-size">0 bytes</span>
                        </div>
                        <div class="flex-1 overflow-auto relative p-4 bg-slate-950">
                            <pre id="code-display" class="text-xs font-mono text-slate-300 leading-relaxed overflow-x-auto select-text">Select a source file from the catalog or paste a code hash to view implementation details.</pre>
                        </div>
                    </div>
                </div>

                <!-- Right panel for File Hashes -->
                <div id="file-hashes-panel" class="hidden w-80 border-l border-slate-800 bg-slate-900/20 flex flex-col overflow-hidden shrink-0">
                    <div class="p-4 border-b border-slate-800 shrink-0 bg-slate-900/40">
                        <h3 class="text-xs font-bold uppercase tracking-wider text-indigo-400">Extracted AST Code Bodies</h3>
                        <p class="text-[10px] text-slate-500 mt-1">Rust elements with unique stable hashes. Click a hash to isolate its implementation.</p>
                    </div>
                    <div id="file-hashes-list" class="flex-1 overflow-y-auto p-4 space-y-2"></div>
                </div>
            </div>
        </div>
    </main>

    <!-- Message Toasts -->
    <div id="toast-container" class="fixed bottom-4 right-4 z-50 space-y-2"></div>

    <script>
        let currentTab = 'repos';
        let loadedCatalogData = [];
        let logInterval = null;

        function escapeHtml(text) {
            return text
                .replace(/&/g, "&amp;")
                .replace(/</g, "&lt;")
                .replace(/>/g, "&gt;")
                .replace(/"/g, "&quot;")
                .replace(/'/g, "&#039;");
        }

        function showToast(message, isError = false) {
            const container = document.getElementById('toast-container');
            const toast = document.createElement('div');
            toast.className = `px-4 py-2.5 rounded-lg border shadow-lg text-xs font-medium transition duration-300 transform translate-y-2 opacity-0 flex items-center space-x-2 ${
                isError 
                ? 'bg-red-950 border-red-800 text-red-300' 
                : 'bg-slate-900 border-slate-700 text-indigo-300'
            }`;
            toast.innerHTML = `<span>${isError ? '❌' : '✅'}</span><span>${message}</span>`;
            container.appendChild(toast);
            
            setTimeout(() => toast.classList.remove('translate-y-2', 'opacity-0'), 10);
            setTimeout(() => {
                toast.classList.add('opacity-0', 'translate-y-2');
                setTimeout(() => toast.remove(), 300);
            }, 4000);
        }

        function getClientLabel(ua) {
            if (!ua) return 'Unknown Client';
            if (ua.includes('reqwest') || ua.includes('concat_rust_cli')) {
                return '💻 Concat CLI';
            }
            if (ua.includes('Mozilla') || ua.includes('Chrome') || ua.includes('Safari')) {
                return '🌐 Web Dashboard';
            }
            return ua;
        }

        function switchTab(tab) {
            currentTab = tab;
            
            if (tab === 'logs') {
                startLogPolling();
            } else {
                stopLogPolling();
            }

            document.getElementById('tab-btn-repos').className = tab === 'repos' 
                ? 'flex-1 py-3 text-xs font-semibold border-b-2 border-indigo-500 text-white transition' 
                : 'flex-1 py-3 text-xs font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition';
            
            document.getElementById('tab-btn-catalog').className = tab === 'catalog' 
                ? 'flex-1 py-3 text-xs font-semibold border-b-2 border-indigo-500 text-white transition' 
                : 'flex-1 py-3 text-xs font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition';

            document.getElementById('tab-btn-logs').className = tab === 'logs' 
                ? 'flex-1 py-3 text-xs font-semibold border-b-2 border-indigo-500 text-white transition' 
                : 'flex-1 py-3 text-xs font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition';

            if (tab === 'repos') {
                document.getElementById('tab-content-repos').classList.remove('hidden');
                document.getElementById('tab-content-catalog').classList.add('hidden');
                document.getElementById('tab-content-logs').classList.add('hidden');
            } else if (tab === 'catalog') {
                document.getElementById('tab-content-repos').classList.add('hidden');
                document.getElementById('tab-content-catalog').classList.remove('hidden');
                document.getElementById('tab-content-logs').classList.add('hidden');
                loadCatalog();
            } else if (tab === 'logs') {
                document.getElementById('tab-content-repos').classList.add('hidden');
                document.getElementById('tab-content-catalog').classList.add('hidden');
                document.getElementById('tab-content-logs').classList.remove('hidden');
            }
        }

        async function loadLogs() {
            try {
                const res = await fetch('/logs');
                if (!res.ok) throw new Error('Logs offline');
                const data = await res.json();
                const logs = data.logs || [];
                const container = document.getElementById('logs-list');
                container.innerHTML = '';

                if (logs.length === 0) {
                    container.innerHTML = `<div class="text-center py-6 text-xs text-slate-500 font-sans">No client connections received yet. Run terminal commands to trigger request logs.</div>`;
                    return;
                }

                logs.reverse().forEach(log => {
                    const timeStr = new Date(log.timestamp * 1000).toLocaleTimeString();
                    const isSuccess = log.status >= 200 && log.status < 300;
                    const statusColor = isSuccess ? 'text-emerald-400 bg-emerald-950/40 border-emerald-800/40' : 'text-red-400 bg-red-950/40 border-red-800/40';
                    const clientLabel = getClientLabel(log.user_agent);

                    const item = document.createElement('div');
                    item.className = 'p-2 bg-slate-900 border border-slate-800/60 rounded flex flex-col space-y-1.5 transition';
                    item.innerHTML = `
                        <div class="flex justify-between items-center text-[10px] text-slate-500 font-sans">
                            <span>${timeStr}</span>
                            <span class="truncate font-semibold text-slate-400" title="${log.user_agent}">${clientLabel}</span>
                        </div>
                        <div class="flex items-center space-x-1.5 pt-0.5">
                            <span class="px-1.5 py-0.5 rounded text-[10px] border font-bold ${statusColor}">${log.status}</span>
                            <span class="text-slate-300 truncate font-mono text-[10px]" title="${log.path}">${log.method} ${log.path}</span>
                        </div>
                    `;
                    container.appendChild(item);
                });
            } catch (err) {
                console.error('Log sync failed:', err);
            }
        }

        function startLogPolling() {
            if (!logInterval) {
                loadLogs();
                logInterval = setInterval(loadLogs, 2000);
            }
        }

        function stopLogPolling() {
            if (logInterval) {
                clearInterval(logInterval);
                logInterval = null;
            }
        }

        async function loadRepos() {
            try {
                const res = await fetch('/repos');
                if (!res.ok) throw new Error(`HTTP error! status: ${res.status}`);
                const repos = await res.json();
                const list = document.getElementById('repos-list');
                list.innerHTML = '';
                
                if (repos.length === 0) {
                    list.innerHTML = `<div class="text-center py-6 text-xs text-slate-500">No repositories registered yet. Use the form above.</div>`;
                    return;
                }

                repos.forEach(repo => {
                    const dateStr = repo.last_sync 
                        ? new Date(repo.last_sync * 1000).toLocaleString() 
                        : 'Never Synced';
                    const activeDot = repo.active 
                        ? '<span class="h-2 w-2 rounded-full bg-emerald-500"></span>' 
                        : '<span class="h-2 w-2 rounded-full bg-slate-500"></span>';
                    
                    const item = document.createElement('div');
                    item.className = 'bg-slate-900 border border-slate-800 rounded-lg p-3 space-y-2 hover:border-slate-700 transition relative group';
                    item.innerHTML = `
                        <div class="flex items-center justify-between">
                            <div class="flex items-center space-x-2">
                                ${activeDot}
                                <span class="font-bold text-xs text-white font-mono">${repo.id}</span>
                                <span class="text-[10px] bg-indigo-950 text-indigo-400 border border-indigo-900 rounded px-1.5 py-0.5 font-medium font-mono">${repo.git_branch || 'detached'}</span>
                            </div>
                            <button onclick="deleteRepo('${repo.id}')" class="text-slate-500 hover:text-red-400 text-xs p-1 rounded hover:bg-slate-800 transition">🗑️</button>
                        </div>
                        <div class="text-[11px] text-slate-400 font-mono truncate" title="${repo.source_path}">
                            Path: ${repo.source_path}
                        </div>
                        <div class="flex justify-between items-center text-[10px] text-slate-500 border-t border-slate-800/60 pt-2 font-mono">
                            <span>Files: ${repo.file_count || 0}</span>
                            <span>Synced: ${dateStr}</span>
                        </div>
                    `;
                    list.appendChild(item);
                });
            } catch (err) {
                console.error(err);
                showToast('Failed to load repositories', true);
            }
        }

        async function handleAddRepo(e) {
            e.preventDefault();
            const id = document.getElementById('repo-id').value.trim();
            const path = document.getElementById('repo-path').value.trim();
            if (!id || !path) return;

            try {
                const res = await fetch('/repos', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ id, source_path: path })
                });
                const msg = await res.text();
                if (!res.ok) throw new Error(msg);
                
                showToast(msg);
                document.getElementById('add-repo-form').reset();
                loadRepos();
            } catch (err) {
                showToast(err.message || 'Failed to register repo', true);
            }
        }

        async function deleteRepo(id) {
            if (!confirm(`Remove repo '${id}'?`)) return;
            try {
                const res = await fetch(`/repos/${id}`, { method: 'DELETE' });
                const msg = await res.text();
                if (!res.ok) throw new Error(msg);
                
                showToast(msg);
                loadRepos();
            } catch (err) {
                showToast(err.message || 'Failed to delete repo', true);
            }
        }

        async function syncAll() {
            const spinner = document.getElementById('sync-spinner');
            spinner.classList.remove('hidden');
            try {
                const res = await fetch('/sync', { method: 'POST' });
                const msg = await res.text();
                if (!res.ok) throw new Error(msg);
                
                showToast(msg);
                loadRepos();
                if (currentTab === 'catalog') loadCatalog();
            } catch (err) {
                showToast(err.message || 'Sync failed', true);
            } finally {
                spinner.classList.add('hidden');
            }
        }

        async function loadCatalog() {
            try {
                const res = await fetch('/catalog');
                if (!res.ok) throw new Error('Failed to fetch catalog');
                const data = await res.json();
                loadedCatalogData = data.files || [];
                filterCatalog();
            } catch (err) {
                showToast('Failed to load file catalog', true);
            }
        }

        function filterCatalog() {
            const query = document.getElementById('catalog-search').value.toLowerCase();
            const list = document.getElementById('catalog-list');
            list.innerHTML = '';

            const filtered = loadedCatalogData.filter(file => 
                file.filepath.toLowerCase().includes(query)
            );

            if (filtered.length === 0) {
                list.innerHTML = `<div class="text-center py-6 text-xs text-slate-500">No matching files.</div>`;
                return;
            }

            filtered.forEach(file => {
                const item = document.createElement('button');
                item.className = 'w-full text-left bg-slate-900 hover:bg-slate-800 text-slate-300 hover:text-white px-3 py-2 rounded text-xs transition font-mono border border-slate-800/40 flex items-center justify-between group';
                item.onclick = () => viewFile(file.filepath);
                
                const isRust = file.filepath.endsWith('.rs');
                item.innerHTML = `
                    <div class="truncate pr-2 flex items-center space-x-1.5">
                        <span class="text-[10px]">${isRust ? '🦀' : '📄'}</span>
                        <span class="truncate" title="${file.filepath}">${file.filepath}</span>
                    </div>
                    <span class="text-[9px] text-slate-500 group-hover:text-slate-400 font-semibold bg-slate-950 px-1.5 py-0.5 rounded shrink-0">${file.loc} LOC</span>
                `;
                list.appendChild(item);
            });
        }

        async function viewFile(filepath) {
            try {
                const displayHeader = document.getElementById('metadata-header');
                displayHeader.classList.remove('hidden');
                document.getElementById('meta-icon').innerText = filepath.endsWith('.rs') ? '🦀' : '📄';
                document.getElementById('meta-title').innerText = filepath;
                document.getElementById('meta-subtitle').innerText = 'Loading file contents...';

                const codeRes = await fetch(`/file/${filepath}`);
                if (!codeRes.ok) throw new Error('Could not fetch file contents.');
                const rawCode = await codeRes.text();

                let viewCode = rawCode;
                if (rawCode.startsWith('//--+ file:///')) {
                    const newlinePos = rawCode.indexOf('\n');
                    if (newlinePos !== -1) {
                        viewCode = rawCode.substring(newlinePos + 1);
                    }
                }

                document.getElementById('code-display').innerHTML = escapeHtml(viewCode);
                document.getElementById('code-viewer-title').innerText = filepath;
                document.getElementById('code-viewer-size').innerText = `${viewCode.length} bytes`;
                
                const hashesPanel = document.getElementById('file-hashes-panel');
                if (filepath.endsWith('.rs')) {
                    hashesPanel.classList.remove('hidden');
                    const infoRes = await fetch(`/file-info/${filepath}`);
                    if (infoRes.ok) {
                        const info = await infoRes.json();
                        document.getElementById('meta-subtitle').innerText = `${info.loc} LOC | ${info.byte_size} bytes`;
                        renderFileHashes(info.body_hashes || []);
                    } else {
                        hashesPanel.classList.add('hidden');
                    }
                } else {
                    hashesPanel.classList.add('hidden');
                    document.getElementById('meta-subtitle').innerText = `${viewCode.split('\n').length} LOC | ${viewCode.length} bytes`;
                }
                
                document.getElementById('right-pane-stats').innerText = `Viewing: ${filepath}`;
            } catch (err) {
                showToast('Failed to open file', true);
            }
        }

        function renderFileHashes(hashes) {
            const container = document.getElementById('file-hashes-list');
            container.innerHTML = '';

            if (hashes.length === 0) {
                container.innerHTML = '<div class="text-slate-500 text-xs text-center py-4">No hash fragments.</div>';
                return;
            }

            hashes.forEach(item => {
                const btn = document.createElement('button');
                btn.className = 'w-full text-left bg-slate-950 hover:bg-indigo-950/20 hover:border-indigo-800/80 border border-slate-800 rounded p-2.5 space-y-1.5 transition block group';
                btn.onclick = () => loadSpecificHash(item.hash);
                btn.innerHTML = `
                    <div class="flex items-center justify-between">
                        <span class="font-mono text-[10px] text-indigo-400 font-bold group-hover:text-indigo-300"># ${item.hash}</span>
                        <span class="text-[9px] text-slate-500 font-bold bg-slate-900 px-1 rounded">${item.loc} LOC</span>
                    </div>
                `;
                container.appendChild(btn);
            });
        }

        async function loadSpecificHash(hash) {
            document.getElementById('hash-search').value = hash;
            inspectHash();
        }

        function handleHashSearch(e) {
            if (e.key === 'Enter') inspectHash();
        }

        async function inspectHash() {
            const query = document.getElementById('hash-search').value.trim();
            if (!query) return;

            try {
                const infoRes = await fetch(`/info/${query}`);
                if (!infoRes.ok) throw new Error('Hash info not found');
                const infos = await infoRes.json();

                const codeRes = await fetch(`/${query}`);
                if (!codeRes.ok) throw new Error('Could not fetch hash code body.');
                const bodyCodeRaw = await codeRes.text();

                let bodyCode = bodyCodeRaw;
                if (bodyCodeRaw.startsWith('//--+ file:///')) {
                    const newlinePos = bodyCodeRaw.indexOf('\n');
                    if (newlinePos !== -1) {
                        bodyCode = bodyCodeRaw.substring(newlinePos + 1);
                    }
                }

                document.getElementById('metadata-header').classList.remove('hidden');
                document.getElementById('meta-icon').innerText = '🔑';
                
                if (infos.length === 1) {
                    const info = infos[0];
                    document.getElementById('meta-title').innerText = `Hash: ${info.hash}`;
                    document.getElementById('meta-subtitle').innerText = `Source file: ${info.filepath} | Scope size: ${info.loc} LOC | ${info.byte_size} bytes`;
                    document.getElementById('code-viewer-title').innerText = `Scope context: ${info.filepath}`;
                } else {
                    document.getElementById('meta-title').innerText = `Hashes: ${query}`;
                    document.getElementById('meta-subtitle').innerText = `Multiple hash definitions found (${typeInfos.length})`;
                    document.getElementById('code-viewer-title').innerText = `Combined Context`;
                }

                document.getElementById('code-display').innerHTML = escapeHtml(bodyCode);
                document.getElementById('code-viewer-size').innerText = `${bodyCode.length} bytes`;
                document.getElementById('file-hashes-panel').classList.add('hidden');
                document.getElementById('right-pane-stats').innerText = `Inspecting: ${query}`;
            } catch (err) {
                showToast(err.message || 'Failed to inspect hash', true);
            }
        }

        function copyCurrentCode() {
            const codeText = document.getElementById('code-display').innerText;
            navigator.clipboard.writeText(codeText).then(() => {
                showToast('Code copied to clipboard!');
            }).catch(err => {
                showToast('Failed to copy code', true);
            });
        }

        window.onload = () => {
            loadRepos();
        };
    </script>
</body>
</html>"#;
