use axum::{
    body::Body,
    extract::{FromRequest, Multipart, Path, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use base64::Engine;
use mcclawd_agent::engine::AgentEngine;
use mcclawd_agent::workspace::WorkspaceLoader;
use mcclawd_channels::{ChannelStatus, OutboundChunk};
use mcclawd_core::types::TaskId;
use mcclawd_tasks::manager::{TaskRecord, TaskStatus};
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::OneOrMany;
use rig::completion::message::{
    DocumentSourceKind, Image, ImageMediaType, Message as RigMessage, MimeType,
    UserContent,
};
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio_util::io::ReaderStream;

use mcclawd_core::hooks::SecurityHook;
use mcclawd_tools::agent_security;

use super::state::AppState;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CreateTaskRequest {
    pub prompt: String,
    pub workspace: Option<String>,
    pub model: Option<String>,
    /// When true, create the task but do NOT auto-start the agent.
    /// The caller must upload attachments and then POST /api/tasks/{id}/message
    /// to trigger the agent. This prevents a race where the agent starts
    /// before attachment files are written to disk.
    #[serde(default)]
    pub delay_start: bool,
    /// Optional tags for categorizing and filtering tasks.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Optional list of skill names to include for this task.
    /// If None or empty, no skills are injected (explicit selection required).
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    /// Tool access profile (minimal/coding/research/full). Falls back to config default.
    #[serde(default)]
    pub tool_profile: Option<mcclawd_core::config::ToolProfile>,
    /// Extra tool prefixes to allow beyond the profile.
    #[serde(default)]
    pub tools_allow: Vec<String>,
    /// Tool prefixes to deny (overrides profile + allow).
    #[serde(default)]
    pub tools_deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub name: String,
    pub size: u64,
    pub content_type: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub prompt: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Skills selected for this task.
    #[serde(default)]
    pub selected_skills: Vec<String>,
    /// Tool names explicitly allowed for this task.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Tool profile used (e.g. "Coding", "Research", "Full", "Minimal").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_profile: Option<String>,
}

impl From<&TaskRecord> for TaskResponse {
    fn from(r: &TaskRecord) -> Self {
        Self {
            id: r.id.0.clone(),
            prompt: r.prompt.clone(),
            status: r.status.clone(),
            tags: r.tags.clone(),
            selected_skills: r.selected_skills.clone(),
            allowed_tools: r.allowed_tools.clone(),
            tool_profile: r.tool_profile.clone(),
        }
    }
}

/// GET /api/tasks — list all tasks (DB-primary with in-memory merge for recent tasks)
pub async fn list_tasks(State(state): State<AppState>) -> Json<Vec<TaskResponse>> {
    let mut tasks: Vec<TaskResponse> = Vec::new();
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // DB is source of truth
    if let Ok(rows) = state.pg_store.list_tasks().await {
        for (id, prompt, status, error_message, tags, selected_skills, allowed_tools, tool_profile, _skill_context) in rows {
            // Never expose the system agent as a user-visible task
            if id == "__system__" || id == "system-agent" {
                continue;
            }
            seen_ids.insert(id.clone());
            let task_status = match status.as_str() {
                "Running" => TaskStatus::Running,
                "Completed" => TaskStatus::Completed,
                "Pending" => TaskStatus::Pending,
                "Building" => TaskStatus::Building,
                "Failed" => TaskStatus::Failed(
                    error_message.unwrap_or_else(|| "Unknown error".to_string()),
                ),
                _ => TaskStatus::Failed(format!("Unknown status: {status}")),
            };
            tasks.push(TaskResponse {
                id: id.clone(),
                prompt,
                status: task_status,
                tags,
                selected_skills,
                allowed_tools,
                tool_profile,
            });
        }
    }

    // Merge in-memory tasks not yet in DB (recently created, race window)
    let mgr = state.tasks.read().await;
    for t in mgr.all_tasks() {
        if t.id.0 == "__system__" || t.id.0 == "system-agent" {
            continue;
        }
        if !seen_ids.contains(&t.id.0) {
            tasks.push(TaskResponse::from(t));
        }
    }

    Json(tasks)
}

/// POST /api/tasks — create a new task and spawn agent execution
pub async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<CreateTaskRequest>,
) -> (StatusCode, Json<TaskResponse>) {
    // Sanitize user prompt to strip known injection patterns
    let sanitized = mcclawd_core::sanitize_prompt(&body.prompt);
    if sanitized.was_modified {
        tracing::warn!(
            patterns = ?sanitized.detected_patterns,
            "Prompt injection patterns detected and stripped from user input"
        );
    }
    let prompt = sanitized.text;

    // Agent security: validate prompt against security policies (blocks secret refs, shell injection, etc.)
    if let Err(reason) = agent_security::validate_task_prompt(&prompt) {
        tracing::warn!(reason = %reason, "Task creation blocked by agent security");
        return (
            StatusCode::BAD_REQUEST,
            Json(TaskResponse {
                id: String::new(),
                prompt,
                status: TaskStatus::Failed(format!("Blocked: {reason}")),
                tags: Vec::new(),
                selected_skills: Vec::new(),
                allowed_tools: Vec::new(),
                tool_profile: None,
            }),
        );
    }

    let workspace_name = body.workspace.clone().unwrap_or_else(|| "default".to_string());
    let task_skills = body.skills.unwrap_or_default();

    // Create task record
    let tags = body.tags.unwrap_or_default();
    let id = {
        let mut mgr = state.tasks.write().await;
        mgr.start_task_with_tags(prompt.clone(), tags)
    };

    // When delay_start is set, persist as Pending (caller will trigger via sendMessage
    // after uploading attachments). Otherwise persist as Running.
    let initial_status = if body.delay_start { "Pending" } else { "Running" };
    // Retrieve tags from the task record for DB persistence
    let task_tags = {
        let mgr = state.tasks.read().await;
        mgr.get_task(&id).map(|t| t.tags.clone()).unwrap_or_default()
    };
    state.pg_save_task(&id, &prompt, initial_status, &task_tags).await;

    let resp = {
        let mgr = state.tasks.read().await;
        match mgr.get_task(&id) {
            Some(task) => TaskResponse::from(task),
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(TaskResponse {
                        id: id.0,
                        prompt,
                        status: TaskStatus::Failed("Task creation failed".to_string()),
                        tags: Vec::new(),
                        selected_skills: Vec::new(),
                        allowed_tools: Vec::new(),
                        tool_profile: None,
                    }),
                );
            }
        }
    };

    // Create broadcast channel for streaming
    let tx = state.create_task_stream(&id).await;

    // Store per-task skill selection for follow-up messages
    {
        let mut skills_map = state.task_skills.write().await;
        skills_map.insert(id.clone(), task_skills.clone());
    }

    if !body.delay_start {
        // Spawn agent execution in background immediately
        let state_clone = state.clone();
        let task_id = id.clone();
        tokio::spawn(async move {
            run_agent(state_clone, task_id, &prompt, &workspace_name, &task_skills, tx).await;
        });
    }

    (StatusCode::CREATED, Json(resp))
}

/// Run the Rig agent and stream output via broadcast channel.
///
/// All execution runs inside Docker containers via the sandbox orchestrator.
/// When `strict_sandbox` is true (default), tasks fail if Docker is unavailable.
/// When false (dev mode only), falls back to in-process host execution.
async fn run_agent(
    state: AppState,
    task_id: TaskId,
    prompt: &str,
    workspace_name: &str,
    task_skills: &[String],
    tx: tokio::sync::broadcast::Sender<OutboundChunk>,
) {
    let strict = state.config.read().await.sandbox.strict_sandbox;

    // Always try Docker-sandboxed execution first
    if let Ok(orch) = crate::sandbox::SandboxOrchestrator::new() {
        if orch.health_check().await {
            run_agent_sandboxed(state, task_id, prompt, workspace_name, task_skills, tx).await;
            return;
        }
    }

    // Docker unavailable — behavior depends on strict_sandbox
    if strict {
        let msg = "Docker required (strict sandbox mode). Start Docker or set strict_sandbox = false in config.".to_string();
        tracing::error!(task_id = %task_id.0, "{msg}");
        state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
        state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
        let mut mgr = state.tasks.write().await;
        mgr.fail_task(&task_id, msg.clone());
        state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
        return;
    }

    // Non-strict fallback: host execution (dev only)
    tracing::warn!("Docker unavailable — falling back to host execution (strict_sandbox=false)");
    let store = state.pg_store.clone();
    let tid = task_id.0.clone();
    tokio::spawn(async move {
        if let Err(e) = store.update_container_info(&tid, "", "host").await {
            tracing::warn!("Failed to record host execution mode: {e}");
        }
    });

    run_agent_host(state, task_id, prompt, workspace_name, task_skills, tx).await;
}

/// Run agent task inside a Docker sandbox container using the JSONL runner protocol.
///
/// Uses persistent containers: the container stays running across messages.
/// On first message, a new persistent container is created with --server mode.
/// Follow-up messages reuse the existing container, sending prompts via stdin.
/// The container is only cleaned up when the task is deleted.
async fn run_agent_sandboxed(
    state: AppState,
    task_id: TaskId,
    prompt: &str,
    _workspace_name: &str,
    task_skills: &[String],
    tx: tokio::sync::broadcast::Sender<OutboundChunk>,
) {
    use crate::sandbox::SandboxOrchestrator;

    // 1. Persist UserMessage
    state
        .send_and_persist(
            &task_id,
            &tx,
            OutboundChunk::UserMessage(prompt.to_string()),
        )
        .await;

    // 2a. Scan the initial prompt through DLP pipeline (inbound content scanning).
    //     The prompt may contain PII, secrets, or injection attempts that should be detected.
    {
        state
            .security_pipeline
            .set_task_context(&task_id.0)
            .await;
        let prompt_json = serde_json::json!({"prompt": prompt});
        if let Err(e) = state
            .security_pipeline
            .before_tool_call("user_prompt", &prompt_json)
            .await
        {
            tracing::warn!(
                task_id = %task_id.0,
                error = %e,
                "Security pipeline flagged user prompt"
            );
            // Don't block the task — log and continue. DLP findings are recorded.
        }
    }

    // 2b. Update status to Running and notify frontend immediately.
    //     The StatusIndicator is persisted so the WS client sees it on history
    //     replay (the WS often connects AFTER the task is already spawned).
    {
        let mut mgr = state.tasks.write().await;
        mgr.running(&task_id);
    }
    state.pg_update_status(&task_id, "Running", None).await;
    state
        .send_and_persist(
            &task_id,
            &tx,
            OutboundChunk::StatusIndicator(ChannelStatus::Processing),
        )
        .await;

    // 3. Check if a persistent container already exists for this task
    let existing_handle = {
        let containers = state.task_containers.read().await;
        containers.get(&task_id).cloned()
    };

    if let Some(handle) = existing_handle {
        // Reuse existing container — send message via stdin
        tracing::info!(
            task_id = %task_id,
            container_id = %handle.container_id,
            "Reusing persistent container for follow-up message"
        );

        let chat_history = state.get_chat_history(&task_id).await;
        let history_json = if !chat_history.is_empty() {
            serde_json::to_string(&chat_history).ok()
        } else {
            None
        };

        if let Err(e) = handle.send_chat(prompt, history_json.as_deref()).await {
            let msg = format!("Failed to send message to container: {e}");
            state
                .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone()))
                .await;
            state
                .send_and_persist(&task_id, &tx, OutboundChunk::Done)
                .await;
            // Container might be dead — remove handle so next message recreates it
            state.task_containers.write().await.remove(&task_id);
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg.clone());
            state
                .pg_update_status(&task_id, "Failed", Some(&msg))
                .await;
        }
        // Response chunks are handled by the background forwarder (started when container was created).
        return;
    }

    // 4. No existing container — create a new persistent one
    let _ = tx.send(OutboundChunk::TextDelta(
        "Starting sandboxed agent...".to_string(),
    ));
    state.pg_update_status(&task_id, "Building", None).await;

    let config = state.config.read().await.clone();

    let orchestrator = match SandboxOrchestrator::new() {
        Ok(o) => o,
        Err(e) => {
            let msg = format!("Docker unavailable for sandbox: {e}");
            state
                .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone()))
                .await;
            state
                .send_and_persist(&task_id, &tx, OutboundChunk::Done)
                .await;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg.clone());
            state
                .pg_update_status(&task_id, "Failed", Some(&msg))
                .await;
            return;
        }
    };

    // Build sandbox config + secrets map
    let sandbox_cfg = mcclawd_core::skills::SandboxConfig {
        workspace_dir: config.workspaces_dir().to_string_lossy().to_string(),
        agentgateway_url: config.mcp.agentgateway_url.clone(),
        memory_limit: config.sandbox.memory_limit,
        cpu_limit: config.sandbox.cpu_limit,
        network: config.sandbox.network.clone(),
        pids_limit: config.sandbox.pids_limit,
        ..Default::default()
    };

    let mut secrets_map = std::collections::HashMap::new();
    if let Some(backend) = state.secrets.read().await.as_ref() {
        if let Ok(Some(key)) = backend.get("ANTHROPIC_API_KEY").await {
            secrets_map.insert("ANTHROPIC_API_KEY".to_string(), key);
        }
    }

    // Use McpPorter to get correct Docker-internal gateway URL (http://agentgateway:3000),
    // ensure network exists, and resolve installed skills → MCP tool filtering.
    // Task agents get full skill resolution — skills and MCP tools are injected here.
    // Falls back to manual construction with container_gateway_url() if McpPorter unavailable.
    let agent_env = if let Some(ref porter) = state.mcp_porter {
        // Load installed skills from disk (~/.mcclawd/skills/) for skill→MCP tool resolution.
        // Per-task skill assignment: only skills explicitly selected for this task are included.
        // If task_skills is empty, no skills are injected (explicit selection required).
        let all_skills: std::collections::HashMap<String, mcclawd_core::skills::LoadedSkill> = {
            let skills_dir = &config.skills.managed_dir;
            let mut map = std::collections::HashMap::new();
            if skills_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(skills_dir) {
                    for entry in entries.flatten() {
                        let skill_md = entry.path().join("SKILL.md");
                        if skill_md.exists() {
                            if let Ok(content) = std::fs::read_to_string(&skill_md) {
                                if let Ok(skill) = mcclawd_core::skill_parser::parse_skill_md(&content) {
                                    map.insert(skill.name.clone(), skill);
                                }
                            }
                        }
                    }
                }
            }
            // Filter to only task-selected skills (empty = no skills)
            if task_skills.is_empty() {
                std::collections::HashMap::new()
            } else {
                map.into_iter()
                    .filter(|(name, _)| task_skills.iter().any(|s| s == name))
                    .collect()
            }
        };
        let skill_count = all_skills.len();
        match porter
            .prepare_task_environment(
                &all_skills,
                &config.mcp.servers,
                &config,
            )
            .await
        {
            Ok(mut env) => {
                // Override image: McpPorter returns mcclawd-agent:{hash}, but we need the runner
                env.image = "mcclawd-runner:latest".to_string();
                tracing::info!(
                    gateway_url = %env.gateway_url,
                    tools = ?env.allowed_tools,
                    skills_loaded = skill_count,
                    skill_context_len = env.skill_context.len(),
                    "McpPorter resolved agent environment for task (with skills)"
                );
                env
            }
            Err(e) => {
                tracing::warn!("McpPorter failed, using fallback: {e}");
                // Derive allowed_tools from skill mcp_tools (fallback when porter unavailable)
                let fallback_tools: Vec<String> = if all_skills.is_empty() {
                    vec![] // No skills selected = no tools
                } else {
                    all_skills.values().flat_map(|s| s.mcp_tools.clone()).collect()
                };
                crate::sandbox::container::AgentEnvironment {
                    image: "mcclawd-runner:latest".to_string(),
                    network: config.sandbox.network.clone(),
                    gateway_url: crate::sandbox::container::container_gateway_url(
                        &config.mcp.agentgateway_url,
                    ),
                    allowed_tools: if fallback_tools.is_empty() { vec!["*".to_string()] } else { fallback_tools },
                    skill_context: String::new(),
                }
            }
        }
    } else {
        // No McpPorter — derive allowed_tools from filtered skills' mcp_tools
        let fallback_tools: Vec<String> = if task_skills.is_empty() {
            vec![] // No skills selected = no tools
        } else {
            // Load skills from disk and collect their mcp_tools
            let skills_dir = &config.skills.managed_dir;
            let mut tools = Vec::new();
            if skills_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(skills_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if task_skills.iter().any(|s| s == &name) {
                            let skill_md = entry.path().join("SKILL.md");
                            if let Ok(content) = std::fs::read_to_string(&skill_md) {
                                if let Ok(skill) = mcclawd_core::skill_parser::parse_skill_md(&content) {
                                    tools.extend(skill.mcp_tools);
                                }
                            }
                        }
                    }
                }
            }
            tools
        };
        crate::sandbox::container::AgentEnvironment {
            image: "mcclawd-runner:latest".to_string(),
            network: config.sandbox.network.clone(),
            gateway_url: crate::sandbox::container::container_gateway_url(
                &config.mcp.agentgateway_url,
            ),
            allowed_tools: if fallback_tools.is_empty() { vec!["*".to_string()] } else { fallback_tools },
            skill_context: String::new(),
        }
    };

    // Audit: log tool access decision as a security event (best-effort, don't fail task creation)
    {
        let tool_audit = serde_json::json!({
            "selected_skills": task_skills,
            "allowed_tools": &agent_env.allowed_tools,
            "skill_count": task_skills.len(),
            "tool_count": agent_env.allowed_tools.len(),
        });
        let msg = format!(
            "Task granted {} tools from {} skills",
            agent_env.allowed_tools.len(),
            task_skills.len(),
        );
        if let Err(e) = state
            .pg_store
            .insert_security_event(
                Some(&task_id.0),
                "system",
                None,  // agent_id
                None,  // trace_id
                None,  // span_id
                "tool_access_granted",
                None,  // tool_name
                None,  // direction
                Some("info"),  // threat_level
                &tool_audit,
                &msg,
            )
            .await
        {
            tracing::warn!(
                task_id = %task_id.0,
                error = %e,
                "Failed to audit tool access decision"
            );
        }
    }

    // Persist resolved tools + skill context to DB (audit trail + restart/retry recovery).
    {
        let store = state.pg_store.clone();
        let tid = task_id.0.clone();
        let skills = task_skills.to_vec();
        let tools = agent_env.allowed_tools.clone();
        let ctx = agent_env.skill_context.clone();
        tokio::spawn(async move {
            if let Err(e) = store
                .update_task_tools(&tid, &skills, &tools, Some("general"), &ctx)
                .await
            {
                tracing::warn!(
                    task_id = %tid,
                    error = %e,
                    "Failed to persist task tools to postgres"
                );
            }
        });
    }

    // Check for task attachments directory
    let att_dir = config
        .data_dir
        .join("tasks")
        .join(&task_id.0)
        .join("attachments");
    let attachments_dir = if att_dir.is_dir() {
        Some(att_dir.to_string_lossy().to_string())
    } else {
        None
    };

    let _ = tx.send(OutboundChunk::TextDelta(
        "Creating runner container...".to_string(),
    ));

    let handle = match orchestrator
        .create_persistent_runner_container(
            &task_id,
            &agent_env,
            &sandbox_cfg.workspace_dir,
            &sandbox_cfg,
            &secrets_map,
            config.agent.max_turns,
            None,
            attachments_dir.as_deref(),
        )
        .await
    {
        Ok(h) => h,
        Err(e) => {
            let msg = format!("Failed to create runner container: {e}");
            state
                .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone()))
                .await;
            state
                .send_and_persist(&task_id, &tx, OutboundChunk::Done)
                .await;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg.clone());
            state
                .pg_update_status(&task_id, "Failed", Some(&msg))
                .await;
            return;
        }
    };

    tracing::info!(
        task_id = %task_id,
        container_id = %handle.container_id,
        "Persistent runner container started for agent task"
    );

    // Persist container tracking info to Postgres
    {
        let store = state.pg_store.clone();
        let tid = task_id.0.clone();
        let cid = handle.container_id.clone();
        let wdir = sandbox_cfg.workspace_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = store.update_container_info(&tid, &cid, "docker-runner").await {
                tracing::warn!("Failed to persist container info: {e}");
            }
            if let Err(e) = store.save_persistent_container(&cid, &tid, "task", &wdir).await {
                tracing::warn!("Failed to persist container record: {e}");
            }
        });
    }

    // Start background output reader → forwarder (lives as long as the container)
    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<OutboundChunk>(256);

    let reader_cid = handle.container_id.clone();
    tokio::spawn(async move {
        if let Ok(reader_orch) = SandboxOrchestrator::new() {
            if let Err(e) = reader_orch.stream_agent_output(&reader_cid, chunk_tx).await {
                tracing::warn!(error = %e, "Agent output streaming ended with error");
            }
        }
    });

    // Spawn forwarder: receives parsed chunks, handles Usage/ChatHistory specially,
    // broadcasts + persists all other chunks to WebSocket clients.
    let fwd_state = state.clone();
    let fwd_task_id = task_id.clone();
    let fwd_config = config.clone();
    let fwd_prompt = prompt.to_string();
    tokio::spawn(async move {
        while let Some(chunk) = chunk_rx.recv().await {
            // Look up current broadcast channel (may change on WS reconnect)
            let fwd_tx = {
                let streams = fwd_state.task_streams.read().await;
                streams.get(&fwd_task_id).cloned()
            };
            let Some(fwd_tx) = fwd_tx else { continue };

            match chunk {
                // Usage: extract and record, do NOT broadcast to WS
                OutboundChunk::Usage {
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    model,
                } => {
                    let model_name =
                        model.unwrap_or_else(|| fwd_config.agent.model.clone());
                    let prompt_preview: String = fwd_prompt.chars().take(50).collect();
                    let cost = mcclawd_core::providers::estimate_cost_usd(
                        &model_name,
                        input_tokens,
                        output_tokens,
                    );
                    {
                        let pool = fwd_state.provider_pool.read().await;
                        pool.record_usage_detailed(
                            "anthropic",
                            total_tokens,
                            input_tokens,
                            output_tokens,
                            cost,
                            Some((&fwd_task_id.0, &prompt_preview, &model_name)),
                        );
                    }
                    tracing::info!(
                        task_id = %fwd_task_id.0,
                        model = %model_name,
                        input_tokens, output_tokens, cost_usd = cost,
                        "Runner usage recorded"
                    );
                    // Persist to database
                    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                    let model_entry = mcclawd_core::providers::ModelUsageEntry {
                        model: model_name.clone(),
                        input_tokens,
                        output_tokens,
                        total_tokens,
                        estimated_cost_usd: cost,
                        request_count: 1,
                    };
                    let task_entry = mcclawd_core::providers::TaskUsageEntry {
                        task_id: fwd_task_id.0.clone(),
                        prompt_preview,
                        model: model_name,
                        total_tokens,
                        estimated_cost_usd: cost,
                    };
                    let store = fwd_state.pg_store.clone();
                    tokio::spawn(async move {
                        if let Err(e) = store
                            .upsert_daily_usage("admin", &today, cost, total_tokens)
                            .await
                        {
                            tracing::warn!("Failed to persist daily usage: {e}");
                        }
                        if let Err(e) =
                            store.upsert_model_usage("admin", &model_entry).await
                        {
                            tracing::warn!("Failed to persist model usage: {e}");
                        }
                        if let Err(e) =
                            store.insert_task_usage("admin", &task_entry).await
                        {
                            tracing::warn!("Failed to persist task usage: {e}");
                        }
                    });
                }
                // ChatHistory: deserialize and persist for multi-turn, do NOT broadcast
                OutboundChunk::ChatHistory(json) => {
                    match serde_json::from_str::<Vec<rig::completion::message::Message>>(
                        &json,
                    ) {
                        Ok(messages) => {
                            tracing::info!(
                                task_id = %fwd_task_id.0,
                                turns = messages.len(),
                                "Runner chat history received"
                            );
                            fwd_state.set_chat_history(&fwd_task_id, messages).await;
                        }
                        Err(e) => {
                            tracing::warn!(
                                task_id = %fwd_task_id.0,
                                error = %e,
                                "Failed to deserialize runner chat history"
                            );
                        }
                    }
                }
                // ToolStart: run security pipeline (audit/DLP) on tool calls from containers
                OutboundChunk::ToolStart { ref name } => {
                    // Set task context so events land with correct task_id
                    fwd_state.security_pipeline.set_task_context(&fwd_task_id.0).await;
                    // Run before_tool_call — for sandboxed agents this is post-hoc auditing
                    // (tool already executed in container), but it records DLP/audit events.
                    let args_json = serde_json::json!({"tool": name});
                    if let Err(e) = fwd_state.security_pipeline.before_tool_call(name, &args_json).await {
                        tracing::warn!(tool=%name, task_id=%fwd_task_id.0, error=%e, "Security pipeline flagged tool call");
                    }
                    // Still broadcast the ToolStart to the frontend
                    fwd_state
                        .send_and_persist(&fwd_task_id, &fwd_tx, chunk)
                        .await;
                }
                // ToolEnd: scan tool results for DLP/secrets via after_tool_call
                OutboundChunk::ToolEnd {
                    ref name,
                    ref summary,
                } => {
                    if let Some(text) = summary {
                        let result_json = serde_json::json!({"result": text});
                        if let Err(e) = fwd_state
                            .security_pipeline
                            .after_tool_call(name, &result_json)
                            .await
                        {
                            tracing::warn!(
                                tool = %name,
                                task_id = %fwd_task_id.0,
                                error = %e,
                                "Security pipeline flagged tool result"
                            );
                        }
                    }
                    fwd_state
                        .send_and_persist(&fwd_task_id, &fwd_tx, chunk)
                        .await;
                }
                // TextBlock: scan LLM responses for leaked secrets/PII
                OutboundChunk::TextBlock(ref text) => {
                    let text_json = serde_json::json!({"text": text});
                    fwd_state
                        .security_pipeline
                        .set_task_context(&fwd_task_id.0)
                        .await;
                    if let Err(e) = fwd_state
                        .security_pipeline
                        .after_tool_call("llm_response", &text_json)
                        .await
                    {
                        tracing::warn!(
                            task_id = %fwd_task_id.0,
                            error = %e,
                            "Security pipeline flagged LLM response"
                        );
                    }
                    fwd_state
                        .send_and_persist(&fwd_task_id, &fwd_tx, chunk)
                        .await;
                }
                // Done: update task status to Completed, then broadcast
                OutboundChunk::Done => {
                    {
                        let mut mgr = fwd_state.tasks.write().await;
                        mgr.complete_task(&fwd_task_id);
                    }
                    fwd_state
                        .pg_update_status(&fwd_task_id, "Completed", None)
                        .await;
                    fwd_state
                        .send_and_persist(&fwd_task_id, &fwd_tx, OutboundChunk::Done)
                        .await;
                }
                // All other chunks: broadcast + persist as normal
                other => {
                    fwd_state
                        .send_and_persist(&fwd_task_id, &fwd_tx, other)
                        .await;
                }
            }
        }
        tracing::info!(task_id = %fwd_task_id.0, "Task forwarder exiting");
    });

    // Store the persistent handle for future messages
    state
        .task_containers
        .write()
        .await
        .insert(task_id.clone(), handle.clone());

    // Send the first message to the container
    let chat_history = state.get_chat_history(&task_id).await;
    let history_json = if !chat_history.is_empty() {
        tracing::info!(task_id = %task_id.0, turns = chat_history.len(), "Resuming with conversation history");
        serde_json::to_string(&chat_history).ok()
    } else {
        None
    };

    if let Err(e) = handle
        .send_chat(prompt, history_json.as_deref())
        .await
    {
        let msg = format!("Failed to send initial message to container: {e}");
        state
            .send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone()))
            .await;
        state
            .send_and_persist(&task_id, &tx, OutboundChunk::Done)
            .await;
        state.task_containers.write().await.remove(&task_id);
        let mut mgr = state.tasks.write().await;
        mgr.fail_task(&task_id, msg.clone());
        state
            .pg_update_status(&task_id, "Failed", Some(&msg))
            .await;
    }

    // Response chunks are handled by the background forwarder.
    // The container stays running for follow-up messages!
}

/// Run agent in-process on the host (original behavior).
async fn run_agent_host(
    state: AppState,
    task_id: TaskId,
    prompt: &str,
    workspace_name: &str,
    task_skills: &[String],
    tx: tokio::sync::broadcast::Sender<OutboundChunk>,
) {
    // Persist the user message for history replay (human/assistant turn separation)
    state.send_and_persist(&task_id, &tx, OutboundChunk::UserMessage(prompt.to_string())).await;

    // Scan the initial prompt through DLP pipeline (inbound content scanning)
    {
        state
            .security_pipeline
            .set_task_context(&task_id.0)
            .await;
        let prompt_json = serde_json::json!({"prompt": prompt});
        if let Err(e) = state
            .security_pipeline
            .before_tool_call("user_prompt", &prompt_json)
            .await
        {
            tracing::warn!(
                task_id = %task_id.0,
                error = %e,
                "Security pipeline flagged user prompt (host)"
            );
        }
    }

    // Helper: broadcast-only (transient status, not persisted to history)
    let broadcast = |tx: &tokio::sync::broadcast::Sender<OutboundChunk>, chunk: OutboundChunk| {
        let _ = tx.send(chunk);
    };

    broadcast(&tx, OutboundChunk::TextDelta("Starting agent...".to_string()));

    // 1. Load workspace
    let config = state.config.read().await.clone();
    let loader = WorkspaceLoader::new(config.workspaces_dir());
    let workspace = match loader.load(workspace_name) {
        Ok(w) => w,
        Err(e) => {
            let msg = format!("Failed to load workspace: {e}");
            state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg.clone());
            state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
            return;
        }
    };
    broadcast(&tx, OutboundChunk::TextDelta("Workspace loaded".to_string()));

    // 1b. Check for attached files — text files injected as prompt text,
    //     images sent as multimodal content parts via rig's UserContent::Image
    let attachment_files = attachment_paths(&state, &task_id.0).await;
    let mut text_augmented = prompt.to_string();
    let mut image_parts: Vec<UserContent> = Vec::new();

    if !attachment_files.is_empty() {
        text_augmented.push_str("\n\n## Attached Files\n\n");
        for path in &attachment_files {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let mime = mime_guess::from_path(path).first_or_octet_stream().to_string();

            if let Some(image_media_type) = ImageMediaType::from_mime_type(&mime) {
                // Image files: read bytes, base64-encode, add as multimodal content
                match tokio::fs::read(path).await {
                    Ok(bytes) => {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        image_parts.push(UserContent::Image(Image {
                            data: DocumentSourceKind::Base64(b64),
                            media_type: Some(image_media_type),
                            detail: None,
                            additional_params: None,
                        }));
                        text_augmented.push_str(&format!(
                            "### Image: {} ({}, {}KB — sent as image content)\n\n",
                            name, mime, bytes.len() / 1024
                        ));
                    }
                    Err(_) => {
                        text_augmented.push_str(&format!("### File: {} (could not read)\n\n", name));
                    }
                }
            } else if mime.starts_with("text/") || mime.contains("json") || mime.contains("xml")
                || mime.contains("markdown") || mime.contains("yaml") || mime.contains("toml")
                || mime.contains("csv") || mime.contains("javascript") || mime.contains("typescript")
            {
                // Text files: read and include content
                match tokio::fs::read_to_string(path).await {
                    Ok(content) => {
                        let truncated = if content.len() > 50_000 { &content[..50_000] } else { &content };
                        // Escape triple backticks to prevent code-block breakout injection
                        let safe_content = truncated.replace("```", "` ` `");
                        text_augmented.push_str(&format!("### File: {}\n\n```\n{}\n```\n\n", name, safe_content));
                    }
                    Err(_) => {
                        text_augmented.push_str(&format!("### File: {} (could not read)\n\n", name));
                    }
                }
            } else {
                text_augmented.push_str(&format!("### File: {} ({}, {} — binary file, content not included)\n\n", name, mime,
                    match tokio::fs::metadata(path).await {
                        Ok(m) => format!("{}KB", m.len() / 1024),
                        Err(_) => "unknown size".to_string(),
                    }
                ));
            }
        }
    }

    // Build the prompt message — multimodal if images attached, plain text otherwise
    let prompt_message: RigMessage = if image_parts.is_empty() {
        text_augmented.as_str().into()
    } else {
        let mut parts = vec![UserContent::text(&text_augmented)];
        parts.extend(image_parts);
        RigMessage::User {
            content: OneOrMany::many(parts).unwrap_or_else(|_| OneOrMany::one(UserContent::text(&text_augmented))),
        }
    };
    let prompt = text_augmented.as_str();

    // 2. Get API key — try vault first, fall back to env var directly
    let api_key = {
        let vault_key = {
            let secrets_guard = state.secrets.read().await;
            match secrets_guard.as_ref() {
                Some(backend) => backend.get("ANTHROPIC_API_KEY").await.ok().flatten(),
                None => None,
            }
        };
        // Prefer vault, fall back to env var (belt + suspenders)
        let key = vault_key
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        match key {
            Some(k) => k,
            None => {
                let msg = "ANTHROPIC_API_KEY not found. Set it in Config > Secrets or .env file.".to_string();
                state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
                state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                let mut mgr = state.tasks.write().await;
                mgr.fail_task(&task_id, msg.clone());
                state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
                return;
            }
        }
    };
    broadcast(&tx, OutboundChunk::TextDelta("Credentials verified".to_string()));

    // 3. Build the agent
    broadcast(&tx, OutboundChunk::TextDelta("Building agent...".to_string()));
    let pipeline = Some(state.security_pipeline.clone());
    // Pass task_skills as filter: empty = no skills, non-empty = only those skills
    let skill_filter = Some(task_skills.to_vec());
    let (agent, _memory, _mcp_conns) = match AgentEngine::build_with_skill_filter(workspace, &api_key, config.agent.max_turns, &config, pipeline, skill_filter).await {
        Ok(result) => result,
        Err(e) => {
            let msg = format!("Failed to build agent: {e}");
            state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
            state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
            let mut mgr = state.tasks.write().await;
            mgr.fail_task(&task_id, msg.clone());
            state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
            return;
        }
    };

    // Report MCP tool availability
    let tool_count = _mcp_conns.iter().map(|b| b.tools.len()).sum::<usize>();
    if tool_count > 0 {
        broadcast(&tx, OutboundChunk::TextDelta(format!("{tool_count} MCP tools available")));
    } else {
        broadcast(&tx, OutboundChunk::TextDelta("No MCP tools connected (is AgentGateway running?)".to_string()));
    }

    // 4. Stream with conversation history — enables multi-turn follow-ups
    // Persist Processing so it replays on WS reconnect — frontend needs it to enter streaming mode
    state.send_and_persist(&task_id, &tx, OutboundChunk::StatusIndicator(ChannelStatus::Processing)).await;

    let chat_history = state.get_chat_history(&task_id).await;
    if !chat_history.is_empty() {
        tracing::info!(task_id = %task_id.0, turns = chat_history.len(), "Resuming with conversation history");
    }

    let mut stream = agent.stream_chat(prompt_message, chat_history.clone()).await;

    // Accumulate full response text for clean history persistence
    let mut accumulated_text = String::new();

    while let Some(item) = stream.next().await {
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => {
                match content {
                    StreamedAssistantContent::Text(text) => {
                        // Broadcast delta for live streaming UX (not persisted)
                        broadcast(&tx, OutboundChunk::TextDelta(text.text.clone()));
                        accumulated_text.push_str(&text.text);
                    }
                    StreamedAssistantContent::ToolCall { tool_call, .. } => {
                        // Associate this tool call with the current task so DLP/audit
                        // events land in security_events with the correct task_id.
                        state.security_pipeline.set_task_context(&task_id.0).await;
                        // Run security pipeline before tool execution
                        let args_json = serde_json::to_value(&tool_call.function.arguments)
                            .unwrap_or_default();
                        if let Err(e) = state.security_pipeline.before_tool_call(
                            &tool_call.function.name, &args_json,
                        ).await {
                            tracing::warn!(
                                tool = %tool_call.function.name,
                                error = %e,
                                "Security pipeline blocked tool call"
                            );
                        }
                        // Persist tool calls so history shows them
                        state.send_and_persist(&task_id, &tx, OutboundChunk::ToolStart { name: tool_call.function.name.clone() }).await;
                    }
                    _ => {} // Reasoning, ToolCallDelta, Final, non_exhaustive
                }
            }
            Ok(MultiTurnStreamItem::StreamUserItem(_)) => {
                // Tool results auto-injected by Rig.
                // TODO: Rig doesn't expose tool results in the stream, so we can't
                // scan them here. The GuardedTool wrapper in engine.rs already calls
                // after_tool_call for builtin tools. For MCP tools routed through
                // AgentGateway, consider adding after_tool_call in the MCP client layer.
            }
            Ok(MultiTurnStreamItem::FinalResponse(final_resp)) => {
                // Record token usage from the LLM response
                let usage = final_resp.usage();
                let model_name = config.agent.model.clone();
                let prompt_preview: String = prompt.chars().take(50).collect();
                let cost = mcclawd_core::providers::estimate_cost_usd(
                    &model_name,
                    usage.input_tokens,
                    usage.output_tokens,
                );
                {
                    let pool = state.provider_pool.read().await;
                    pool.record_usage_detailed(
                        "anthropic",
                        usage.total_tokens,
                        usage.input_tokens,
                        usage.output_tokens,
                        cost,
                        Some((&task_id.0, &prompt_preview, &model_name)),
                    );
                }
                tracing::info!(
                    task_id = %task_id.0,
                    model = %model_name,
                    input_tokens = usage.input_tokens,
                    output_tokens = usage.output_tokens,
                    cost_usd = cost,
                    "Usage recorded"
                );

                // Persist usage to database
                {
                    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                    let model_entry = mcclawd_core::providers::ModelUsageEntry {
                        model: model_name.clone(),
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        total_tokens: usage.total_tokens,
                        estimated_cost_usd: cost,
                        request_count: 1,
                    };
                    let task_entry = mcclawd_core::providers::TaskUsageEntry {
                        task_id: task_id.0.clone(),
                        prompt_preview: prompt_preview.clone(),
                        model: model_name.clone(),
                        total_tokens: usage.total_tokens,
                        estimated_cost_usd: cost,
                    };
                    let store = state.pg_store.clone();
                    let today_c = today.clone();
                    // Spawn DB writes so they don't block streaming
                    tokio::spawn(async move {
                        if let Err(e) = store.upsert_daily_usage("admin", &today_c, cost, usage.total_tokens).await {
                            tracing::warn!("Failed to persist daily usage: {e}");
                        }
                        if let Err(e) = store.upsert_model_usage("admin", &model_entry).await {
                            tracing::warn!("Failed to persist model usage: {e}");
                        }
                        if let Err(e) = store.insert_task_usage("admin", &task_entry).await {
                            tracing::warn!("Failed to persist task usage: {e}");
                        }
                    });
                }

                // Persist the complete response as a TextBlock for clean history replay
                if !accumulated_text.is_empty() {
                    state.persist_only(&task_id, OutboundChunk::TextBlock(accumulated_text.clone())).await;
                }

                // Persist conversation history for follow-ups
                if let Some(history) = final_resp.history() {
                    state.set_chat_history(&task_id, history.to_vec()).await;
                    tracing::debug!(task_id = %task_id.0, messages = history.len(), "Chat history persisted");
                } else {
                    // Fallback: manually append user + assistant messages
                    let mut history = chat_history.clone();
                    history.push(RigMessage::user(prompt));
                    history.push(RigMessage::assistant(&accumulated_text));
                    state.set_chat_history(&task_id, history).await;
                    tracing::debug!(task_id = %task_id.0, "Chat history persisted (manual fallback)");
                }

                broadcast(&tx, OutboundChunk::StatusIndicator(ChannelStatus::Done));
                state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                let mut mgr = state.tasks.write().await;
                mgr.complete_task(&task_id);
                state.pg_update_status(&task_id, "Completed", None).await;
            }
            Err(e) => {
                let msg = format!("Streaming error: {e}");
                state.send_and_persist(&task_id, &tx, OutboundChunk::Error(msg.clone())).await;
                state.send_and_persist(&task_id, &tx, OutboundChunk::Done).await;
                let mut mgr = state.tasks.write().await;
                mgr.fail_task(&task_id, msg.clone());
                state.pg_update_status(&task_id, "Failed", Some(&msg)).await;
                return;
            }
            _ => {} // non_exhaustive guard
        }
    }
}

/// GET /api/tasks/{id} — get single task
pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TaskResponse>, StatusCode> {
    let task_id = TaskId(id.clone());

    // Try in-memory first
    {
        let mgr = state.tasks.read().await;
        if let Some(task) = mgr.get_task(&task_id) {
            return Ok(Json(TaskResponse::from(task)));
        }
    }

    // Fall back to Postgres (handles cargo-watch restarts where hydration raced)
    if let Ok(Some((_, prompt, status, error_message, tags, selected_skills, allowed_tools, tool_profile, skill_context))) =
        state.pg_store.get_task(&id).await
    {
        let task_status =
            crate::commands::serve::row_to_status(&status, error_message.as_deref());
        let mut mgr = state.tasks.write().await;
        mgr.hydrate_task(task_id.clone(), prompt, task_status, tags, selected_skills, allowed_tools, tool_profile, skill_context);
        if let Some(task) = mgr.get_task(&task_id) {
            return Ok(Json(TaskResponse::from(task)));
        }
    }

    Err(StatusCode::NOT_FOUND)
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub message: String,
    /// If set, truncate chat history to this many messages before appending.
    /// Used for edit/retry: discard everything after the edited message.
    #[serde(default)]
    pub truncate_history_to: Option<usize>,
}

/// POST /api/tasks/{id}/message — send a follow-up message to an existing task
pub async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> Result<StatusCode, StatusCode> {
    let sanitized = mcclawd_core::sanitize_prompt(&body.message);
    if sanitized.was_modified {
        tracing::warn!(
            patterns = ?sanitized.detected_patterns,
            "Prompt injection patterns detected and stripped from follow-up message"
        );
    }
    let message = sanitized.text;

    // Agent security: validate follow-up message against security policies
    if let Err(reason) = agent_security::validate_task_prompt(&message) {
        tracing::warn!(reason = %reason, "Follow-up message blocked by agent security");
        return Err(StatusCode::BAD_REQUEST);
    }

    let task_id = TaskId(id);

    // Verify task exists
    {
        let mgr = state.tasks.read().await;
        if mgr.get_task(&task_id).is_none() {
            return Err(StatusCode::NOT_FOUND);
        }
    }

    // Get existing broadcast channel, or create a new one if the old one was dropped
    let tx = {
        let streams = state.task_streams.read().await;
        streams.get(&task_id).cloned()
    };
    let tx = match tx {
        Some(tx) => tx,
        None => state.create_task_stream(&task_id).await,
    };

    // Truncate chat history if requested (edit/retry — discard messages after edit point)
    if let Some(keep) = body.truncate_history_to {
        let mut history = state.get_chat_history(&task_id).await;
        if keep < history.len() {
            tracing::info!(task_id = %task_id.0, keep, total = history.len(), "Truncating chat history for edit/retry");
            history.truncate(keep);
            state.set_chat_history(&task_id, history).await;
        }
        // Also truncate persisted events
        let mut events = state.get_task_events(&task_id).await;
        // Keep only events up to the Nth UserMessage
        let mut user_msg_count = 0;
        let mut cut_at = events.len();
        for (i, ev) in events.iter().enumerate() {
            if matches!(ev, mcclawd_channels::OutboundChunk::UserMessage(_)) {
                user_msg_count += 1;
                // keep = number of chat turns (user+assistant pairs) to keep
                // Each UserMessage starts a new turn
                if user_msg_count > keep / 2 {
                    cut_at = i;
                    break;
                }
            }
        }
        if cut_at < events.len() {
            events.truncate(cut_at);
            state.set_task_events(&task_id, events).await;
        }
    }

    // Mark task as running again
    {
        let mut mgr = state.tasks.write().await;
        mgr.running(&task_id);
    }
    state.pg_update_status(&task_id, "Running", None).await;

    // Spawn agent execution for the follow-up message
    let workspace_name = "default".to_string();
    let task_skills = {
        let skills_map = state.task_skills.read().await;
        skills_map.get(&task_id).cloned().unwrap_or_default()
    };
    let state_clone = state.clone();
    let tid = task_id.clone();
    tokio::spawn(async move {
        run_agent(state_clone, tid, &message, &workspace_name, &task_skills, tx).await;
    });

    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Deserialize)]
pub struct DeleteAllQuery {
    /// Optional status filter: "Completed", "Failed", "Running", "Pending"
    pub status: Option<String>,
    /// Optional tag filter: delete only tasks with this tag
    pub tag: Option<String>,
}

/// DELETE /api/tasks — delete all tasks (or filter by ?status=... and/or ?tag=...)
pub async fn delete_all_tasks(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<DeleteAllQuery>,
) -> Json<serde_json::Value> {
    let mut mgr = state.tasks.write().await;

    // If tag filter is provided, use the dedicated delete_by_tag method
    // (optionally combined with status filter)
    let to_delete: Vec<TaskId> = if let Some(ref tag) = query.tag {
        let all = mgr.all_tasks().iter().map(|t| (t.id.clone(), t.status.clone(), t.tags.clone())).collect::<Vec<_>>();
        all.into_iter()
            .filter(|(_, status, tags)| {
                let tag_match = tags.iter().any(|tg| tg == tag);
                let status_match = if let Some(ref filter) = query.status {
                    let status_str = match status {
                        TaskStatus::Pending => "Pending",
                        TaskStatus::Building => "Building",
                        TaskStatus::Running => "Running",
                        TaskStatus::Completed => "Completed",
                        TaskStatus::Failed(_) => "Failed",
                        TaskStatus::Restarting { .. } => "Restarting",
                        TaskStatus::SwarmRunning { .. } => "SwarmRunning",
                    };
                    status_str.eq_ignore_ascii_case(filter)
                } else {
                    true
                };
                tag_match && status_match
            })
            .map(|(id, _, _)| id)
            .collect()
    } else {
        let all = mgr.all_tasks().iter().map(|t| (t.id.clone(), t.status.clone())).collect::<Vec<_>>();
        all.into_iter()
            .filter(|(_, status)| {
                if let Some(ref filter) = query.status {
                    let status_str = match status {
                        TaskStatus::Pending => "Pending",
                        TaskStatus::Building => "Building",
                        TaskStatus::Running => "Running",
                        TaskStatus::Completed => "Completed",
                        TaskStatus::Failed(_) => "Failed",
                        TaskStatus::Restarting { .. } => "Restarting",
                        TaskStatus::SwarmRunning { .. } => "SwarmRunning",
                    };
                    status_str.eq_ignore_ascii_case(filter)
                } else {
                    true
                }
            })
            .map(|(id, _)| id)
            .collect()
    };

    let count = to_delete.len();
    for id in &to_delete {
        mgr.delete_task(id);
    }
    drop(mgr);

    // Clean up each deleted task: in-memory state, Docker containers, then atomic DB cascade
    for id in &to_delete {
        // Clean up in-memory state
        state.task_streams.write().await.remove(id);
        state.task_chat_history.write().await.remove(id);
        state.task_events.write().await.remove(id);

        // Shutdown Docker container (Docker API calls — not DB)
        let handle = state.task_containers.write().await.remove(id);
        if let Some(handle) = handle {
            let _ = handle.shutdown().await;
            let cid = handle.container_id.clone();
            if let Ok(orch) = crate::sandbox::SandboxOrchestrator::new() {
                let _ = orch.cleanup_container(&cid).await;
            }
        } else {
            // No in-memory handle — check DB for orphaned Docker containers
            if let Ok(container_ids) = state.pg_store.get_container_ids_by_task(&id.0).await {
                for cid in &container_ids {
                    tracing::info!(task_id = %id.0, container_id = %cid,
                        "Cleaning up orphaned container from DB (bulk delete)");
                    if let Ok(orch) = crate::sandbox::SandboxOrchestrator::new() {
                        let _ = orch.cleanup_container(cid).await;
                    }
                }
            }
        }

        // Cascade-delete from postgres atomically (task + security_events + dlp_findings + persistent_containers)
        state.pg_delete_task_sync(id).await;
    }

    Json(serde_json::json!({ "deleted": count }))
}

/// DELETE /api/tasks/{id} — cancel running task or remove completed/failed task
pub async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    let mut mgr = state.tasks.write().await;
    let task_id = TaskId(id);

    // Return 404 if task doesn't exist
    let task = match mgr.get_task(&task_id) {
        Some(t) => t.clone(),
        None => return StatusCode::NOT_FOUND,
    };

    if matches!(task.status, TaskStatus::Running | TaskStatus::Building) {
        mgr.fail_task(&task_id, "Cancelled by user".to_string());
    }

    mgr.delete_task(&task_id);
    drop(mgr);

    // Shutdown Docker containers (Docker API calls only — DB cleanup handled by cascade below)
    {
        let mut containers = state.task_containers.write().await;
        let in_memory_handle = containers.remove(&task_id);

        if let Some(handle) = in_memory_handle {
            // In-memory handle exists — shutdown gracefully and cleanup Docker container
            let _ = handle.shutdown().await;
            let cid = handle.container_id.clone();
            if let Ok(orch) = crate::sandbox::SandboxOrchestrator::new() {
                let _ = orch.cleanup_container(&cid).await;
            }
        } else {
            // No in-memory handle (e.g. server restarted) — check DB for orphaned containers
            if let Ok(container_ids) = state
                .pg_store
                .get_container_ids_by_task(&task_id.0)
                .await
            {
                for cid in &container_ids {
                    tracing::info!(task_id = %task_id.0, container_id = %cid,
                        "Cleaning up orphaned container from DB (no in-memory handle)");
                    if let Ok(orch) = crate::sandbox::SandboxOrchestrator::new() {
                        let _ = orch.cleanup_container(cid).await;
                    }
                }
            }
        }
    }

    // Cascade-delete from postgres atomically (task + security_events + dlp_findings + persistent_containers)
    state.pg_delete_task_sync(&task_id).await;

    // Clean up broadcast channel + chat history + event cache
    state.task_streams.write().await.remove(&task_id);
    state.task_chat_history.write().await.remove(&task_id);
    state.task_events.write().await.remove(&task_id);

    StatusCode::NO_CONTENT
}

// ── Attachments ─────────────────────────────────────────────────────────────

/// Sanitize a filename: strip path separators and `..` to prevent traversal.
fn sanitize_filename(name: &str) -> String {
    name.replace(['/', '\\'], "")
        .replace("..", "")
        .trim()
        .to_string()
}

/// Resolve the attachments directory for a given task.
async fn attachments_dir(state: &AppState, task_id: &str) -> PathBuf {
    let config = state.config.read().await;
    config.data_dir.join("tasks").join(task_id).join("attachments")
}

/// POST /api/tasks/{id}/attachments — upload one or more files
pub async fn upload_attachments(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: axum::extract::Request,
) -> Result<Json<Vec<AttachmentMeta>>, (StatusCode, String)> {
    // Extract multipart manually so we can log the exact rejection reason (BUG-031 fix).
    // The bare `Multipart` extractor returns 400 with no diagnostic info on failure.
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let mut multipart =
        Multipart::from_request(request, &state)
            .await
            .map_err(|rejection| {
                tracing::error!(
                    task_id = %id,
                    content_type = %content_type,
                    rejection = %rejection,
                    "Multipart extraction failed — check Content-Type header"
                );
                (
                    rejection.status(),
                    format!(
                        "Multipart parse error: {rejection}. Content-Type was: {content_type}"
                    ),
                )
            })?;

    let dir = attachments_dir(&state, &id).await;
    tokio::fs::create_dir_all(&dir).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to create attachments dir");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create attachments dir: {e}"),
        )
    })?;

    let mut results = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        tracing::error!(error = %e, task_id = %id, "Multipart next_field error");
        (
            StatusCode::BAD_REQUEST,
            format!("Multipart field error: {e}"),
        )
    })? {
        let original_name = field.file_name().unwrap_or("unnamed").to_string();
        let safe_name = sanitize_filename(&original_name);
        if safe_name.is_empty() {
            continue;
        }

        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        let data = field.bytes().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to read multipart field");
            (
                StatusCode::BAD_REQUEST,
                format!("Failed to read field bytes: {e}"),
            )
        })?;

        let file_path = dir.join(&safe_name);
        tokio::fs::write(&file_path, &data).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to write attachment");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to write attachment: {e}"),
            )
        })?;

        results.push(AttachmentMeta {
            name: safe_name.clone(),
            size: data.len() as u64,
            content_type,
            url: format!("/api/tasks/{id}/attachments/{safe_name}"),
        });
    }

    tracing::info!(task_id = %id, count = results.len(), "Attachments uploaded");

    // Emit attachment event to the task stream for conversation history
    let attachment_infos: Vec<mcclawd_channels::AttachmentInfo> = results
        .iter()
        .map(|a| mcclawd_channels::AttachmentInfo {
            name: a.name.clone(),
            size: a.size,
            content_type: a.content_type.clone(),
            url: a.url.clone(),
        })
        .collect();

    // Persist attachment info for history replay, but do NOT broadcast —
    // the frontend already shows local thumbnails during the current session.
    // Broadcasting would cause duplicates (Bug: image attachments shown twice).
    if !attachment_infos.is_empty() {
        let chunk = OutboundChunk::Attachments(attachment_infos);
        let task_id_typed = TaskId(id.clone());
        state.persist_only(&task_id_typed, chunk).await;
    }

    Ok(Json(results))
}

/// GET /api/tasks/{id}/attachments — list all attachments for a task
pub async fn list_attachments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AttachmentMeta>>, StatusCode> {
    let dir = attachments_dir(&state, &id).await;
    if !dir.exists() {
        return Ok(Json(Vec::new()));
    }

    let mut entries = tokio::fs::read_dir(&dir).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to read attachments dir");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut results = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(meta) = entry.metadata().await {
            if meta.is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                let content_type = mime_guess::from_path(&name)
                    .first_or_octet_stream()
                    .to_string();
                results.push(AttachmentMeta {
                    url: format!("/api/tasks/{id}/attachments/{name}"),
                    name,
                    size: meta.len(),
                    content_type,
                });
            }
        }
    }

    Ok(Json(results))
}

/// GET /api/tasks/{id}/attachments/{filename} — download/serve a single attachment
pub async fn download_attachment(
    State(state): State<AppState>,
    Path((id, filename)): Path<(String, String)>,
) -> Result<Response<Body>, StatusCode> {
    let safe_name = sanitize_filename(&filename);
    if safe_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let dir = attachments_dir(&state, &id).await;
    let file_path = dir.join(&safe_name);

    let file = tokio::fs::File::open(&file_path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let stream = ReaderStream::new(file);

    let content_type = mime_guess::from_path(&safe_name)
        .first_or_octet_stream()
        .to_string();

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("inline; filename=\"{safe_name}\""),
        )
        .body(Body::from_stream(stream))
        .unwrap())
}

/// List attachment file paths for a task (used to inject into agent context).
pub async fn attachment_paths(state: &AppState, task_id: &str) -> Vec<PathBuf> {
    let dir = attachments_dir(state, task_id).await;
    if !dir.exists() {
        return Vec::new();
    }
    let mut paths = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.metadata().await.map(|m| m.is_file()).unwrap_or(false) {
                paths.push(entry.path());
            }
        }
    }
    paths
}

// ── Generated Files (container output) ──────────────────────────────────────

/// Resolve the output files directory for a given task.
async fn output_files_dir(state: &AppState, task_id: &str) -> PathBuf {
    let config = state.config.read().await;
    config.data_dir.join("tasks").join(task_id).join("output")
}

/// GET /api/tasks/{id}/files — list all generated files for a task
pub async fn list_generated_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AttachmentMeta>>, StatusCode> {
    let dir = output_files_dir(&state, &id).await;
    if !dir.exists() {
        return Ok(Json(Vec::new()));
    }

    let mut entries = tokio::fs::read_dir(&dir).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to read output files dir");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut results = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(meta) = entry.metadata().await {
            if meta.is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                let content_type = mime_guess::from_path(&name)
                    .first_or_octet_stream()
                    .to_string();
                results.push(AttachmentMeta {
                    url: format!("/api/tasks/{id}/files/{name}"),
                    name,
                    size: meta.len(),
                    content_type,
                });
            }
        }
    }

    Ok(Json(results))
}

/// GET /api/tasks/{id}/files/{filename} — download/serve a single generated file
pub async fn download_generated_file(
    State(state): State<AppState>,
    Path((id, filename)): Path<(String, String)>,
) -> Result<Response<Body>, StatusCode> {
    let safe_name = sanitize_filename(&filename);
    if safe_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let dir = output_files_dir(&state, &id).await;
    let file_path = dir.join(&safe_name);

    // Security: ensure the resolved path is within the output dir
    let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());
    if let Ok(canonical_file) = file_path.canonicalize() {
        if !canonical_file.starts_with(&canonical_dir) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let file = tokio::fs::File::open(&file_path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let stream = ReaderStream::new(file);

    let content_type = mime_guess::from_path(&safe_name)
        .first_or_octet_stream()
        .to_string();

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("inline; filename=\"{safe_name}\""),
        )
        .body(Body::from_stream(stream))
        .unwrap())
}

/// Response for GET /api/tasks/{id}/container
#[derive(Debug, Serialize)]
pub struct ContainerInfoResponse {
    pub task_id: String,
    pub container_id: Option<String>,
    pub execution_mode: String,
    pub base_image: String,
    pub network: String,
    pub strict_sandbox: bool,
    pub pids_limit: Option<i64>,
    pub memory_limit: Option<i64>,
}

/// GET /api/tasks/{id}/container — get container isolation info for a task.
///
/// Container metadata persists in Postgres even after the container is removed.
/// History and artifacts are always persisted independently of container lifecycle.
/// POST /api/transcribe — speech-to-text via ElevenLabs STT API
pub async fn transcribe_audio(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // 1. Extract audio file from multipart
    let mut audio_data: Option<Vec<u8>> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("audio") {
            audio_data = Some(field.bytes().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to read audio field");
                StatusCode::BAD_REQUEST
            })?.to_vec());
            break;
        }
    }
    let audio_bytes = audio_data.ok_or(StatusCode::BAD_REQUEST)?;

    // 2. Get ElevenLabs API key from vault
    let api_key = {
        let secrets = state.secrets.read().await;
        match secrets.as_ref() {
            Some(backend) => match backend.get("ELEVENLABS_API_KEY").await {
                Ok(Some(key)) if !key.is_empty() => key,
                _ => {
                    return Ok(Json(serde_json::json!({
                        "error": "ELEVENLABS_API_KEY not set"
                    })));
                }
            },
            None => {
                return Ok(Json(serde_json::json!({ "error": "Vault locked" })));
            }
        }
    };

    // 3. Call ElevenLabs Speech-to-Text API
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::bytes(audio_bytes)
        .file_name("audio.webm")
        .mime_str("application/octet-stream")
        .unwrap();
    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model_id", "scribe_v2");

    let res = client
        .post("https://api.elevenlabs.io/v1/speech-to-text")
        .header("xi-api-key", &api_key)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    match res {
        Ok(r) if r.status().is_success() => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
            Ok(Json(serde_json::json!({ "text": text })))
        }
        Ok(r) => {
            let status = r.status().as_u16();
            let body = r.text().await.unwrap_or_default();
            tracing::error!(status, body = %body, "ElevenLabs STT failed");
            Ok(Json(serde_json::json!({ "error": format!("ElevenLabs {status}: {body}") })))
        }
        Err(e) => {
            tracing::error!(error = %e, "ElevenLabs STT network error");
            Ok(Json(serde_json::json!({ "error": format!("Network error: {e}") })))
        }
    }
}

pub async fn get_container_info(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ContainerInfoResponse>, StatusCode> {
    // Verify task exists
    let task_id = TaskId(id.clone());
    {
        let mgr = state.tasks.read().await;
        if mgr.get_task(&task_id).is_none() {
            return Err(StatusCode::NOT_FOUND);
        }
    }

    // Try to load container info from Postgres
    let (container_id, execution_mode) = match state
        .pg_store
        .get_container_info(&id)
        .await
    {
        Ok(Some((cid, mode))) => (Some(cid), mode),
        Ok(None) => (None, "docker".to_string()),
        Err(_) => (None, "docker".to_string()),
    };

    let config = state.config.read().await;
    Ok(Json(ContainerInfoResponse {
        task_id: id,
        container_id,
        execution_mode,
        base_image: config.sandbox.base_image.clone(),
        network: config.sandbox.network.clone(),
        strict_sandbox: config.sandbox.strict_sandbox,
        pids_limit: config.sandbox.pids_limit,
        memory_limit: config.sandbox.memory_limit,
    }))
}

