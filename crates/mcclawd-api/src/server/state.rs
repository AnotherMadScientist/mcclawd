use crate::supervisor::AgentSupervisor;
use mcclawd_channels::OutboundChunk;
use mcclawd_core::secrets::SecretBackend;
use mcclawd_core::types::TaskId;
use mcclawd_core::McclawdConfig;
use mcclawd_tasks::TaskManager;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use webauthn_rs::prelude::*;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<McclawdConfig>>,
    pub tasks: Arc<RwLock<TaskManager>>,
    pub jwt_secret: String,
    pub secrets: Arc<RwLock<Option<Arc<dyn SecretBackend>>>>,
    /// Per-task broadcast channels for streaming agent output to WebSocket clients.
    pub task_streams: Arc<RwLock<HashMap<TaskId, broadcast::Sender<OutboundChunk>>>>,
    /// Agent supervisor for sandboxed execution (None if Docker unavailable).
    pub supervisor: Option<Arc<AgentSupervisor>>,
    /// WebAuthn verifier instance for passkey authentication.
    pub webauthn: Arc<Webauthn>,
    /// Temporary registration state (in-flight challenge).
    /// Single-slot: McClawd is a single-user local app — only one ceremony at a time.
    pub webauthn_reg_state: Arc<RwLock<Option<(Uuid, PasskeyRegistration)>>>,
    /// Temporary authentication state (in-flight challenge).
    /// Single-slot: McClawd is a single-user local app — only one ceremony at a time.
    pub webauthn_auth_state: Arc<RwLock<Option<PasskeyAuthentication>>>,
}

impl AppState {
    pub fn new(
        config: McclawdConfig,
        supervisor: Option<Arc<AgentSupervisor>>,
    ) -> anyhow::Result<Self> {
        let rp_id = "localhost";
        let rp_origin = url::Url::parse("http://localhost:8080")?;
        let webauthn = WebauthnBuilder::new(rp_id, &rp_origin)?
            .rp_name("McClawd")
            .build()?;

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            tasks: Arc::new(RwLock::new(TaskManager::new())),
            jwt_secret: uuid::Uuid::new_v4().to_string(),
            secrets: Arc::new(RwLock::new(None)),
            task_streams: Arc::new(RwLock::new(HashMap::new())),
            supervisor,
            webauthn: Arc::new(webauthn),
            webauthn_reg_state: Arc::new(RwLock::new(None)),
            webauthn_auth_state: Arc::new(RwLock::new(None)),
        })
    }

    /// Create a broadcast channel for a task and return the sender.
    pub async fn create_task_stream(&self, task_id: &TaskId) -> broadcast::Sender<OutboundChunk> {
        let (tx, _) = broadcast::channel(64);
        let mut streams = self.task_streams.write().await;
        streams.insert(task_id.clone(), tx.clone());
        tx
    }

    /// Subscribe to a task's output stream.
    pub async fn subscribe_task_stream(
        &self,
        task_id: &TaskId,
    ) -> Option<broadcast::Receiver<OutboundChunk>> {
        let streams = self.task_streams.read().await;
        streams.get(task_id).map(|tx| tx.subscribe())
    }
}
