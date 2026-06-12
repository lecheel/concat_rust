// === src/daemon/state.rs ===
pub use crate::registry::RepoRegistry;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RequestLog {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub timestamp: String,
    pub duration_ms: u64,
    pub user_agent: String,
}

#[derive(Clone)]
pub struct AppState {
    pub cache: Arc<Mutex<crate::cache::DaemonCache>>,
    pub registry: Arc<tokio::sync::Mutex<RepoRegistry>>,
    pub request_log: Arc<Mutex<Vec<RequestLog>>>,
    pub central_dir: std::path::PathBuf,
    pub daemon_port: u16,
    pub no_format: bool,
    pub max_width: i32,
}
