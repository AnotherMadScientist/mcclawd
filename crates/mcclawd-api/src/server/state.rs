use crate::sandbox::container::{PersistentHandle, SandboxHandle};
use crate::server::mcp_lifecycle::McpLifecycleManager;

// McpPorter is in server module, accessed directly
use super::mcp_porter::McpPorter;
use super::runner_build::RunnerBuildState;
use crate::server::pg_store::PgTaskStore;
use crate::server::swarm_registry::SwarmRegistry;
use crate::supervisor::AgentSupervisor;
use dashmap::DashMap;
use mcclawd_channels::OutboundChunk;
use mcclawd_core::providers::{ProviderPool, ProviderPoolConfig};
use mcclawd_core::hooks::HookPipeline;
use mcclawd_core::scanner::ScanResult;
use mcclawd_core::secrets::SecretBackend;
use mcclawd_core::types::TaskId;
use mcclawd_core::McclawdConfig;
use mcclawd_tasks::scheduler::TaskScheduler;
use mcclawd_tasks::TaskManager;
use rig::completion::message::Message;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::fs;
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
    /// Per-task LLM conversation history for multi-turn follow-ups.
    pub task_chat_history: Arc<RwLock<HashMap<TaskId, Vec<Message>>>>,
    /// Per-task skill selection (stored on creation, read on follow-up messages).
    pub task_skills: Arc<RwLock<HashMap<TaskId, Vec<String>>>>,
    /// PostgreSQL store for durable persistence (required).
    pub pg_store: PgTaskStore,
    /// Cache for security scan results (skill name -> ScanResult).
    pub scan_cache: Arc<DashMap<String, ScanResult>>,
    /// Task scheduler for cron-based recurring tasks.
    pub scheduler: TaskScheduler,
    /// Swarm run registry for tracking active/completed swarm executions.
    pub swarm_registry: SwarmRegistry,
    /// MCP server lifecycle manager (None if Docker unavailable).
    pub mcp_lifecycle: Option<McpLifecycleManager>,
    /// McpPorter: builds on-demand Docker images, resolves tools, manages agent environments.
    /// None if Docker is unavailable.
    pub mcp_porter: Option<Arc<McpPorter>>,
    /// Long-lived system agent persistent container handle (started on first WS connection).
    pub system_agent: Arc<RwLock<Option<PersistentHandle>>>,
    /// Per-task persistent container handles (one container per task, lives until task deleted).
    pub task_containers: Arc<RwLock<HashMap<TaskId, PersistentHandle>>>,
    /// Runner image build state (progress, logs, status).
    pub runner_build: Arc<RwLock<RunnerBuildState>>,
    /// Security hook pipeline (DLP + secret scanner + sidecar + audit).
    pub security_pipeline: Arc<HookPipeline>,
}

impl AppState {
    pub fn new(
        config: McclawdConfig,
        supervisor: Option<Arc<AgentSupervisor>>,
        pg_store: PgTaskStore,
    ) -> anyhow::Result<Self> {
        let rp_id = "localhost";
        let rp_origin = url::Url::parse("http://localhost:8080")?;
        let webauthn = WebauthnBuilder::new(rp_id, &rp_origin)?
            .rp_name("McClawd")
            .build()?;

        let pool_config = Self::build_provider_pool_config(&config);
        let data_dir = dirs::home_dir().map(|h| h.join(".mcclawd"));
        let provider_pool = ProviderPool::with_data_dir(pool_config, data_dir);

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            tasks: Arc::new(RwLock::new(TaskManager::new())),
            jwt_secret: Self::load_or_create_jwt_secret()?,
            secrets: Arc::new(RwLock::new(None)),
            task_streams: Arc::new(RwLock::new(HashMap::new())),
            task_events: Arc::new(RwLock::new(HashMap::new())),
            supervisor,
            webauthn: Arc::new(webauthn),
            webauthn_reg_state: Arc::new(RwLock::new(None)),
            webauthn_auth_state: Arc::new(RwLock::new(None)),
            provider_pool: Arc::new(RwLock::new(provider_pool)),
            config_path: None,
            task_chat_history: Arc::new(RwLock::new(HashMap::new())),
            task_skills: Arc::new(RwLock::new(HashMap::new())),
            pg_store,
            scan_cache: Arc::new(DashMap::new()),
            scheduler: TaskScheduler::new(),
            swarm_registry: SwarmRegistry::new(),
            mcp_lifecycle: McpLifecycleManager::new().ok(),
            mcp_porter: McpLifecycleManager::new()
                .ok()
                .and_then(|lm| McpPorter::new(lm).ok())
                .map(Arc::new),
            system_agent: Arc::new(RwLock::new(None)),
            task_containers: Arc::new(RwLock::new(HashMap::new())),
            runner_build: Arc::new(RwLock::new(RunnerBuildState::default())),
            security_pipeline: Arc::new(HookPipeline::default()),
        })
    }

    /// Load JWT signing secret from `~/.mcclawd/jwt.key`, or generate and persist a new one.
    /// Persisting the secret means JWT tokens survive server restarts (cargo-watch, etc.)
    /// so frontend sessions are not invalidated on every code change.
    fn load_or_create_jwt_secret() -> anyhow::Result<String> {
        let path = dirs::home_dir()
            .unwrap_or_default()
            .join(".mcclawd")
            .join("jwt.key");
        match fs::read_to_string(&path) {
            Ok(s) if !s.trim().is_empty() => return Ok(s.trim().to_string()),
            Ok(_) | Err(_) => {} // missing or empty — generate below
        }
        let secret = uuid::Uuid::new_v4().to_string();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &secret)?;
        // Restrict permissions (owner-only read/write)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(secret)
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
    /// Broadcast is immediate (non-blocking); persistence happens in a spawned task
    /// so the streaming loop is never blocked by the write lock.
    pub async fn send_and_persist(&self, task_id: &TaskId, tx: &broadcast::Sender<OutboundChunk>, chunk: OutboundChunk) {
        let _ = tx.send(chunk.clone());
        // In-memory persistence
        let events = self.task_events.clone();
        let tid = task_id.clone();
        let chunk_clone = chunk.clone();
        tokio::spawn(async move {
            let mut guard = events.write().await;
            guard.entry(tid).or_default().push(chunk_clone);
        });
        // PostgreSQL persistence (fire-and-forget)
        let store = self.pg_store.clone();
        let tid = task_id.0.clone();
        tokio::spawn(async move {
            if let Err(e) = store.append_event(&tid, &chunk).await {
                tracing::warn!(task_id = %tid, error = %e, "Failed to persist event to postgres");
            }
        });
    }

    /// Persist a chunk to task_events only (no broadcast). Used for complete TextBlocks at turn end.
    pub async fn persist_only(&self, task_id: &TaskId, chunk: OutboundChunk) {
        // In-memory
        let events = self.task_events.clone();
        let tid = task_id.clone();
        let chunk_clone = chunk.clone();
        tokio::spawn(async move {
            let mut guard = events.write().await;
            guard.entry(tid).or_default().push(chunk_clone);
        });
        // PostgreSQL
        let store = self.pg_store.clone();
        let tid = task_id.0.clone();
        tokio::spawn(async move {
            if let Err(e) = store.append_event(&tid, &chunk).await {
                tracing::warn!(task_id = %tid, error = %e, "Failed to persist event to postgres");
            }
        });
    }

    /// Get the LLM conversation history for a task (for multi-turn follow-ups).
    pub async fn get_chat_history(&self, task_id: &TaskId) -> Vec<Message> {
        // Try in-memory first
        let history = self.task_chat_history.read().await;
        if let Some(msgs) = history.get(task_id) {
            if !msgs.is_empty() {
                return msgs.clone();
            }
        }
        drop(history);

        // Fall back to postgres
        match self.pg_store.get_chat_history(&task_id.0).await {
            Ok(msgs) if !msgs.is_empty() => {
                // Hydrate in-memory cache
                let mut history = self.task_chat_history.write().await;
                history.insert(task_id.clone(), msgs.clone());
                msgs
            }
            Err(e) => {
                tracing::warn!(task_id = %task_id.0, error = %e, "Failed to load chat history from postgres");
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Replace the LLM conversation history for a task with the full history from FinalResponse.
    pub async fn set_chat_history(&self, task_id: &TaskId, messages: Vec<Message>) {
        // In-memory
        let mut history = self.task_chat_history.write().await;
        history.insert(task_id.clone(), messages.clone());
        drop(history);

        // PostgreSQL
        let store = self.pg_store.clone();
        let tid = task_id.0.clone();
        tokio::spawn(async move {
            if let Err(e) = store.set_chat_history(&tid, &messages).await {
                tracing::warn!(task_id = %tid, error = %e, "Failed to persist chat history to postgres");
            }
        });
    }

    /// Get persisted event history for a task.
    pub async fn get_task_events(&self, task_id: &TaskId) -> Vec<OutboundChunk> {
        // Try in-memory first
        let events = self.task_events.read().await;
        if let Some(evts) = events.get(task_id) {
            if !evts.is_empty() {
                return evts.clone();
            }
        }
        drop(events);

        // Fall back to postgres
        match self.pg_store.get_events(&task_id.0).await {
            Ok(evts) if !evts.is_empty() => {
                // Hydrate in-memory cache
                let mut events = self.task_events.write().await;
                events.insert(task_id.clone(), evts.clone());
                evts
            }
            Err(e) => {
                tracing::warn!(task_id = %task_id.0, error = %e, "Failed to load events from postgres");
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Overwrite task events in memory (used for edit/retry truncation).
    pub async fn set_task_events(&self, task_id: &TaskId, events: Vec<OutboundChunk>) {
        let mut store = self.task_events.write().await;
        store.insert(task_id.clone(), events);
    }

    /// Persist a new task to postgres (called after TaskManager::start_task).
    /// This is synchronous (awaited inline) to guarantee the task is in Postgres
    /// before the API returns to the client — prevents data loss on cargo-watch restarts.
    pub async fn pg_save_task(&self, task_id: &TaskId, prompt: &str, status: &str, tags: &[String]) {
        if let Err(e) = self
            .pg_store
            .save_task(&task_id.0, prompt, status, None, "admin", tags)
            .await
        {
            tracing::warn!(task_id = %task_id.0, error = %e, "Failed to save task to postgres");
        }
    }

    /// Update task status in postgres (fire-and-forget).
    pub async fn pg_update_status(&self, task_id: &TaskId, status: &str, error_message: Option<&str>) {
        let store = self.pg_store.clone();
        let tid = task_id.0.clone();
        let status = status.to_string();
        let err_msg = error_message.map(|s| s.to_string());
        tokio::spawn(async move {
            if let Err(e) = store.update_status(&tid, &status, err_msg.as_deref()).await {
                tracing::warn!(task_id = %tid, error = %e, "Failed to update task status in postgres");
            }
        });
    }

    /// Update task status in postgres synchronously (awaits completion).
    pub async fn pg_update_status_sync(&self, task_id: &TaskId, status: &str, error_message: Option<&str>) {
        if let Err(e) = self.pg_store.update_status(&task_id.0, status, error_message).await {
            tracing::warn!(task_id = %task_id.0, error = %e, "Failed to update task status in postgres");
        }
    }

    /// Delete task from postgres (fire-and-forget, spawned in background).
    pub async fn pg_delete_task(&self, task_id: &TaskId) {
        let store = self.pg_store.clone();
        let tid = task_id.0.clone();
        tokio::spawn(async move {
            if let Err(e) = store.delete_task(&tid).await {
                tracing::warn!(task_id = %tid, error = %e, "Failed to delete task from postgres");
            }
        });
    }

    /// Delete task from postgres synchronously (awaits completion).
    /// Use this in cascade-delete paths where the caller needs to guarantee
    /// the row is gone before returning (e.g. delete_task, delete_container handlers).
    pub async fn pg_delete_task_sync(&self, task_id: &TaskId) {
        if let Err(e) = self.pg_store.delete_task(&task_id.0).await {
            tracing::warn!(task_id = %task_id.0, error = %e, "Failed to delete task from postgres");
        }
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
        let data_dir = dirs::home_dir().map(|h| h.join(".mcclawd"));
        let new_pool = ProviderPool::with_data_dir(pool_config, data_dir);

        // Update config and pool atomically.
        {
            let mut config = self.config.write().await;
            *config = new_config;
        }
        {
            // Re-hydrate usage from DB before swapping
            if let (Ok(daily), Ok(models), Ok(tasks)) = (
                self.pg_store.load_daily_usage().await,
                self.pg_store.load_model_usage().await,
                self.pg_store.load_task_usage().await,
            ) {
                new_pool.hydrate_usage(daily, models, tasks);
            }
            let mut pool = self.provider_pool.write().await;
            *pool = new_pool;
        }

        tracing::info!("Config and provider pool reloaded from disk");
        Ok(())
    }
}
