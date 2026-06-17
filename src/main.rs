use axum::routing::{delete, get, post};
use clap::Parser;
use grab::cache::DaemonCache;
use grab::daemon::routes_read;
use grab::daemon::routes_write;
use grab::daemon::state::AppState;
use grab::registry::RepoRegistry;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
#[derive(Parser, Debug)]
#[command(
    name = "concat_rust",
    about = "Compressed code skeleton daemon with multi-repo sync"
)]
struct Args {
    #[arg(long, default_value = ".concat_rust_central")]
    central_dir: String,
    #[arg(long, default_value_t = 7890)]
    port: u16,
    #[arg(long, default_value_t = true)]
    no_format: bool,
    #[arg(long, default_value_t = 350)]
    max_width: i32,
    #[arg(long, default_value = "concat_rust.cache")]
    cache: String,
}
async fn get_meta_prompt(axum::extract::State(state): axum::extract::State<AppState>) -> String {
    let db = state.cache.lock().await;
    db.effective_meta_prompt()
}
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
    std::fs::create_dir_all(&central_dir).expect("Failed to create central dir");
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
    let state = AppState {
        cache: cache.clone(),
        registry: registry.clone(),
        request_log: log_buffer,
        central_dir: central_dir.clone(),
        daemon_port: args.port,
        max_width: args.max_width,
        no_format: args.no_format,
        skeleton_loc: Arc::new(AtomicUsize::new(0)),
        file_loc: Arc::new(AtomicUsize::new(0)),
        hash_loc: Arc::new(AtomicUsize::new(0)),
    };
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let app = axum::Router::new()
        .route("/", get(routes_read::get_dashboard))
        .route("/dashboard", get(routes_read::get_dashboard))
        .route("/logs", get(routes_read::get_logs))
        .route("/skeleton", get(routes_read::get_skeleton))
        .route("/meta-prompt", get(get_meta_prompt))
        .route("/catalog", get(routes_read::get_catalog))
        .route("/loc-info", get(routes_read::get_loc_info))
        .route("/info/:hash", get(routes_read::get_body_info))
        .route("/file-info/*path", get(routes_read::get_file_info))
        .route("/file/*path", get(routes_read::get_file))
        .route("/active", get(get_active))
        .route("/active-repo", get(get_active))
        .route("/:hash", get(routes_read::get_body))
        .route("/repos", get(routes_write::get_repos))
        .route("/repos", post(routes_write::post_repo_add))
        .route("/repos/:id", delete(routes_write::delete_repo))
        .route("/sync", post(routes_write::post_sync))
        .route("/sync/:id", post(routes_write::post_sync_repo))
        .route("/stats", get(routes_read::get_stats))
        .route("/resolve", post(routes_write::post_resolve))
        .layer(cors)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            routes_read::log_middleware,
        ))
        .with_state(state);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
    println!(" 🚀 Daemon on http://{}", addr);
    println!("    GET    /skeleton       → compressed skeleton");
    println!("    GET    /catalog        → all files, LOC, sizes");
    println!("    GET    /file/<path>    → full file code");
    println!("    GET    /loc-info       → fetched LOC stats (resets on /skeleton)");
    println!("    GET    /active         → retrieve active repository name");
    println!("    GET    /<HASH>         → body code");
    println!("    POST   /repos          → register repo (auto-syncs that repo)");
    println!("    GET    /repos          → list repos");
    println!("    DELETE /repos/<id>     → remove repo");
    println!("    POST   /sync           → sync all repos & reindex");
    println!("    POST   /sync/<id>      → sync specific repo & reindex");
    if let Err(e) = axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app).await {
        eprintln!("Daemon error: {}", e);
    }
}
