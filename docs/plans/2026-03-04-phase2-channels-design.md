# Phase 2: Multi-Channel Architecture Design

**Date:** 2026-03-04
**Status:** Draft
**Scope:** Telegram adapter, channel registry, message normalization, session management, media handling, channel-specific auth
**Depends on:** Phase 0 (complete), Phase 1 (sandbox, skills, daemon, web channel)

---

## 1. Executive Summary

Phase 2 transforms McClawd from a single-channel agent (CLI + Web) into a multi-channel platform. Users interact with the same agent through Telegram, Discord, Slack, or any future channel -- all normalized to a unified message pipeline. The existing `Channel` trait already anticipates this: it hides five transport patterns behind `recv()`/`send_chunk()`. Phase 2 makes that promise real.

**Key deliverables:**
1. Evolved `Channel` trait with lifecycle, state persistence, and media support
2. Telegram adapter (teloxide) as the reference multi-channel implementation
3. `ChannelRegistry` for dynamic registration, routing, and health monitoring
4. Normalized `Envelope` message format that carries metadata across all channels
5. Per-user, per-channel session management with cross-channel context sharing
6. Media pipeline (upload/download/transcode) abstracted behind `MediaStore`
7. Channel-specific authentication mapped to McClawd identity

---

## 2. Architecture Overview

```
                    ┌──────────────────────────────────────────┐
                    │         Channel Registry                  │
                    │  register / unregister / health check     │
                    ├──────────────────────────────────────────┤
                    │                                          │
                    │  CliChannel   WebChannel   TelegramBot   │
                    │  (Phase 0)    (Phase 1)    (Phase 2)     │
                    │                                          │
                    │  DiscordGw    SlackEvents  [future...]   │
                    │  (Phase 3)    (Phase 3)                  │
                    │                                          │
                    └──────────────┬───────────────────────────┘
                                   │ Envelope (normalized)
                                   ▼
┌──────────────────────────────────────────────────────────────────┐
│  Inbound Pipeline                                                │
│  normalize → dedup → auth_map → access → route → debounce →     │
│  dispatch                                                        │
└──────────────────────┬───────────────────────────────────────────┘
                       │
            ┌──────────┴───────────┐
            ▼                      ▼
   ┌─────────────────┐   ┌──────────────────┐
   │  Task Manager    │   │  Session Manager  │
   │  (interactive +  │   │  per-(agent,      │
   │   background)    │   │   channel, peer)  │
   └────────┬─────────┘   └──────────────────┘
            │
            ▼
   ┌─────────────────┐
   │  Agent Engine    │
   │  (Rig-powered)   │
   └────────┬─────────┘
            │ OutboundChunk
            ▼
   ┌──────────────────────────────────────────┐
   │  Outbound Router                          │
   │  SessionKey → ChannelRegistry → adapter   │
   │  format → send_chunk()                    │
   └──────────────────────────────────────────┘
```

---

## 3. Channel Trait Evolution

### 3a. Current Trait (Phase 0/1)

```rust
#[async_trait]
pub trait Channel: Send + Sync + 'static {
    fn kind(&self) -> ChannelKind;
    async fn start(&self, inbound_tx: mpsc::Sender<InboundMessage>,
                   shutdown: CancellationToken) -> Result<()>;
    async fn send_chunk(&self, chunk: OutboundChunk) -> Result<()>;
}
```

### 3b. Evolved Trait (Phase 2)

The trait gains lifecycle management, state persistence, media capabilities, and health reporting. All additions are backward-compatible via default implementations.

```rust
#[async_trait]
pub trait Channel: Send + Sync + 'static {
    // === Identity ===
    fn kind(&self) -> ChannelKind;

    /// Human-readable name for logging and UI display
    fn display_name(&self) -> &str { self.kind().as_str() }

    // === Lifecycle ===

    /// Start receiving messages. Adapter spawns its own tasks internally.
    /// The adapter MUST normalize all platform-specific messages into Envelope
    /// before sending on inbound_tx.
    async fn start(
        &self,
        inbound_tx: mpsc::Sender<Envelope>,
        shutdown: CancellationToken,
    ) -> Result<()>;

    /// Graceful shutdown. Called before process exit or channel removal.
    /// Default: no-op (channels that need cleanup override this).
    async fn stop(&self) -> Result<()> { Ok(()) }

    // === Outbound ===

    /// Send a chunk to a specific peer on this channel.
    /// The session_key identifies the target conversation.
    async fn send_chunk(
        &self,
        session_key: &SessionKey,
        chunk: OutboundChunk,
    ) -> Result<()>;

    // === State Persistence (Principle 3) ===

    /// Serialize channel connection state for daemon restart survival.
    /// Returns None if channel is stateless (e.g., Telegram bot API).
    async fn save_state(&self) -> Result<Option<Vec<u8>>> { Ok(None) }

    /// Restore channel state from a previous save.
    async fn restore_state(&self, _state: &[u8]) -> Result<()> { Ok(()) }

    // === Capabilities ===

    /// Declare what this channel supports. Used by outbound router
    /// to decide formatting (e.g., skip markdown on SMS).
    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities::default()
    }

    // === Health ===

    /// Health check. Registry polls this periodically.
    async fn health(&self) -> ChannelHealth {
        ChannelHealth::Healthy
    }
}
```

### 3c. Capability Declaration

```rust
#[derive(Debug, Clone)]
pub struct ChannelCapabilities {
    /// Max message length (0 = unlimited)
    pub max_message_length: usize,
    /// Supports markdown formatting
    pub markdown: bool,
    /// Supports inline code blocks
    pub code_blocks: bool,
    /// Supports file attachments
    pub file_upload: bool,
    /// Max file size in bytes (0 = no limit)
    pub max_file_size: u64,
    /// Supported MIME types for media (empty = all)
    pub supported_media_types: Vec<String>,
    /// Supports message editing (for streaming updates)
    pub message_edit: bool,
    /// Supports reply threading
    pub threading: bool,
    /// Supports reactions/emoji
    pub reactions: bool,
    /// Streaming mode: how to deliver incremental output
    pub streaming_mode: StreamingMode,
}

#[derive(Debug, Clone, Default)]
pub enum StreamingMode {
    /// Send each delta as a new message (SMS, Email)
    #[default]
    Batch,
    /// Edit a single message with accumulated content (Telegram, Discord)
    EditInPlace,
    /// True streaming via persistent connection (WebSocket)
    Stream,
}

impl Default for ChannelCapabilities {
    fn default() -> Self {
        Self {
            max_message_length: 0,
            markdown: true,
            code_blocks: true,
            file_upload: false,
            max_file_size: 0,
            supported_media_types: vec![],
            message_edit: false,
            threading: false,
            reactions: false,
            streaming_mode: StreamingMode::Batch,
        }
    }
}
```

---

## 4. Message Normalization: The Envelope

### 4a. Design Rationale

The current `InboundMessage` is too simple for multi-channel. We need:
- Platform-specific metadata (Telegram chat_id, Discord guild_id) without leaking into the pipeline
- Media attachments as first-class citizens
- Reply/thread context for channels that support it
- Origin tracking for outbound routing

The `Envelope` replaces `InboundMessage` as the normalized message format flowing through the pipeline.

### 4b. Envelope Type

```rust
/// Normalized message flowing through the inbound pipeline.
/// Every channel adapter produces these; the pipeline never sees
/// platform-specific types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Globally unique message ID (UUID v7 for time-ordering)
    pub id: MessageId,

    /// Which channel produced this message
    pub channel: ChannelKind,

    /// Who sent it (normalized peer identity)
    pub peer: Peer,

    /// Conversation context (reply-to, thread)
    pub thread: Option<ThreadContext>,

    /// The actual content
    pub content: MessageContent,

    /// Attached media (images, files, voice notes)
    pub attachments: Vec<Attachment>,

    /// Platform-specific metadata (opaque to pipeline)
    /// Stored for outbound routing (e.g., Telegram chat_id needed for reply)
    pub platform_meta: PlatformMeta,

    /// When the message was created on the platform
    pub platform_timestamp: DateTime<Utc>,

    /// When McClawd received it
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    /// Channel-scoped ID (Telegram user_id, Discord user_id, etc.)
    pub platform_id: String,

    /// McClawd user ID (set by auth_map pipeline stage, None if unmapped)
    pub mcclawd_user_id: Option<UserId>,

    /// Display name for logging/UI
    pub display_name: Option<String>,

    /// Avatar URL if available
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadContext {
    /// Platform-specific thread/conversation ID
    pub thread_id: String,

    /// ID of the message being replied to (if any)
    pub reply_to: Option<MessageId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    /// Plain text or markdown
    Text(String),

    /// Bot command (e.g., /start, /ask)
    Command {
        name: String,
        args: String,
    },

    /// User canceled the current operation
    Cancel,

    /// Voice message (transcribed by channel adapter or sent to STT)
    Voice {
        transcription: Option<String>,
        media_ref: MediaRef,
    },

    /// Location share
    Location {
        latitude: f64,
        longitude: f64,
    },
}

/// Opaque platform metadata preserved for outbound routing.
/// The pipeline doesn't interpret this — it's round-tripped back
/// to the channel adapter on outbound.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlatformMeta {
    Telegram {
        chat_id: i64,
        message_id: i64,
        chat_type: String,  // "private", "group", "supergroup"
    },
    Discord {
        guild_id: Option<u64>,
        channel_id: u64,
        message_id: u64,
    },
    Slack {
        team_id: String,
        channel_id: String,
        ts: String,         // Slack message timestamp (unique ID)
    },
    Web {
        session_id: String,
    },
    Cli,
    Custom(serde_json::Value),
}
```

### 4c. OutboundChunk Evolution

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutboundChunk {
    /// Incremental text from LLM (for streaming channels)
    TextDelta(String),

    /// Complete text block (for batch channels, or final message)
    TextBlock(String),

    /// Tool execution started
    ToolStart { name: String },

    /// Tool execution completed
    ToolEnd { name: String, summary: Option<String> },

    /// Media attachment (agent generated a file/image)
    Media(Attachment),

    /// Typing indicator (channels that support it)
    Typing,

    /// Agent finished
    Done,

    /// Error message to display to user
    Error(String),
}
```

---

## 5. Channel Registry

### 5a. Purpose

The `ChannelRegistry` is the central manager for all active channels. It handles:
- Dynamic registration/removal of channels at runtime
- Routing outbound messages to the correct channel
- Health monitoring and automatic reconnection
- Configuration-driven channel initialization

### 5b. Type Sketch

```rust
pub struct ChannelRegistry {
    /// Active channels indexed by kind
    channels: RwLock<HashMap<ChannelKind, ChannelEntry>>,

    /// Inbound message sender (shared across all channels)
    inbound_tx: mpsc::Sender<Envelope>,

    /// Global shutdown signal
    shutdown: CancellationToken,

    /// State persistence backend
    state_store: Arc<dyn ChannelStateStore>,
}

struct ChannelEntry {
    channel: Arc<dyn Channel>,
    status: ChannelStatus,
    started_at: DateTime<Utc>,
    last_health_check: Option<DateTime<Utc>>,
    message_count: AtomicU64,
}

#[derive(Debug, Clone)]
pub enum ChannelStatus {
    Starting,
    Running,
    Degraded(String),   // partial failure, still accepting messages
    Reconnecting,
    Stopped,
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum ChannelHealth {
    Healthy,
    Degraded(String),
    Unhealthy(String),
}

/// Persists channel connection state across daemon restarts
#[async_trait]
pub trait ChannelStateStore: Send + Sync {
    async fn save(&self, kind: &ChannelKind, state: &[u8]) -> Result<()>;
    async fn load(&self, kind: &ChannelKind) -> Result<Option<Vec<u8>>>;
    async fn delete(&self, kind: &ChannelKind) -> Result<()>;
}

impl ChannelRegistry {
    /// Register and start a new channel
    pub async fn register(&self, channel: Arc<dyn Channel>) -> Result<()>;

    /// Unregister and stop a channel
    pub async fn unregister(&self, kind: &ChannelKind) -> Result<()>;

    /// Route an outbound chunk to the correct channel
    pub async fn send(
        &self,
        session_key: &SessionKey,
        chunk: OutboundChunk,
    ) -> Result<()>;

    /// Get capabilities for a channel (used by outbound formatter)
    pub fn capabilities(&self, kind: &ChannelKind) -> Option<ChannelCapabilities>;

    /// Health check all channels
    pub async fn health_check_all(&self) -> HashMap<ChannelKind, ChannelHealth>;

    /// Persist all channel states (called on graceful shutdown)
    pub async fn save_all_state(&self) -> Result<()>;

    /// Restore channel states (called on startup)
    pub async fn restore_all_state(&self) -> Result<()>;
}
```

### 5c. Configuration

```toml
# mcclawd.toml — channel configuration

[channels.telegram]
enabled = true
token_secret = "TELEGRAM_BOT_TOKEN"   # looked up via SecretBackend
webhook_url = "https://example.com/webhook/telegram"
# or polling mode:
# polling = true
allowed_chat_ids = []                  # empty = allow all
admin_user_ids = [123456789]

[channels.discord]
enabled = false
token_secret = "DISCORD_BOT_TOKEN"
guild_ids = []

[channels.web]
enabled = true
# configured via [server] section

[channels.cli]
enabled = true
# always available in non-daemon mode
```

---

## 6. Telegram Adapter

### 6a. Crate Choice: teloxide

**teloxide** is the standard Rust Telegram bot framework. It provides:
- Typed API bindings for the full Telegram Bot API
- Long-polling and webhook modes
- Dialogue/FSM framework (we won't use this -- McClawd has its own session management)
- File upload/download helpers
- Graceful shutdown support

**Transport pattern:** A (Stateless API) in webhook mode, B (Long-Poll) in polling mode. Both map cleanly to our Channel trait.

### 6b. Type Sketch

```rust
// crates/mcclawd-channel-telegram/src/lib.rs

use teloxide::prelude::*;
use teloxide::types::{Message as TgMessage, ChatKind, MediaKind};

pub struct TelegramChannel {
    bot: Bot,
    config: TelegramConfig,
    /// Active "typing" indicators per chat
    typing_tasks: RwLock<HashMap<i64, JoinHandle<()>>>,
    /// Message ID of the "in-progress" message being edited (for streaming)
    progress_messages: RwLock<HashMap<SessionKey, i64>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    /// Bot token (from SecretBackend)
    pub token: String,

    /// Webhook URL (None = use long-polling)
    pub webhook_url: Option<String>,

    /// Allowed chat IDs (empty = allow all private chats)
    pub allowed_chat_ids: Vec<i64>,

    /// Users with admin privileges
    pub admin_user_ids: Vec<i64>,

    /// Max message length before splitting (Telegram limit: 4096)
    pub max_message_length: usize,

    /// Whether to use "edit in place" for streaming output
    pub edit_streaming: bool,
}

#[async_trait]
impl Channel for TelegramChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Telegram
    }

    fn display_name(&self) -> &str {
        "Telegram"
    }

    async fn start(
        &self,
        inbound_tx: mpsc::Sender<Envelope>,
        shutdown: CancellationToken,
    ) -> Result<()> {
        // Depending on config, either:
        // (a) Start long-polling loop with teloxide::dispatching
        // (b) Register webhook and start axum route for callbacks
        //
        // Both normalize TgMessage → Envelope and send on inbound_tx
        todo!()
    }

    async fn send_chunk(
        &self,
        session_key: &SessionKey,
        chunk: OutboundChunk,
    ) -> Result<()> {
        let chat_id = session_key.platform_conversation_id()
            .parse::<i64>()?;

        match chunk {
            OutboundChunk::TextDelta(delta) => {
                // Accumulate deltas. When buffer exceeds threshold or
                // a debounce timer fires, edit the progress message.
                self.append_and_maybe_edit(session_key, chat_id, &delta).await
            }
            OutboundChunk::TextBlock(text) => {
                // Split if > 4096 chars, send as new message(s)
                self.send_text(chat_id, &text).await
            }
            OutboundChunk::Typing => {
                self.bot.send_chat_action(
                    ChatId(chat_id),
                    teloxide::types::ChatAction::Typing,
                ).await?;
                Ok(())
            }
            OutboundChunk::Media(attachment) => {
                self.send_media(chat_id, attachment).await
            }
            OutboundChunk::Done => {
                // Finalize the progress message (remove "thinking..." suffix)
                self.finalize_progress(session_key, chat_id).await
            }
            OutboundChunk::Error(msg) => {
                self.send_text(chat_id, &format!("Error: {}", msg)).await
            }
            _ => Ok(()),  // ToolStart/ToolEnd: optionally show status
        }
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            max_message_length: 4096,
            markdown: true,          // MarkdownV2
            code_blocks: true,
            file_upload: true,
            max_file_size: 50 * 1024 * 1024,  // 50MB (Telegram limit)
            supported_media_types: vec![
                "image/*".into(), "audio/*".into(),
                "video/*".into(), "application/pdf".into(),
            ],
            message_edit: true,
            threading: true,         // reply_to_message_id
            reactions: true,
            streaming_mode: StreamingMode::EditInPlace,
        }
    }

    async fn health(&self) -> ChannelHealth {
        match self.bot.get_me().await {
            Ok(_) => ChannelHealth::Healthy,
            Err(e) => ChannelHealth::Unhealthy(e.to_string()),
        }
    }
}
```

### 6c. Message Normalization (Telegram -> Envelope)

```rust
impl TelegramChannel {
    fn normalize(&self, msg: TgMessage) -> Option<Envelope> {
        let peer = Peer {
            platform_id: msg.from()?.id.0.to_string(),
            mcclawd_user_id: None,  // set by auth_map stage
            display_name: msg.from().map(|u| {
                u.full_name()
            }),
            avatar_url: None,
        };

        let content = match msg.kind {
            // Bot command: /ask how do I...
            _ if msg.text().map_or(false, |t| t.starts_with('/')) => {
                let text = msg.text().unwrap();
                let (cmd, args) = text.split_once(' ')
                    .unwrap_or((text, ""));
                MessageContent::Command {
                    name: cmd.trim_start_matches('/').to_string(),
                    args: args.to_string(),
                }
            }
            // Plain text
            _ if msg.text().is_some() => {
                MessageContent::Text(msg.text().unwrap().to_string())
            }
            // Voice message
            _ if msg.voice().is_some() => {
                MessageContent::Voice {
                    transcription: None,  // STT pipeline fills this
                    media_ref: self.voice_to_media_ref(msg.voice().unwrap()),
                }
            }
            // Photo (take largest resolution)
            _ if !msg.photo().unwrap_or_default().is_empty() => {
                // Convert to attachment, extract caption as text
                MessageContent::Text(
                    msg.caption().unwrap_or("(photo)").to_string()
                )
            }
            _ => return None,  // unsupported message type
        };

        let platform_meta = PlatformMeta::Telegram {
            chat_id: msg.chat.id.0,
            message_id: msg.id.0 as i64,
            chat_type: match msg.chat.kind {
                ChatKind::Private(_) => "private",
                ChatKind::Public(ref p) => match p.kind {
                    _ => "group",
                },
            }.to_string(),
        };

        Some(Envelope {
            id: MessageId::new(),
            channel: ChannelKind::Telegram,
            peer,
            thread: msg.reply_to_message().map(|reply| ThreadContext {
                thread_id: msg.chat.id.0.to_string(),
                reply_to: Some(MessageId::from_platform(
                    "telegram",
                    reply.id.0.to_string(),
                )),
            }),
            content,
            attachments: self.extract_attachments(&msg),
            platform_meta,
            platform_timestamp: msg.date,
            received_at: Utc::now(),
        })
    }
}
```

### 6d. Streaming Output via Edit-in-Place

Telegram doesn't support true streaming. The adapter uses "edit in place":

1. On first `TextDelta`, send a new message with the delta text + a typing indicator suffix
2. On subsequent `TextDelta`s, accumulate text and call `editMessageText` (rate-limited to 1 edit/second per Telegram API limits)
3. On `Done`, send a final `editMessageText` removing the typing indicator
4. If accumulated text exceeds 4096 chars, finalize current message and start a new one

```rust
impl TelegramChannel {
    async fn append_and_maybe_edit(
        &self,
        session_key: &SessionKey,
        chat_id: i64,
        delta: &str,
    ) -> Result<()> {
        let mut progress = self.progress_messages.write().await;

        match progress.get(session_key) {
            None => {
                // First delta: send new message
                let sent = self.bot
                    .send_message(ChatId(chat_id), format!("{delta} ..."))
                    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                    .await?;
                progress.insert(session_key.clone(), sent.id.0);
                // Start accumulator in separate state
            }
            Some(&msg_id) => {
                // Subsequent delta: debounced edit
                // (actual implementation uses a debounce timer to avoid
                //  hitting Telegram's rate limit of ~30 edits/minute)
            }
        }
        Ok(())
    }
}
```

### 6e. Bot Commands

```
/start          — Register with McClawd, create session
/ask <prompt>   — Send a prompt to the agent
/cancel         — Cancel current task
/status         — Show running tasks
/help           — List available commands
/workspace      — Switch agent workspace
/agent <name>   — Switch to a specific agent (from AGENTS.md)
```

Bot commands are normalized to `MessageContent::Command` and routed through the pipeline like any other message. The command dispatcher lives in the pipeline, not in the Telegram adapter.

---

## 7. Session Management

### 7a. Session Key

Sessions are keyed by the triple `(agent, channel, peer)`. This means the same Telegram user talking to two different agents has two separate sessions with independent context.

```rust
/// Unique session identifier. Composite key ensures isolation.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionKey {
    /// Which agent is handling this session
    pub agent_id: AgentId,

    /// Which channel the user is on
    pub channel: ChannelKind,

    /// Platform-specific peer identifier
    pub peer_id: String,

    /// Platform-specific conversation ID
    /// (e.g., Telegram chat_id, Discord channel_id)
    /// Separate from peer_id because group chats have multiple peers
    pub conversation_id: String,
}

impl SessionKey {
    pub fn platform_conversation_id(&self) -> &str {
        &self.conversation_id
    }
}
```

### 7b. Session State

```rust
pub struct Session {
    pub key: SessionKey,

    /// Current task (if any)
    pub active_task: Option<TaskId>,

    /// Conversation history (bounded ring buffer)
    pub history: ConversationHistory,

    /// Working memory for the agent (tool results, scratchpad)
    pub memory: WorkingMemory,

    /// Channel-specific user identity (mapped from platform auth)
    pub user: Option<AuthenticatedUser>,

    /// Session metadata
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub message_count: u64,

    /// Session configuration overrides
    pub config: SessionConfig,
}

pub struct ConversationHistory {
    /// Max messages to retain
    max_entries: usize,
    /// Ring buffer of messages
    entries: VecDeque<HistoryEntry>,
}

pub struct HistoryEntry {
    pub role: Role,       // User, Assistant, System, Tool
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub metadata: Option<serde_json::Value>,
}

pub struct SessionConfig {
    /// Which agent handles this session (can be switched via /agent)
    pub agent_id: AgentId,
    /// Max conversation history entries
    pub max_history: usize,
    /// Session timeout (auto-cleanup after inactivity)
    pub timeout: Duration,
    /// Whether to persist session across daemon restarts
    pub persistent: bool,
}
```

### 7c. Session Manager

```rust
pub struct SessionManager {
    /// Active sessions
    sessions: RwLock<HashMap<SessionKey, Arc<RwLock<Session>>>>,

    /// Session persistence backend
    store: Arc<dyn SessionStore>,

    /// Default session configuration
    defaults: SessionConfig,
}

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn save(&self, session: &Session) -> Result<()>;
    async fn load(&self, key: &SessionKey) -> Result<Option<Session>>;
    async fn delete(&self, key: &SessionKey) -> Result<()>;
    async fn list_active(&self) -> Result<Vec<SessionKey>>;
    async fn cleanup_expired(&self, timeout: Duration) -> Result<usize>;
}

impl SessionManager {
    /// Get or create session for an inbound message
    pub async fn get_or_create(
        &self,
        envelope: &Envelope,
    ) -> Result<Arc<RwLock<Session>>>;

    /// End a session (user disconnected, timeout, explicit /end)
    pub async fn end_session(&self, key: &SessionKey) -> Result<()>;

    /// Cross-channel session linking: find sessions for same user
    /// across different channels
    pub async fn find_linked_sessions(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<SessionKey>>;

    /// Periodic cleanup of expired sessions
    pub async fn cleanup_loop(&self, interval: Duration);
}
```

### 7d. Cross-Channel Context

When a user is identified (via auth mapping) across channels, their sessions can share context:

- **Conversation history** remains per-session (channel-specific)
- **Working memory** (tool results, agent scratchpad) can be shared or isolated per config
- **User preferences** are global (stored against UserId, not SessionKey)

This is intentionally conservative. Full cross-channel conversation merging is a Phase 3+ concern.

---

## 8. Media Handling

### 8a. Design

Media flows through a `MediaStore` abstraction. Channel adapters download platform-specific media, store it in the MediaStore, and reference it via `MediaRef`. The agent engine and outbound router use `MediaRef` to access media without knowing the storage backend.

```rust
/// Reference to stored media. Lightweight, serializable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRef {
    pub id: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub filename: Option<String>,
}

/// Media attachment on an envelope or outbound chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub media_ref: MediaRef,
    pub kind: AttachmentKind,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttachmentKind {
    Image,
    Audio,
    Video,
    Document,
    VoiceNote,
    Other,
}

/// Abstraction over media storage.
#[async_trait]
pub trait MediaStore: Send + Sync {
    /// Store media from bytes, return reference
    async fn store(
        &self,
        data: Bytes,
        mime_type: &str,
        filename: Option<&str>,
    ) -> Result<MediaRef>;

    /// Store media from a URL (download first)
    async fn store_from_url(&self, url: &str) -> Result<MediaRef>;

    /// Retrieve media bytes
    async fn get(&self, media_ref: &MediaRef) -> Result<Bytes>;

    /// Get a time-limited URL for the media (if backend supports it)
    async fn presigned_url(
        &self,
        media_ref: &MediaRef,
        ttl: Duration,
    ) -> Result<Option<String>>;

    /// Delete media
    async fn delete(&self, media_ref: &MediaRef) -> Result<()>;

    /// Cleanup expired media
    async fn cleanup(&self, max_age: Duration) -> Result<usize>;
}
```

### 8b. Implementations (phased)

| Phase | Backend | Use Case |
|-------|---------|----------|
| Phase 2 | `LocalMediaStore` | Files in `data_dir/media/`. Simple, no deps. |
| Phase 3 | `S3MediaStore` | Production deployments with S3/MinIO. |

### 8c. Cross-Channel Media Flow

```
Telegram → download file via Bot API → MediaStore.store() → MediaRef in Envelope
                                                                    │
Agent processes message, generates response with image              │
                                                                    ▼
OutboundChunk::Media(Attachment) → Outbound Router
                                     │
                ┌────────────────────┼────────────────────┐
                ▼                    ▼                    ▼
          Telegram               Discord              Web/WS
          upload via             upload via            serve via
          sendPhoto              REST API              /api/media/:id
```

### 8d. Transcoding

Some channels need media in specific formats. The media pipeline supports optional transcoding:

```rust
pub struct TranscodeRequest {
    pub source: MediaRef,
    pub target_mime: String,
    pub max_dimension: Option<u32>,
    pub max_file_size: Option<u64>,
}
```

Phase 2 keeps transcoding simple (image resize only via the `image` crate). Full video/audio transcoding is Phase 3+.

---

## 9. Authentication & Identity Mapping

### 9a. The Problem

Each channel has its own identity system:
- **Telegram**: numeric user_id (stable, unique per bot)
- **Discord**: snowflake user_id + OAuth2
- **Slack**: workspace-scoped user_id
- **Web**: JWT from McClawd auth
- **CLI**: local user (always trusted)

McClawd needs to map these to a unified `UserId` for:
- Access control (who can use the agent)
- Session linking (same user across channels)
- Audit logging

### 9b. Identity Mapping

```rust
/// McClawd unified user identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedUser {
    /// McClawd internal user ID
    pub user_id: UserId,

    /// User's role
    pub role: UserRole,

    /// Linked platform identities
    pub identities: Vec<PlatformIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformIdentity {
    pub channel: ChannelKind,
    pub platform_id: String,
    pub verified: bool,
    pub linked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserRole {
    Admin,      // full access, manage agents/channels
    User,       // normal access, interact with agents
    Guest,      // limited access (e.g., read-only, rate-limited)
    Blocked,    // denied access
}
```

### 9c. Auth Pipeline Stage

The `auth_map` stage in the InboundPipeline resolves platform identity to McClawd identity:

```rust
pub struct AuthMapper {
    /// Platform identity → McClawd user mapping
    store: Arc<dyn IdentityStore>,

    /// Per-channel access control rules
    access_rules: HashMap<ChannelKind, AccessRule>,
}

#[derive(Debug, Clone)]
pub enum AccessRule {
    /// Anyone can interact (auto-create guest account)
    Open,
    /// Only pre-registered users
    RegisteredOnly,
    /// Only users with specific platform IDs
    AllowList(Vec<String>),
    /// Specific platform IDs blocked
    BlockList(Vec<String>),
}

#[async_trait]
pub trait IdentityStore: Send + Sync {
    /// Look up McClawd user by platform identity
    async fn resolve(
        &self,
        channel: &ChannelKind,
        platform_id: &str,
    ) -> Result<Option<AuthenticatedUser>>;

    /// Link a platform identity to a McClawd user
    async fn link(
        &self,
        user_id: &UserId,
        channel: &ChannelKind,
        platform_id: &str,
    ) -> Result<()>;

    /// Create a new McClawd user from a platform identity
    async fn create_from_platform(
        &self,
        channel: &ChannelKind,
        platform_id: &str,
        display_name: Option<&str>,
        role: UserRole,
    ) -> Result<AuthenticatedUser>;
}
```

### 9d. Linking Flow

1. **Telegram user sends `/start`** → auth_map checks IdentityStore → no match → creates Guest user
2. **User sends `/link`** → McClawd generates a one-time code
3. **User enters code in Web UI** (already authenticated via JWT) → links Telegram identity to existing McClawd user
4. **Future messages** from that Telegram user resolve to the linked McClawd user

```
Telegram user_id: 123456789
        │
        ▼
   IdentityStore.resolve("telegram", "123456789")
        │
        ├── Found → AuthenticatedUser { user_id: "mc_abc123", role: User }
        │
        └── Not found → AccessRule check
                │
                ├── Open → create_from_platform() → Guest user
                ├── RegisteredOnly → reject with "Use /link to register"
                └── AllowList → reject with "Not authorized"
```

---

## 10. Inbound Pipeline Evolution

### 10a. Pipeline Stages

```rust
pub struct InboundPipeline {
    /// All registered channels feed into this receiver
    inbound_rx: mpsc::Receiver<Envelope>,

    /// Pipeline stages (executed in order)
    stages: Vec<Box<dyn PipelineStage>>,

    /// Session manager (lookup/create sessions)
    session_manager: Arc<SessionManager>,

    /// Task manager (dispatch to agent engine)
    task_manager: Arc<TaskManager>,
}

#[async_trait]
pub trait PipelineStage: Send + Sync {
    /// Process an envelope. Return Some to continue, None to drop.
    async fn process(&self, envelope: Envelope) -> Result<Option<Envelope>>;
}
```

**Stage order (Phase 2):**

| # | Stage | Purpose |
|---|-------|---------|
| 1 | `NormalizeStage` | Ensure envelope fields are well-formed (trim whitespace, validate IDs) |
| 2 | `DedupStage` | Drop duplicate messages (by platform message ID, 5-minute window) |
| 3 | `AuthMapStage` | Resolve platform identity → McClawd user, set `peer.mcclawd_user_id` |
| 4 | `AccessControlStage` | Check user role against channel access rules |
| 5 | `RateLimitStage` | Per-user, per-channel rate limiting |
| 6 | `CommandDispatchStage` | Handle bot commands (/start, /help, /link, /cancel) |
| 7 | `RouteStage` | Map envelope to SessionKey, find or create session |
| 8 | `DebounceStage` | Coalesce rapid-fire messages (100ms window) |
| 9 | `DispatchStage` | Send to TaskManager for agent processing |

### 10b. Outbound Router

The outbound router takes `(SessionKey, OutboundChunk)` pairs and routes them to the correct channel adapter, applying format transformations based on channel capabilities.

```rust
pub struct OutboundRouter {
    registry: Arc<ChannelRegistry>,
    formatters: HashMap<ChannelKind, Box<dyn OutboundFormatter>>,
}

#[async_trait]
pub trait OutboundFormatter: Send + Sync {
    /// Transform an OutboundChunk for this channel's capabilities.
    /// E.g., convert Markdown to Telegram MarkdownV2, split long messages.
    fn format(&self, chunk: OutboundChunk, caps: &ChannelCapabilities)
        -> Vec<OutboundChunk>;
}

impl OutboundRouter {
    pub async fn send(
        &self,
        session_key: &SessionKey,
        chunk: OutboundChunk,
    ) -> Result<()> {
        let kind = &session_key.channel;
        let caps = self.registry.capabilities(kind)
            .ok_or(Error::ChannelNotFound(kind.clone()))?;

        let formatter = self.formatters.get(kind)
            .unwrap_or(&self.default_formatter);

        let formatted = formatter.format(chunk, &caps);

        for chunk in formatted {
            self.registry.send(session_key, chunk).await?;
        }
        Ok(())
    }
}
```

---

## 11. Crate Organization

### 11a. New Crates

| Crate | Purpose |
|-------|---------|
| `mcclawd-channel-telegram` | Telegram adapter (teloxide) |
| `mcclawd-media` | MediaStore trait + local backend |

### 11b. Modified Crates

| Crate | Changes |
|-------|---------|
| `mcclawd-channels` | Evolved Channel trait, Envelope, ChannelRegistry, OutboundRouter, pipeline stages |
| `mcclawd-core` | UserId type, UserRole, PlatformIdentity, IdentityStore trait |
| `mcclawd-tasks` | Accept Envelope instead of InboundMessage, session-aware task creation |
| `mcclawd-api` | Channel management REST endpoints, Telegram webhook route |

### 11c. Dependency Graph

```
mcclawd-core (types, identity, secrets)
    │
    ├── mcclawd-channels (Channel trait, Envelope, registry, pipeline)
    │       │
    │       ├── mcclawd-channel-telegram (teloxide adapter)
    │       │
    │       └── mcclawd-media (media store)
    │
    ├── mcclawd-agent (unchanged)
    │
    ├── mcclawd-tasks (Envelope-aware)
    │
    └── mcclawd-api (channel management endpoints)
```

---

## 12. API Endpoints (Channel Management)

```
POST   /api/channels                  — Register a new channel
DELETE /api/channels/:kind            — Remove a channel
GET    /api/channels                  — List all channels with status
GET    /api/channels/:kind/health     — Health check for a channel
POST   /api/channels/:kind/restart    — Restart a channel

GET    /api/sessions                  — List active sessions
GET    /api/sessions/:key             — Get session details
DELETE /api/sessions/:key             — End a session

POST   /api/users/link                — Link platform identity to user
GET    /api/users/:id/identities      — List linked identities
DELETE /api/users/:id/identities/:platform — Unlink identity

POST   /webhook/telegram              — Telegram webhook callback
POST   /webhook/slack                 — Slack Events API callback (Phase 3)
```

---

## 13. Implementation Plan

### Phase 2a: Foundation (Week 1-2)

1. **Evolve Channel trait** — add `stop()`, `save_state()`/`restore_state()`, `capabilities()`, `health()`
2. **Implement Envelope** — replace InboundMessage with Envelope throughout
3. **Build ChannelRegistry** — registration, routing, health monitoring
4. **Update InboundPipeline** — add auth_map, rate_limit, command_dispatch stages
5. **Build OutboundRouter** — capability-aware formatting and routing
6. **Build SessionManager** — with SQLite-backed SessionStore

### Phase 2b: Telegram (Week 3)

7. **Create `mcclawd-channel-telegram`** crate
8. **Implement polling mode** first (simpler, no public URL needed)
9. **Message normalization** (text, commands, photos, voice, documents)
10. **Edit-in-place streaming** with debounced edits
11. **Bot command handling** (/start, /ask, /cancel, /status, /link)
12. **Webhook mode** (optional, for production deployments)

### Phase 2c: Media & Identity (Week 4)

13. **Build `mcclawd-media`** crate with LocalMediaStore
14. **Telegram media pipeline** (download, store, reference)
15. **IdentityStore** with SQLite backend
16. **Identity linking flow** (/link command + web UI verification)
17. **Access control rules** per channel

### Phase 2d: Integration & Testing (Week 5)

18. **E2E tests**: Telegram adapter with mock Bot API
19. **Integration tests**: multi-channel routing (CLI + Web + Telegram)
20. **Load tests**: concurrent sessions across channels
21. **Documentation**: operator guide for Telegram setup
22. **UI updates**: channel management page, session viewer

---

## 14. Open Questions

1. **Database choice for SessionStore/IdentityStore**: SQLite (single-node simplicity) vs Postgres (multi-node ready)?
   **Recommendation:** SQLite for Phase 2, with a trait abstraction that allows Postgres swap in Phase 3.

2. **Voice message handling**: Should Telegram voice notes be transcribed before reaching the agent?
   **Recommendation:** Yes, via Whisper API (OpenAI) or local whisper.cpp. The agent receives text; the original audio is stored as an attachment.

3. **Group chat support**: Should the bot respond to all messages in a group, or only when mentioned?
   **Recommendation:** Only when mentioned (@bot or /command) in groups. All messages in private chats.

4. **Rate limiting strategy**: Per-user global, or per-user-per-channel?
   **Recommendation:** Per-user-per-channel with a global cap. Prevents a single channel from exhausting the user's budget.

5. **Message queue**: Should the pipeline use a message queue (e.g., NATS) between stages?
   **Recommendation:** No, for Phase 2. In-process `mpsc` channels are sufficient for single-node. Add NATS/Redis Streams in Phase 3 if multi-node deployment is needed.

---

## 15. Security Considerations

1. **Bot tokens** stored via SecretBackend (encrypted at rest), never in config files or env vars
2. **Webhook endpoints** validated with Telegram's secret_token header
3. **Identity linking** requires proof-of-possession (one-time code, not just platform ID)
4. **Rate limiting** prevents abuse from any single channel/user
5. **Media cleanup** prevents disk exhaustion (TTL-based, configurable)
6. **Input sanitization** on all normalized messages (strip control characters, limit length)
7. **PlatformMeta** is opaque to the agent — platform-specific data never enters LLM context
8. **Audit logging** for all identity operations (link, unlink, role changes)

---

## 16. Migration Path

### From Phase 1 to Phase 2

1. `InboundMessage` is replaced by `Envelope`. Existing CLI and Web adapters are updated to produce `Envelope` instead.
2. The `Channel` trait gains new methods with default implementations, so existing adapters compile without changes.
3. `SessionKey` replaces the current task-based session routing.
4. The Web channel's WebSocket handler continues to work but now routes through the `OutboundRouter`.

### Breaking Changes

- `InboundMessage` → `Envelope` (all pipeline consumers must update)
- `send_chunk(&self, chunk)` → `send_chunk(&self, session_key, chunk)` (channels must know which conversation to target)
- Session creation is now explicit (via SessionManager) rather than implicit in task creation

These are internal API changes only. No user-facing breaking changes.
