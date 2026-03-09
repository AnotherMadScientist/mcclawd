use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use bollard::Docker;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use super::state::AppState;

// ── Types ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunnerBuildState {
    pub status: BuildStatus,
    pub progress_pct: u8,
    pub logs: Vec<String>,
    pub error: Option<String>,
    pub image_available: bool,
    pub image_id: Option<String>,
    pub image_size: Option<u64>,
    /// Build duration in seconds (set when build completes or image found).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_duration_secs: Option<f64>,
    /// System agent container startup time in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_startup_secs: Option<f64>,
    /// Unix timestamp when the build/check started.
    #[serde(skip)]
    pub started_at: Option<std::time::Instant>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BuildStatus {
    Checking,
    ImageReady,
    Building,
    Complete,
    Failed,
}

impl Default for RunnerBuildState {
    fn default() -> Self {
        Self {
            status: BuildStatus::Checking,
            progress_pct: 0,
            logs: Vec::new(),
            error: None,
            image_available: false,
            image_id: None,
            image_size: None,
            build_duration_secs: None,
            agent_startup_secs: None,
            started_at: Some(std::time::Instant::now()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ContainerAttachmentMeta {
    pub name: String,
    pub size: u64,
    pub is_image: bool,
}

#[derive(Debug, Serialize)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub task_id: Option<String>,
    pub status: String,
    pub state: String,
    pub image: String,
    pub created: i64,
    pub ports: Vec<String>,
    pub mounts: Vec<MountInfo>,
    pub env_vars: HashMap<String, String>,
    pub labels: HashMap<String, String>,
    pub attachments: Vec<ContainerAttachmentMeta>,
    pub skills: Vec<String>,
    pub mcp_tools: Vec<String>,
    pub gateway_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MountInfo {
    pub source: String,
    pub destination: String,
    pub mode: String,
}

// ── Background build job ───────────────────────────────────────────

/// Check if `mcclawd-runner:latest` exists and populate image metadata.
async fn check_image(docker: &Docker, build_state: &Arc<RwLock<RunnerBuildState>>) -> bool {
    match docker.inspect_image("mcclawd-runner:latest").await {
        Ok(info) => {
            let mut state = build_state.write().await;
            state.build_duration_secs = state.started_at.map(|s| s.elapsed().as_secs_f64());
            state.status = BuildStatus::ImageReady;
            state.image_available = true;
            state.progress_pct = 100;
            state.image_id = info.id.clone();
            state.image_size = info.size.map(|s| s as u64);
            state.logs.push("Runner image already available.".into());
            if let Some(ref id) = info.id {
                state
                    .logs
                    .push(format!("Image ID: {}", &id[..std::cmp::min(id.len(), 19)]));
            }
            if let Some(size) = info.size {
                state
                    .logs
                    .push(format!("Image size: {:.1} MB", size as f64 / 1_048_576.0));
            }
            true
        }
        Err(_) => false,
    }
}

/// Spawn the background image build. Called from server startup.
pub fn spawn_runner_build(build_state: Arc<RwLock<RunnerBuildState>>, project_root: PathBuf) {
    tokio::spawn(async move {
        let docker = match Docker::connect_with_local_defaults() {
            Ok(d) => d,
            Err(e) => {
                let mut state = build_state.write().await;
                state.status = BuildStatus::Failed;
                state.error = Some(format!("Docker not available: {e}"));
                state
                    .logs
                    .push(format!("ERROR: Docker connection failed: {e}"));
                return;
            }
        };

        // Check if image already exists
        if check_image(&docker, &build_state).await {
            return;
        }

        // Image not found — start building
        {
            let mut state = build_state.write().await;
            state.status = BuildStatus::Building;
            state.progress_pct = 5;
            state
                .logs
                .push("Runner image not found. Starting build...".into());
            state
                .logs
                .push(format!("Project root: {}", project_root.display()));
        }

        run_docker_build(&build_state, &project_root).await;
    });
}

/// Run `docker build` via CLI (supports BuildKit cache mounts).
async fn run_docker_build(build_state: &Arc<RwLock<RunnerBuildState>>, project_root: &PathBuf) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    let dockerfile = project_root.join("docker/agent-runner/Dockerfile");
    if !dockerfile.exists() {
        let mut state = build_state.write().await;
        state.status = BuildStatus::Failed;
        state.error = Some("Dockerfile not found at docker/agent-runner/Dockerfile".into());
        state.logs.push("ERROR: Dockerfile not found".into());
        return;
    }

    {
        let mut state = build_state.write().await;
        state.progress_pct = 10;
        state
            .logs
            .push("Running: docker build -t mcclawd-runner:latest ...".into());
    }

    let mut child = match Command::new("docker")
        .env("DOCKER_BUILDKIT", "1")
        .arg("build")
        .arg("-t")
        .arg("mcclawd-runner:latest")
        .arg("-f")
        .arg(&dockerfile)
        .arg(project_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let mut state = build_state.write().await;
            state.status = BuildStatus::Failed;
            state.error = Some(format!("Failed to spawn docker build: {e}"));
            state.logs.push(format!("ERROR: {e}"));
            return;
        }
    };

    // Stream stderr (docker build output goes to stderr with BuildKit)
    let stderr = child.stderr.take();
    let build_state_clone = build_state.clone();
    let stderr_handle = tokio::spawn(async move {
        if let Some(stderr) = stderr {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            let mut line_count = 0u32;
            while let Ok(Some(line)) = lines.next_line().await {
                line_count += 1;
                let mut state = build_state_clone.write().await;
                // Estimate progress from build output (heuristic)
                let pct = std::cmp::min(10 + (line_count as u8).saturating_mul(2), 90);
                state.progress_pct = pct;
                state.logs.push(line);
            }
        }
    });

    // Also capture stdout
    let stdout = child.stdout.take();
    let build_state_clone2 = build_state.clone();
    let stdout_handle = tokio::spawn(async move {
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut state = build_state_clone2.write().await;
                state.logs.push(line);
            }
        }
    });

    let _ = stderr_handle.await;
    let _ = stdout_handle.await;

    match child.wait().await {
        Ok(status) if status.success() => {
            // Verify the image was created
            let docker = Docker::connect_with_local_defaults().ok();
            let mut state = build_state.write().await;
            state.build_duration_secs = state.started_at.map(|s| s.elapsed().as_secs_f64());
            state.status = BuildStatus::Complete;
            state.image_available = true;
            state.progress_pct = 100;
            if let Some(dur) = state.build_duration_secs {
                state.logs.push(format!("Build completed in {dur:.1}s."));
            } else {
                state.logs.push("Build completed successfully.".into());
            }

            // Fetch image metadata
            if let Some(docker) = docker {
                if let Ok(info) = docker.inspect_image("mcclawd-runner:latest").await {
                    state.image_id = info.id.clone();
                    state.image_size = info.size.map(|s| s as u64);
                    if let Some(ref id) = info.id {
                        state
                            .logs
                            .push(format!("Image ID: {}", &id[..std::cmp::min(id.len(), 19)]));
                    }
                    if let Some(size) = info.size {
                        state
                            .logs
                            .push(format!("Image size: {:.1} MB", size as f64 / 1_048_576.0));
                    }
                }
            }
        }
        Ok(status) => {
            let mut state = build_state.write().await;
            state.status = BuildStatus::Failed;
            state.error = Some(format!(
                "docker build exited with code {}",
                status.code().unwrap_or(-1)
            ));
            state.logs.push(format!(
                "Build FAILED (exit code {})",
                status.code().unwrap_or(-1)
            ));
        }
        Err(e) => {
            let mut state = build_state.write().await;
            state.status = BuildStatus::Failed;
            state.error = Some(format!("Failed to wait for docker build: {e}"));
            state.logs.push(format!("ERROR: {e}"));
        }
    }
}

// ── Image readiness ────────────────────────────────────────────────

/// Wait until the runner image is available (built or pre-existing).
/// Returns `true` if ready, `false` if build failed or timed out.
pub async fn wait_for_image_ready(
    build_state: &Arc<RwLock<RunnerBuildState>>,
    timeout: std::time::Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        {
            let state = build_state.read().await;
            match state.status {
                BuildStatus::ImageReady | BuildStatus::Complete => return true,
                BuildStatus::Failed => return false,
                _ => {} // still checking/building
            }
        }
        if tokio::time::Instant::now() >= deadline {
            tracing::warn!("Timed out waiting for runner image");
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Check if the runner image is currently available (non-blocking).
pub async fn is_image_ready(build_state: &Arc<RwLock<RunnerBuildState>>) -> bool {
    let state = build_state.read().await;
    state.image_available
}

// ── API handlers ───────────────────────────────────────────────────

/// GET /api/docker/build-status — current build state
pub async fn get_build_status(State(state): State<AppState>) -> Json<RunnerBuildState> {
    Json(state.runner_build.read().await.clone())
}

/// POST /api/docker/build — trigger a rebuild
pub async fn trigger_build(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    {
        let current = state.runner_build.read().await;
        if current.status == BuildStatus::Building {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "Build already in progress" })),
            );
        }
    }

    // Reset state
    {
        let mut bs = state.runner_build.write().await;
        *bs = RunnerBuildState::default();
    }

    // Determine project root from the binary location or cwd
    let project_root = std::env::current_dir().unwrap_or_default();
    spawn_runner_build(state.runner_build.clone(), project_root);

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "status": "build_started" })),
    )
}

/// GET /api/docker/build/stream — SSE stream of build logs
pub async fn build_log_stream(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let build_state = state.runner_build.clone();
    let mut last_log_index = 0usize;

    let stream = async_stream::stream! {
        loop {
            let snapshot = build_state.read().await.clone();
            let new_logs = &snapshot.logs[last_log_index..];

            for log in new_logs {
                yield Ok(Event::default().event("log").data(log.clone()));
            }
            last_log_index = snapshot.logs.len();

            // Send progress update
            yield Ok(Event::default()
                .event("progress")
                .data(serde_json::json!({
                    "status": snapshot.status,
                    "progress_pct": snapshot.progress_pct,
                    "image_available": snapshot.image_available,
                    "error": snapshot.error,
                }).to_string()));

            // Stop streaming when build is done
            if matches!(snapshot.status, BuildStatus::Complete | BuildStatus::Failed | BuildStatus::ImageReady) {
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    };

    Sse::new(stream)
}

/// GET /api/docker/containers — list agent containers tracked in Postgres
pub async fn list_containers(
    State(state): State<AppState>,
) -> Result<Json<Vec<ContainerInfo>>, (StatusCode, Json<serde_json::Value>)> {
    // Source of truth: Postgres persistent_containers table
    let rows = state.pg_store.load_persistent_containers().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to load containers: {e}") })),
        )
    })?;

    if rows.is_empty() {
        return Ok(Json(Vec::new()));
    }

    // Enrich each record with live Docker status
    let docker = Docker::connect_with_local_defaults().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": format!("Docker not available: {e}") })),
        )
    })?;

    let mut result = Vec::new();
    for (container_id, task_id, agent_type, workspace_dir) in &rows {
        let (status, docker_state, image, created, mounts, env_vars_raw) =
            match docker.inspect_container(container_id, None).await {
                Ok(info) => {
                    let st = info.state.as_ref();
                    let status_str = st
                        .and_then(|s| s.status)
                        .map(|s| format!("{s:?}"))
                        .unwrap_or_else(|| "unknown".into());
                    let state_str = status_str.to_lowercase();
                    let img = info
                        .config
                        .as_ref()
                        .and_then(|c| c.image.clone())
                        .unwrap_or_default();
                    let created_str = info.created.as_deref().unwrap_or_default();
                    let created_ts = chrono::DateTime::parse_from_rfc3339(created_str)
                        .map(|dt| dt.timestamp())
                        .unwrap_or(0);
                    let mounts: Vec<MountInfo> = info
                        .mounts
                        .as_ref()
                        .map(|ms| {
                            ms.iter()
                                .map(|m| MountInfo {
                                    source: m.source.clone().unwrap_or_default(),
                                    destination: m.destination.clone().unwrap_or_default(),
                                    mode: m.mode.clone().unwrap_or_default(),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let env_raw: Vec<String> = info.config.as_ref()
                        .and_then(|c| c.env.clone())
                        .unwrap_or_default();
                    (status_str, state_str, img, created_ts, mounts, env_raw)
                }
                Err(_) => (
                    "not found".into(),
                    "removed".into(),
                    String::new(),
                    0i64,
                    Vec::new(),
                    Vec::new(),
                ),
            };

        // Extract env vars for mcp_tools and gateway_url detection
        let mcp_tools: Vec<String> = {
            let raw = env_vars_raw.iter()
                .find(|e| e.starts_with("MCCLAWD_ALLOWED_TOOLS="))
                .map(|e| e.trim_start_matches("MCCLAWD_ALLOWED_TOOLS=").trim().to_string())
                .unwrap_or_default();
            if raw == "*" {
                // Wildcard — resolve to actual MCP server names
                let config = state.config.read().await;
                let from_config: Vec<String> = config.mcp.servers.iter().map(|s| s.name.clone()).collect();
                if !from_config.is_empty() {
                    from_config
                } else {
                    // Config has no servers — read from agentgateway.yaml
                    let gw_config = std::env::current_dir()
                        .unwrap_or_default()
                        .join("config/agentgateway.yaml");
                    if gw_config.exists() {
                        std::fs::read_to_string(&gw_config)
                            .map(|content| {
                                content.lines()
                                    .filter_map(|l| {
                                        let trimmed = l.trim();
                                        if trimmed.starts_with("- name:") {
                                            Some(trimmed.trim_start_matches("- name:").trim().to_string())
                                        } else {
                                            None
                                        }
                                    })
                                    .filter(|n| !n.is_empty())
                                    .collect()
                            })
                            .unwrap_or_else(|_| vec!["filesystem".into(), "langextract".into(), "scrapling".into()])
                    } else {
                        // Hardcoded fallback for known MCP servers
                        vec!["filesystem".into(), "langextract".into(), "scrapling".into()]
                    }
                }
            } else if raw.is_empty() {
                Vec::new()
            } else {
                raw.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
            }
        };

        let gateway_url: Option<String> = env_vars_raw.iter()
            .find(|e| e.starts_with("MCCLAWD_GATEWAY_URL="))
            .map(|e| e.trim_start_matches("MCCLAWD_GATEWAY_URL=").to_string());

        // Parse skill names from MCCLAWD_SKILL_CONTEXT (format: "## Skill: name\n...")
        let skills: Vec<String> = {
            let ctx = env_vars_raw.iter()
                .find(|e| e.starts_with("MCCLAWD_SKILL_CONTEXT="))
                .map(|e| e.trim_start_matches("MCCLAWD_SKILL_CONTEXT="))
                .unwrap_or("");
            if ctx.is_empty() {
                // Fallback: check installed skills on disk
                let config = state.config.read().await;
                let skills_dir = &config.skills.managed_dir;
                if skills_dir.exists() {
                    std::fs::read_dir(skills_dir)
                        .map(|entries| entries.filter_map(|e| e.ok())
                            .filter(|e| e.path().join("SKILL.md").exists())
                            .map(|e| e.file_name().to_string_lossy().to_string())
                            .collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            } else {
                // Parse "## Skill: name" lines from context
                ctx.lines()
                    .filter(|l| l.starts_with("## Skill:") || l.starts_with("# Skill:"))
                    .map(|l| l.trim_start_matches('#').trim().trim_start_matches("Skill:").trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            }
        };

        // Build masked env_vars map
        let env_vars: HashMap<String, String> = env_vars_raw.iter()
            .filter_map(|e| {
                let parts: Vec<&str> = e.splitn(2, '=').collect();
                if parts.len() == 2 {
                    let key = parts[0];
                    let value = if key.contains("KEY") || key.contains("SECRET") || key.contains("TOKEN") || key.contains("PASSWORD") {
                        "***masked***".to_string()
                    } else {
                        parts[1].to_string()
                    };
                    Some((key.to_string(), value))
                } else {
                    None
                }
            })
            .collect();

        // Read attachments from task directory
        let attachments = if task_id != "system-agent" {
            let config = state.config.read().await;
            let att_dir = config.data_dir.join("tasks").join(task_id).join("attachments");
            if att_dir.is_dir() {
                let mut atts = Vec::new();
                if let Ok(mut entries) = tokio::fs::read_dir(&att_dir).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        if let Ok(meta) = entry.metadata().await {
                            if meta.is_file() {
                                let name = entry.file_name().to_string_lossy().to_string();
                                let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
                                let is_image = matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg");
                                atts.push(ContainerAttachmentMeta { name, size: meta.len(), is_image });
                            }
                        }
                    }
                }
                atts
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let name = format!("mcclawd-{agent_type}-{}", &task_id[..std::cmp::min(task_id.len(), 8)]);

        // Skip containers that no longer exist in Docker — clean up stale DB rows
        if docker_state == "removed" {
            let _ = state.pg_store.delete_persistent_container(container_id).await;
            continue;
        }

        result.push(ContainerInfo {
            id: container_id.clone(),
            name,
            task_id: Some(task_id.clone()),
            status,
            state: docker_state,
            image,
            created,
            ports: Vec::new(),
            mounts,
            env_vars,
            labels: {
                let mut l = HashMap::new();
                l.insert("agent_type".into(), agent_type.clone());
                l.insert("workspace".into(), workspace_dir.clone());
                l
            },
            attachments,
            skills,
            mcp_tools,
            gateway_url,
        });
    }

    Ok(Json(result))
}

/// GET /api/docker/containers/{id} — get detailed container info
pub async fn get_container(
    State(_state): State<AppState>,
    axum::extract::Path(container_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let docker = Docker::connect_with_local_defaults().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": format!("Docker not available: {e}") })),
        )
    })?;

    let info = docker
        .inspect_container(&container_id, None)
        .await
        .map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("Container not found: {e}") })),
            )
        })?;

    // Extract useful metadata
    let config = info.config.as_ref();
    let env: Vec<String> = config.and_then(|c| c.env.clone()).unwrap_or_default();

    // Parse env into map, filtering out secrets
    let env_map: HashMap<String, String> = env
        .iter()
        .filter_map(|e| {
            let parts: Vec<&str> = e.splitn(2, '=').collect();
            if parts.len() == 2 {
                let key = parts[0];
                // Mask sensitive values
                let value = if key.contains("KEY")
                    || key.contains("SECRET")
                    || key.contains("TOKEN")
                    || key.contains("PASSWORD")
                {
                    "***masked***".to_string()
                } else {
                    parts[1].to_string()
                };
                Some((key.to_string(), value))
            } else {
                None
            }
        })
        .collect();

    let state_info = info.state.as_ref();

    Ok(Json(serde_json::json!({
        "id": info.id,
        "name": info.name.as_ref().map(|n| n.trim_start_matches('/')),
        "image": config.and_then(|c| c.image.as_ref()),
        "status": state_info.and_then(|s| s.status.as_ref()),
        "running": state_info.and_then(|s| s.running),
        "started_at": state_info.and_then(|s| s.started_at.as_ref()),
        "finished_at": state_info.and_then(|s| s.finished_at.as_ref()),
        "exit_code": state_info.and_then(|s| s.exit_code),
        "env": env_map,
        "mounts": info.mounts.as_ref().map(|ms| ms.iter().map(|m| serde_json::json!({
            "source": m.source,
            "destination": m.destination,
            "mode": m.mode,
            "rw": m.rw,
        })).collect::<Vec<_>>()),
        "network": info.network_settings.as_ref().and_then(|ns| {
            ns.networks.as_ref().map(|nets| nets.keys().collect::<Vec<_>>())
        }),
        "labels": config.and_then(|c| c.labels.as_ref()),
    })))
}

/// DELETE /api/docker/containers/{id} — stop + remove a container and its associated task.
pub async fn delete_container(
    State(state): State<AppState>,
    axum::extract::Path(container_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let docker = Docker::connect_with_local_defaults().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": format!("Docker not available: {e}") })),
        )
    })?;

    // Find associated task_id from container labels before removing.
    // Fall back to persistent_containers DB table if container is already gone.
    let task_id = docker
        .inspect_container(&container_id, None)
        .await
        .ok()
        .and_then(|info| {
            info.config
                .as_ref()
                .and_then(|c| c.labels.as_ref())
                .and_then(|l| l.get("mcclawd.task_id").cloned())
        })
        .or_else(|| {
            // Container might already be removed — look up task_id from DB
            None // filled async below
        });
    let task_id = match task_id {
        Some(tid) => Some(tid),
        None => {
            // Fallback: look up task_id from persistent_containers DB by container_id
            state
                .pg_store
                .load_persistent_containers()
                .await
                .ok()
                .and_then(|rows| {
                    rows.into_iter()
                        .find(|(cid, _, _, _)| cid == &container_id)
                        .map(|(_, tid, _, _)| tid)
                })
        }
    };

    // Stop the container (ignore errors if already stopped)
    let _ = docker
        .stop_container(&container_id, Some(bollard::container::StopContainerOptions { t: 5 }))
        .await;

    // Remove the container
    let _ = docker
        .remove_container(
            &container_id,
            Some(bollard::container::RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;

    // Clean up associated task if found
    if let Some(tid) = &task_id {
        let task_id_typed = mcclawd_core::types::TaskId(tid.clone());

        // Remove from in-memory task_containers map
        state.task_containers.write().await.remove(&task_id_typed);

        // Remove task from in-memory task manager (so it disappears from UI)
        {
            let mut mgr = state.tasks.write().await;
            if let Some(t) = mgr.get_task(&task_id_typed) {
                if matches!(
                    t.status,
                    mcclawd_tasks::manager::TaskStatus::Running
                        | mcclawd_tasks::manager::TaskStatus::Building
                ) {
                    mgr.fail_task(&task_id_typed, "Container removed".to_string());
                }
            }
            mgr.delete_task(&task_id_typed);
        }

        // Clean up broadcast channel + chat history + event cache
        state.task_streams.write().await.remove(&task_id_typed);
        state.task_chat_history.write().await.remove(&task_id_typed);
        state.task_events.write().await.remove(&task_id_typed);

        // Update PG task status to Failed before deleting (ensures no stale Running rows)
        state
            .pg_update_status(&task_id_typed, "Failed", Some("Container removed"))
            .await;

        // Cascade-delete from postgres atomically (task + security_events + dlp_findings + persistent_containers)
        state.pg_delete_task_sync(&task_id_typed).await;
    } else {
        // No task_id label found — still clean up the persistent_container record by container_id
        let _ = state
            .pg_store
            .delete_persistent_container(&container_id)
            .await;
    }

    Ok(Json(serde_json::json!({
        "deleted": true,
        "container_id": container_id,
        "task_id": task_id,
    })))
}
