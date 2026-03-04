use crate::supervisor::AgentSupervisor;
use mcclawd_channels::OutboundChunk;
use mcclawd_core::providers::{ProviderPool, ProviderPoolConfig};
use mcclawd_core::secrets::SecretBackend;
use mcclawd_core::types::TaskId;
use mcclawd_core::McclawdConfig;
use mcclawd_tasks::TaskManager;
use std::collections::HashMap;
use std::path::PathBuf;
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
    /// Persisted event history per task (survives broadcast channel drops).
    pub task_events: Arc<RwLock<HashMap<TaskId, Vec<OutboundChunk>>>>,
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
    /// Provider pool for LLM provider selection and usage tracking.
    pub provider_pool: Arc<RwLock<ProviderPool>>,
    /// Path to config file on disk (for hot-reload).
    pub config_path: Option<PathBuf>,
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

        let pool_config = Self::build_provider_pool_config(&config);
        let provider_pool = ProviderPool::new(pool_config);

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            tasks: Arc::new(RwLock::new(TaskManager::new())),
            jwt_secret: uuid::Uuid::new_v4().to_string(),
            secrets: Arc::new(RwLock::new(None)),
            task_streams: Arc::new(RwLock::new(HashMap::new())),
            task_events: Arc::new(RwLock::new(HashMap::new())),
            supervisor,
            webauthn: Arc::new(webauthn),
            webauthn_reg_state: Arc::new(RwLock::new(None)),
            webauthn_auth_state: Arc::new(RwLock::new(None)),
            provider_pool: Arc::new(RwLock::new(provider_pool)),
            config_path: None,
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

    /// Send a chunk on the broadcast channel AND persist it in task_events.
    pub async fn send_and_persist(&self, task_id: &TaskId, tx: &broadcast::Sender<OutboundChunk>, chunk: OutboundChunk) {
        let _ = tx.send(chunk.clone());
        let mut events = self.task_events.write().await;
        events.entry(task_id.clone()).or_default().push(chunk);
    }

    /// Get persisted event history for a task.
    pub async fn get_task_events(&self, task_id: &TaskId) -> Vec<OutboundChunk> {
        let events = self.task_events.read().await;
        events.get(task_id).cloned().unwrap_or_default()
    }

    /// Build a ProviderPoolConfig from the current McclawdConfig.
    ///
    /// Converts the simple ProvidersConfig into a full ProviderPoolConfig
    /// with entries for each configured provider.
    pub fn build_provider_pool_config(config: &McclawdConfig) -> ProviderPoolConfig {
        use mcclawd_core::providers::{ProviderEntry, ProviderKind};

        let mut providers = Vec::new();

        if let Some(ref anthropic) = config.providers.anthropic {
            providers.push(ProviderEntry {
                name: "anthropic".to_string(),
                kind: ProviderKind::Anthropic,
                api_key_secret: anthropic.api_key_secret.clone(),
                models: vec!["claude-sonnet-4-5".to_string(), "claude-sonnet-4-20250514".to_string()],
                priority: 10,
                max_rpm: None,
                enabled: true,
            });
        }

        if let Some(ref openai) = config.providers.openai {
            providers.push(ProviderEntry {
                name: "openai".to_string(),
                kind: ProviderKind::OpenAI,
                api_key_secret: openai.api_key_secret.clone(),
                models: vec!["gpt-4".to_string(), "gpt-4o".to_string()],
                priority: 20,
                max_rpm: None,
                enabled: true,
            });
        }

        if config.providers.ollama.is_some() {
            providers.push(ProviderEntry {
                name: "ollama".to_string(),
                kind: ProviderKind::Ollama,
                api_key_secret: String::new(),
                models: vec!["llama3".to_string(), "mistral".to_string()],
                priority: 30,
                max_rpm: None,
                enabled: true,
            });
        }

        ProviderPoolConfig {
            providers,
            budget: None,
            fallback_order: None,
        }
    }

    /// Get a ProviderPoolConfig from a McclawdConfig reference (for route handlers).
    pub fn provider_pool_config(&self, config: &McclawdConfig) -> ProviderPoolConfig {
        Self::build_provider_pool_config(config)
    }

    /// Reload config from disk and update the provider pool.
    pub async fn reload_config(&self) -> anyhow::Result<()> {
        let config_path = self
            .config_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No config path set — cannot reload"))?;

        let new_config = McclawdConfig::load(config_path)?;
        let pool_config = Self::build_provider_pool_config(&new_config);
        let new_pool = ProviderPool::new(pool_config);

        // Update config and pool atomically.
        {
            let mut config = self.config.write().await;
            *config = new_config;
        }
        {
            let mut pool = self.provider_pool.write().await;
            *pool = new_pool;
        }

        tracing::info!("Config and provider pool reloaded from disk");
        Ok(())
    }
}
