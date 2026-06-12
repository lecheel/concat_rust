use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::cache::DaemonCache;
use crate::config::ScanConfig;
use crate::registry::RepoRegistry;

#[derive(Serialize, Clone, Debug)]
pub struct RequestLog {
    pub timestamp: u64,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub user_agent: String,
}

#[derive(Clone)]
pub struct AppState {
    pub cache: Arc<Mutex<DaemonCache>>,
    pub registry: Arc<Mutex<RepoRegistry>>,
    pub config: Arc<ScanConfig>,
    pub central_dir: PathBuf,
    pub daemon_port: u16,
    pub max_width: i32,
    pub no_format: bool,
    pub log_buffer: Arc<Mutex<Vec<RequestLog>>>,
}
