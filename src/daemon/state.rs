//--+ src/daemon/state.rs

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::cache::DaemonCache;
use crate::config::ScanConfig;
use crate::registry::RepoRegistry;

#[derive(Clone)]
pub struct AppState {
    pub cache: Arc<Mutex<DaemonCache>>,
    pub registry: Arc<Mutex<RepoRegistry>>,
    pub config: Arc<ScanConfig>,
    pub central_dir: PathBuf,
    pub daemon_port: u16,
    pub max_width: i32,
    pub no_format: bool,
}
