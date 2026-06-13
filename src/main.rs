// === src/main.rs ===
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use axum::routing::{delete, get, post};
use grab::cache::DaemonCache;
use grab::config::ScanConfig;
use grab::daemon::routes_read;
use grab::daemon::routes_write;
use grab::daemon::state::AppState;
use grab::registry::RepoRegistry;
use grab::scanner;
use tower_http::cors::{Any, CorsLayer};

#[derive(Parser, Debug)]
#[command(
    name = "concat_rust",
    about = "Compressed code skeleton daemon with multi-repo sync"
)]
struct Args {
    /// Central directory for synced source mirrors
    #[arg(long, default_value = ".concat_rust_central")]
    central_dir: String,

    /// Port for the daemon
    #[arg(long, default_value_t = 7890)]
    port: u16,

    /// Skip rustfmt (default: true, use --no-format=false to enable formatting)
    #[arg(long, default_value_t = true)]
    no_format: bool,

    /// rustfmt max width
    #[arg(long, default_value_t = 350)]
    max_width: i32,

    /// Cache file path
    #[arg(long, default_value = "concat_rust.cache")]
    cache: String,
}

async fn get_meta_prompt(axum::extract::State(state): axum::extract::State<AppState>) -> String {
    let db = state.cache.lock().await;
    db.effective_meta_prompt()
}

/// Retrieve the active repository context from the local filesystem configuration
async fn get_active() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let path = std::path::PathBuf::from(home)
        .join(".concat_rust")
        .join("active");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "none")
        .unwrap_or_default()
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let central_dir = PathBuf::from(&args.central_dir);

    // Ensure central dir exists
    std::fs::create_dir_all(&central_dir).expect("Failed to create central dir");

    // ── Load Registry & Cache ──
    let registry = RepoRegistry::load_or_create(&central_dir);
    let mut cache = DaemonCache::default();
    cache.cache_path = args.cache.clone();

    if PathBuf::from(&args.cache).exists() {
        if let Ok(loaded) = DaemonCache::load(&args.cache) {
            cache = loaded;
        }
    }

    let registry = Arc::new(Mutex::new(registry));
    let cache = Arc::new(Mutex::new(cache));
    let log_buffer = Arc::new(Mutex::new(Vec::new()));

    // ── Initial Index (only if central dir already has files) ──
    let has_repos = !registry.lock().await.repos.is_empty();

    if has_repos {
        println!("🔄 Syncing registered repos...");
        let _ = grab::sync_runner::sync_all_repos(registry.clone(), central_dir.clone()).await;

        println!("🔍 Indexing central dir...");
        let config = ScanConfig::default();
        let scan_result =
            scanner::scan_directory(&central_dir, &config, args.no_format, args.max_width);

        let mut db = cache.lock().await;
        for file in scan_result.files {
            let rel_str = file.rel_path;
            if rel_str.ends_with(".rs") {
                let body_hashes: Vec<String> =
                    file.bodies.iter().map(|(h, _, _)| h.clone()).collect();
                for (hash, meta, body) in file.bodies {
                    db.bodies
                        .insert(hash, grab::cache::BodyEntry { meta, body });
                }
                db.files.insert(
                    rel_str.clone(),
                    grab::cache::FileEntry {
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
        db.generation += 1;
        let _ = db.save();
        println!("✅ Index complete (gen {})", db.generation);
    } else {
        println!("ℹ️  No repos registered yet. Use the CLI to add one:");
        println!("   cli add-repo <id> <path>");
    }

    // ── Start Daemon ──
    let state = AppState {
        cache: cache.clone(),
        registry: registry.clone(),
        request_log: log_buffer,
        central_dir: central_dir.clone(),
        daemon_port: args.port,
        max_width: args.max_width,
        no_format: args.no_format,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = axum::Router::new()
        // Dashboard routes
        .route("/", get(routes_read::get_dashboard))
        .route("/dashboard", get(routes_read::get_dashboard))
        .route("/logs", get(routes_read::get_logs))
        // Read routes
        .route("/skeleton", get(routes_read::get_skeleton))
        .route("/meta-prompt", get(get_meta_prompt))
        .route("/catalog", get(routes_read::get_catalog))
        .route("/info/:hash", get(routes_read::get_body_info))
        .route("/file-info/*path", get(routes_read::get_file_info))
        .route("/file/*path", get(routes_read::get_file))
        .route("/active", get(get_active))
        .route("/active-repo", get(get_active))
        .route("/:hash", get(routes_read::get_body))
        // Write/Sync routes
        .route("/repos", get(routes_write::get_repos))
        .route("/repos", post(routes_write::post_repo_add))
        .route("/repos/:id", delete(routes_write::delete_repo))
        .route("/sync", post(routes_write::post_sync))
        .route("/stats", get(routes_read::get_stats))
        .layer(cors)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            routes_read::log_middleware,
        ))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
    println!(" 🚀 Daemon on http://{}", addr);
    println!("    GET    /skeleton          → compressed skeleton");
    println!("    GET    /catalog           → all files, LOC, sizes");
    println!("    GET    /file/<path>       → full file code");
    println!("    GET    /active            → retrieve active repository name");
    println!("    GET    /<HASH>            → body code");
    println!("    POST   /repos             → register repo");
    println!("    GET    /repos             → list repos");
    println!("    DELETE /repos/<id>        → remove repo");
    println!("    POST   /sync              → sync & reindex");

    if let Err(e) = axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app).await {
        eprintln!("Daemon error: {}", e);
    }
}
