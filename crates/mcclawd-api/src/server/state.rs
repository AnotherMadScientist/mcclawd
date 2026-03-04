use mcclawd_core::McclawdConfig;
use mcclawd_tasks::TaskManager;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<McclawdConfig>>,
    pub tasks: Arc<RwLock<TaskManager>>,
    pub jwt_secret: String,
}

impl AppState {
    pub fn new(config: McclawdConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            tasks: Arc::new(RwLock::new(TaskManager::new())),
            jwt_secret: uuid::Uuid::new_v4().to_string(),
        }
    }
}
