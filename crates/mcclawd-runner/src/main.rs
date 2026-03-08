//! agent-runner — standalone binary that runs inside a Docker container.
//!
//! Reads configuration from environment variables and files mounted at /run/,
//! builds a Rig agent, streams the response, and emits OutboundChunk JSONL
//! on stdout. Tracing goes to stderr as structured JSON.
//!
//! Supports two modes:
//! - **Single-shot** (default): reads prompt from env/file, processes once, exits.
//! - **Server mode** (`--server`): long-lived process that reads JSON messages from
//!   stdin and processes each one, reusing the same agent instance.

mod protocol;

use anyhow::{bail, Context, Result};
use futures::StreamExt;
use mcclawd_agent::context::ContextBuilder;
use mcclawd_agent::engine::AgentEngine;
use mcclawd_agent::workspace::Workspace;
use mcclawd_channels::OutboundChunk;
use mcclawd_core::config::McclawdConfig;
use rig::agent::MultiTurnStreamItem;
use base64::Engine;
use rig::OneOrMany;
use rig::completion::message::Message as RigMessage;
use rig::completion::message::{DocumentSourceKind, Image, ImageMediaType, MimeType, ToolResultContent, UserContent};
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingChat};
use std::path::{Path, PathBuf};
use tokio::io::AsyncBufReadExt;

/// Inbound message for server mode (one JSON object per line on stdin).
#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum ServerMessage {
    /// Process a chat prompt, optionally with conversation history.
    #[serde(rename = "chat")]
    Chat {
        prompt: String,
        #[serde(default)]
        history_json: Option<String>,
    },
    /// Gracefully shut down the server.
    #[serde(rename = "shutdown")]
    Shutdown,
}

/// Configuration read from environment variables and mounted files.
struct RunnerConfig {
    prompt: String,
    api_key: String,
    task_id: Option<String>,
    agent_type: String,
    max_turns: usize,
    history: Vec<RigMessage>,
}

impl RunnerConfig {
    /// Read configuration from env vars and /run/ filesystem.
    fn from_env() -> Result<Self> {
        // API key from mounted secret file (never from env var)
        let api_key = std::fs::read_to_string("/run/secrets/ANTHROPIC_API_KEY")
            .context("Failed to read /run/secrets/ANTHROPIC_API_KEY")?
            .trim()
            .to_string();

        if api_key.is_empty() {
            bail!("ANTHROPIC_API_KEY file is empty");
        }

        // Prompt: env var first, then mounted file (for large prompts >32KB)
        let prompt = match std::env::var("MCCLAWD_PROMPT") {
            Ok(p) if !p.is_empty() => p,
            _ => std::fs::read_to_string("/run/prompt.txt")
                .context("No MCCLAWD_PROMPT env var and /run/prompt.txt not found")?,
        };

        if prompt.trim().is_empty() {
            bail!("Prompt is empty");
        }

        // Optional conversation history for multi-turn follow-ups
        let history = match std::fs::read_to_string("/run/history.json") {
            Ok(data) => serde_json::from_str::<Vec<RigMessage>>(&data)
                .context("Failed to parse /run/history.json")?,
            Err(_) => vec![],
        };

        let task_id = std::env::var("MCCLAWD_TASK_ID").ok();
        let agent_type = std::env::var("MCCLAWD_AGENT_TYPE").unwrap_or_else(|_| "task".into());
        let max_turns: usize = std::env::var("MCCLAWD_MAX_TURNS")
            .unwrap_or_else(|_| "25".into())
            .parse()
            .unwrap_or(25);

        Ok(Self {
            prompt,
            api_key,
            task_id,
            agent_type,
            max_turns,
            history,
        })
    }
}

/// Check /attachments for mounted files and augment the prompt.
/// Returns (augmented_prompt, image_parts) mirroring the host-side logic in tasks.rs.
fn augment_prompt_with_attachments(prompt: &str) -> (String, Vec<UserContent>) {
    let att_dir = Path::new("/attachments");
    let mut text_augmented = prompt.to_string();
    let mut image_parts: Vec<UserContent> = Vec::new();

    if !att_dir.is_dir() {
        return (text_augmented, image_parts);
    }

    let entries: Vec<_> = match std::fs::read_dir(att_dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return (text_augmented, image_parts),
    };

    if entries.is_empty() {
        return (text_augmented, image_parts);
    }

    text_augmented.push_str("\n\n## Attached Files\n\n");

    for entry in &entries {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let mime = mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string();

        if let Some(image_media_type) = ImageMediaType::from_mime_type(&mime) {
            // Image files: base64-encode, add as multimodal content
            match std::fs::read(&path) {
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
                        name,
                        mime,
                        bytes.len() / 1024
                    ));
                }
                Err(_) => {
                    text_augmented
                        .push_str(&format!("### File: {} (could not read)\n\n", name));
                }
            }
        } else if mime.starts_with("text/")
            || mime.contains("json")
            || mime.contains("xml")
            || mime.contains("markdown")
            || mime.contains("yaml")
            || mime.contains("toml")
            || mime.contains("csv")
            || mime.contains("javascript")
            || mime.contains("typescript")
        {
            // Text files: read and include as code block (max 50KB)
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let truncated = if content.len() > 50_000 {
                        &content[..50_000]
                    } else {
                        &content
                    };
                    let safe_content = truncated.replace("```", "` ` `");
                    text_augmented.push_str(&format!(
                        "### File: {}\n\n```\n{}\n```\n\n",
                        name, safe_content
                    ));
                }
                Err(_) => {
                    text_augmented
                        .push_str(&format!("### File: {} (could not read)\n\n", name));
                }
            }
        } else {
            // Binary files: metadata only
            let size_str = match std::fs::metadata(&path) {
                Ok(m) => format!("{}KB", m.len() / 1024),
                Err(_) => "unknown size".to_string(),
            };
            text_augmented.push_str(&format!(
                "### File: {} ({}, {} — binary file, content not included)\n\n",
                name, mime, size_str
            ));
        }
    }

    (text_augmented, image_parts)
}

/// Load workspace files from /workspace directory.
fn load_workspace() -> Workspace {
    let path = PathBuf::from("/workspace");
    Workspace {
        name: "default".to_string(),
        soul: std::fs::read_to_string(path.join("SOUL.md")).ok(),
        agents: std::fs::read_to_string(path.join("AGENTS.md")).ok(),
        user: std::fs::read_to_string(path.join("USER.md")).ok(),
        identity: std::fs::read_to_string(path.join("IDENTITY.md")).ok(),
        tools: std::fs::read_to_string(path.join("TOOLS.md")).ok(),
        heartbeat: std::fs::read_to_string(path.join("HEARTBEAT.md")).ok(),
        path,
    }
}

#[tokio::main]
async fn main() {
    // JSON tracing to stderr (stdout is reserved for JSONL protocol)
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let server_mode = std::env::args().any(|a| a == "--server");

    let result = if server_mode {
        run_server().await
    } else {
        run().await
    };

    if let Err(e) = result {
        let msg = format!("{e:#}");
        tracing::error!(error = %msg, "Runner failed");
        protocol::emit(&OutboundChunk::Error(msg));
        protocol::emit(&OutboundChunk::Done);
        std::process::exit(1);
    }
}

/// Server mode: build agent once, then read JSON messages from stdin in a loop.
/// Each `chat` message triggers a streaming response; `shutdown` exits cleanly.
async fn run_server() -> Result<()> {
    // Wait for secrets to be written by host (docker exec after container start)
    let api_key = {
        let mut attempts = 0;
        loop {
            match std::fs::read_to_string("/run/secrets/ANTHROPIC_API_KEY") {
                Ok(val) if !val.trim().is_empty() => break val.trim().to_string(),
                _ if attempts < 30 => {
                    attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                _ => bail!("Timed out waiting for /run/secrets/ANTHROPIC_API_KEY (15s)"),
            }
        }
    };

    let agent_type = std::env::var("MCCLAWD_AGENT_TYPE").unwrap_or_else(|_| "task".into());
    let max_turns: usize = std::env::var("MCCLAWD_MAX_TURNS")
        .unwrap_or_else(|_| "25".into())
        .parse()
        .unwrap_or(25);

    let workspace = load_workspace();
    let config = McclawdConfig::default();

    // Build agent once — reused across all messages.
    // Use MCCLAWD_SKILL_CONTEXT env var if set (selective skill mounting from host),
    // otherwise fall back to loading skills from disk (which loads ALL skills).
    let skill_context_override = std::env::var("MCCLAWD_SKILL_CONTEXT").ok().filter(|s| !s.is_empty());
    let (agent, _memory_store, _mcp_bundles) = if agent_type == "system" {
        let mut context =
            ContextBuilder::new(workspace).with_skills_dir(config.skills.managed_dir.clone());
        if let Some(ref ctx) = skill_context_override {
            context = context.with_skill_context_override(ctx.clone());
        }
        let system_prompt = context.build_system_prompt();
        let agent = AgentEngine::build_system_agent(&api_key, &system_prompt).await?;
        (agent, None, vec![])
    } else {
        // For task agents: if MCCLAWD_SKILL_CONTEXT is set, use it as override;
        // if not set, load no skills (empty filter) to avoid injecting all skills.
        let skill_filter: Option<Vec<String>> = if skill_context_override.is_none() {
            Some(vec![]) // No env var = no skills (safe default for containers)
        } else {
            None // Will use skill_context_override instead
        };
        let (agent, mem, bundles) =
            AgentEngine::build_with_skill_filter(workspace, &api_key, max_turns, &config, None, skill_filter).await?;
        (agent, Some(mem), bundles)
    };

    // Log readiness to stderr (tracing) only — NOT via protocol::emit.
    // Emitting TextDelta + Done on startup would be forwarded as a spurious task
    // response, causing the frontend to show "complete" before the real reply arrives.
    tracing::info!(agent_type = %agent_type, "Server mode ready, waiting for messages on stdin");

    // Read JSON messages from stdin, one per line
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let msg: ServerMessage = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                protocol::emit(&OutboundChunk::Error(format!("Invalid message: {e}")));
                protocol::emit(&OutboundChunk::Done);
                continue;
            }
        };

        match msg {
            ServerMessage::Chat {
                prompt,
                history_json,
            } => {
                let history: Vec<RigMessage> = history_json
                    .and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or_default();

                tracing::info!(
                    history_len = history.len(),
                    prompt_len = prompt.len(),
                    "Processing chat message"
                );

                // Augment prompt with attachments (same as single-shot mode)
                let (augmented_prompt, image_parts) = augment_prompt_with_attachments(&prompt);

                // Build prompt message
                let prompt_message: RigMessage = if image_parts.is_empty() {
                    RigMessage::user(&augmented_prompt)
                } else {
                    let mut parts = vec![UserContent::text(&augmented_prompt)];
                    parts.extend(image_parts);
                    RigMessage::User {
                        content: OneOrMany::many(parts).expect("non-empty image parts"),
                    }
                };

                // Stream response
                let mut stream = agent.stream_chat(prompt_message, history).await;
                let mut accumulated_text = String::new();
                let mut last_tool_name = String::new();

                while let Some(item) = stream.next().await {
                    match item {
                        Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                            StreamedAssistantContent::Text(text) => {
                                protocol::emit(&OutboundChunk::TextDelta(text.text.clone()));
                                accumulated_text.push_str(&text.text);
                            }
                            StreamedAssistantContent::ToolCall { tool_call, .. } => {
                                last_tool_name = tool_call.function.name.clone();
                                protocol::emit(&OutboundChunk::ToolStart {
                                    name: tool_call.function.name.clone(),
                                });
                            }
                            _ => {}
                        },
                        Ok(MultiTurnStreamItem::StreamUserItem(
                            StreamedUserContent::ToolResult { tool_result, .. },
                        )) => {
                            let summary = match tool_result.content.first() {
                                ToolResultContent::Text(t) => t.text.clone(),
                                _ => String::new(),
                            };
                            let tool_name = if last_tool_name.is_empty() {
                                "unknown".to_string()
                            } else {
                                last_tool_name.clone()
                            };
                            protocol::emit(&OutboundChunk::ToolEnd {
                                name: tool_name,
                                summary: Some(summary),
                            });
                        }
                        Ok(MultiTurnStreamItem::FinalResponse(final_resp)) => {
                            if !accumulated_text.is_empty() {
                                protocol::emit(&OutboundChunk::TextBlock(
                                    accumulated_text.clone(),
                                ));
                            }

                            let usage = final_resp.usage();
                            protocol::emit(&OutboundChunk::Usage {
                                input_tokens: usage.input_tokens,
                                output_tokens: usage.output_tokens,
                                total_tokens: usage.total_tokens,
                                model: None,
                            });

                            if let Some(history) = final_resp.history() {
                                if let Ok(json) = serde_json::to_string(history) {
                                    protocol::emit(&OutboundChunk::ChatHistory(json));
                                }
                            }

                            protocol::emit(&OutboundChunk::Done);
                        }
                        Err(e) => {
                            let msg = format!("Streaming error: {e}");
                            tracing::error!(error = %msg, "Stream failed");
                            protocol::emit(&OutboundChunk::Error(msg));
                            protocol::emit(&OutboundChunk::Done);
                            // Don't exit in server mode — wait for next message
                            break;
                        }
                        _ => {}
                    }
                }
            }
            ServerMessage::Shutdown => {
                tracing::info!("Shutdown requested");
                protocol::emit(&OutboundChunk::Done);
                break;
            }
        }
    }

    tracing::info!("Server mode exiting");
    Ok(())
}

async fn run() -> Result<()> {
    let cfg = RunnerConfig::from_env()?;

    tracing::info!(
        task_id = cfg.task_id.as_deref().unwrap_or("none"),
        agent_type = %cfg.agent_type,
        max_turns = cfg.max_turns,
        history_len = cfg.history.len(),
        "Starting agent-runner"
    );

    let workspace = load_workspace();
    let config = McclawdConfig::default();

    // Build the agent based on type.
    // Use MCCLAWD_SKILL_CONTEXT env var if set (selective skill mounting from host).
    let skill_context_override = std::env::var("MCCLAWD_SKILL_CONTEXT").ok().filter(|s| !s.is_empty());
    let (agent, _memory_store, _mcp_bundles) = if cfg.agent_type == "system" {
        let mut context = ContextBuilder::new(workspace)
            .with_skills_dir(config.skills.managed_dir.clone());
        if let Some(ref ctx) = skill_context_override {
            context = context.with_skill_context_override(ctx.clone());
        }
        let system_prompt = context.build_system_prompt();
        let agent = AgentEngine::build_system_agent(&cfg.api_key, &system_prompt).await?;
        (agent, None, vec![])
    } else {
        let skill_filter: Option<Vec<String>> = if skill_context_override.is_none() {
            Some(vec![]) // No env var = no skills (safe default for containers)
        } else {
            None
        };
        let (agent, mem, bundles) =
            AgentEngine::build_with_skill_filter(workspace, &cfg.api_key, cfg.max_turns, &config, None, skill_filter).await?;
        (agent, Some(mem), bundles)
    };

    // Augment prompt with any mounted attachments
    let (augmented_prompt, image_parts) = augment_prompt_with_attachments(&cfg.prompt);

    // Build prompt message — multimodal if images attached, plain text otherwise
    let prompt_message: RigMessage = if image_parts.is_empty() {
        RigMessage::user(&augmented_prompt)
    } else {
        let mut parts = vec![UserContent::text(&augmented_prompt)];
        parts.extend(image_parts);
        RigMessage::User {
            content: OneOrMany::many(parts).expect("non-empty image parts"),
        }
    };
    let mut stream = agent.stream_chat(prompt_message, cfg.history).await;
    let mut accumulated_text = String::new();
    let mut last_tool_name = String::new();

    while let Some(item) = stream.next().await {
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                StreamedAssistantContent::Text(text) => {
                    protocol::emit(&OutboundChunk::TextDelta(text.text.clone()));
                    accumulated_text.push_str(&text.text);
                }
                StreamedAssistantContent::ToolCall { tool_call, .. } => {
                    last_tool_name = tool_call.function.name.clone();
                    protocol::emit(&OutboundChunk::ToolStart {
                        name: tool_call.function.name.clone(),
                    });
                }
                _ => {} // Reasoning, ToolCallDelta, Final, non_exhaustive
            },
            Ok(MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult {
                tool_result,
                ..
            })) => {
                // Emit ToolEnd with summary from tool result
                let summary = match tool_result.content.first() {
                    ToolResultContent::Text(t) => t.text.clone(),
                    _ => String::new(),
                };
                let tool_name = if last_tool_name.is_empty() {
                    "unknown".to_string()
                } else {
                    last_tool_name.clone()
                };
                protocol::emit(&OutboundChunk::ToolEnd {
                    name: tool_name,
                    summary: Some(summary),
                });
            }
            Ok(MultiTurnStreamItem::FinalResponse(final_resp)) => {
                // Emit complete text block
                if !accumulated_text.is_empty() {
                    protocol::emit(&OutboundChunk::TextBlock(accumulated_text.clone()));
                }

                // Emit token usage for host-side cost tracking
                let usage = final_resp.usage();
                protocol::emit(&OutboundChunk::Usage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                    model: None, // host knows the model from config
                });

                // Emit conversation history for multi-turn follow-ups
                if let Some(history) = final_resp.history() {
                    if let Ok(json) = serde_json::to_string(history) {
                        protocol::emit(&OutboundChunk::ChatHistory(json));
                    }
                }

                protocol::emit(&OutboundChunk::Done);
            }
            Err(e) => {
                let msg = format!("Streaming error: {e}");
                tracing::error!(error = %msg, "Stream failed");
                protocol::emit(&OutboundChunk::Error(msg));
                protocol::emit(&OutboundChunk::Done);
                std::process::exit(1);
            }
            _ => {} // non_exhaustive guard
        }
    }

    Ok(())
}
