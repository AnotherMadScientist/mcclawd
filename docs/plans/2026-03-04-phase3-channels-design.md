# Phase 3: Full Channel Ecosystem — Design & Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan.

**Goal:** Add Discord, Slack, WhatsApp, and Email channel adapters following the Telegram pattern, and wire the existing Telegram adapter to live teloxide.

**Architecture:** Each channel is a separate crate (`mcclawd-channel-{name}`) implementing the `Channel` trait from `mcclawd-channels`. Every adapter normalizes platform-specific messages into `Envelope` types and routes outbound `OutboundChunk` back through mpsc channels. Platform SDKs are abstracted behind the adapter so all business logic remains platform-agnostic.

**Tech Stack:** serenity 0.12 (Discord), slack-morphism 2.x (Slack), reqwest (WhatsApp Cloud API), lettre + async-imap (Email), teloxide 0.13 (Telegram enhancement)

---

## Pre-requisites: Extend Core Types

Before creating channel crates, extend `ChannelKind` and `Platform` enums:

### Task 0: Extend ChannelKind + Platform enums

**Files:**
- Modify: `crates/mcclawd-channels/src/types.rs` — add `Slack`, `WhatsApp`, `Email` to `ChannelKind`
- Modify: `crates/mcclawd-channels/src/envelope.rs` — add `WhatsApp` to `Platform`
- Modify: `crates/mcclawd-channels/src/traits.rs` — add `platform()` mappings for new variants

**Changes:**
1. `ChannelKind`: add `Slack`, `WhatsApp`, `Email` variants + Display impls
2. `Platform`: add `WhatsApp` variant + Display impl
3. `Channel::platform()` default: map new ChannelKind variants to Platform

---

## Group A: Discord Channel (serenity 0.12)

### Task 1: Create mcclawd-channel-discord crate scaffold

**Files:**
- Create: `crates/mcclawd-channel-discord/Cargo.toml`
- Create: `crates/mcclawd-channel-discord/src/lib.rs`
- Create: `crates/mcclawd-channel-discord/src/error.rs`
- Modify: `Cargo.toml` (workspace members + deps)

**Cargo.toml deps:** mcclawd-channels, mcclawd-core, serenity = { version = "0.12", features = ["client", "gateway", "model"] }, tokio, async-trait, serde, serde_json, tracing, chrono, uuid, thiserror, tokio-util, anyhow

### Task 2: Discord normalize module

**Files:**
- Create: `crates/mcclawd-channel-discord/src/normalize.rs`

Intermediate types: `DiscordMessage { message_id, channel_id, guild_id, author_id, author_name, content, attachments, timestamp }` and `DiscordAttachment { filename, url, content_type }`.

`normalize(msg: DiscordMessage) -> Envelope` maps:
- peer.id = author_id, peer.display_name = author_name, peer.platform = Discord
- thread = Some(ThreadContext { thread_id: channel_id, parent_message_id: None })
- content = Text(content) or Command if starts with "/"
- attachments from DiscordAttachment → Attachment { filename, mime_type, media_ref: Url }
- platform_meta = json!({ "channel_id", "guild_id", "message_id" })

Tests: text, command, attachments, empty content, thread context.

### Task 3: Discord adapter + Channel trait impl

**Files:**
- Create: `crates/mcclawd-channel-discord/src/adapter.rs`

Pattern: Same mpsc pattern as Telegram.
- `DiscordConfig { bot_token, allowed_guild_ids: Option<Vec<u64>>, allowed_channel_ids: Option<Vec<u64>> }`
- `DiscordChannel` with inbox_rx/tx, outbound_tx/rx
- `start_listener()` placeholder for serenity Client
- `discord_capabilities()`: supports_streaming: false, supports_edit: true, supports_markdown: true, max_message_len: 2000, supports_files: true, max_file_size: 25MB
- Channel trait: kind=Discord, platform=Discord, recv_envelope from inbox_rx, send_chunk to outbound_tx

Tests: inject message, send chunk, capabilities, ordering, config.

### Task 4: Discord integration tests

**Files:**
- Create: `crates/mcclawd-channel-discord/tests/discord_integration.rs`

Tests: full normalize→channel flow, registry integration, multi-message ordering, media message flow.

---

## Group B: Slack Channel (slack-morphism)

### Task 5: Create mcclawd-channel-slack crate scaffold

**Files:**
- Create: `crates/mcclawd-channel-slack/Cargo.toml`
- Create: `crates/mcclawd-channel-slack/src/lib.rs`
- Create: `crates/mcclawd-channel-slack/src/error.rs`
- Modify: `Cargo.toml` (workspace members + deps)

**Cargo.toml deps:** mcclawd-channels, mcclawd-core, slack-morphism = "2", reqwest, tokio, async-trait, serde, serde_json, tracing, chrono, uuid, thiserror, tokio-util, anyhow

### Task 6: Slack normalize module

**Files:**
- Create: `crates/mcclawd-channel-slack/src/normalize.rs`

Intermediate types: `SlackMessage { ts, channel_id, user_id, user_name, text, thread_ts, files }` and `SlackFile { name, url_private, mimetype }`.

`normalize(msg: SlackMessage) -> Envelope` maps:
- peer.id = user_id, peer.display_name = user_name, peer.platform = Slack
- thread = thread_ts.map(|ts| ThreadContext { thread_id: ts, parent_message_id: None })
- content = Text(text) or Command if starts with "/"
- attachments from SlackFile → Attachment { filename, mime_type, media_ref: Url(url_private) }
- platform_meta = json!({ "channel_id", "ts", "thread_ts" })

Tests: text, threaded, command, files, no thread.

### Task 7: Slack adapter + Channel trait impl

**Files:**
- Create: `crates/mcclawd-channel-slack/src/adapter.rs`

Pattern: Same mpsc pattern.
- `SlackConfig { bot_token, app_token, allowed_channel_ids: Option<Vec<String>> }`
- `SlackChannel` with inbox_rx/tx, outbound_tx/rx
- `start_listener()` placeholder for Socket Mode
- `slack_capabilities()`: supports_streaming: false, supports_edit: true, supports_markdown: true (mrkdwn), max_message_len: 40000 (blocks), supports_files: true, max_file_size: 1GB for paid
- Channel trait: kind=Slack, platform=Slack

Tests: inject message, send chunk, capabilities, ordering, config.

### Task 8: Slack integration tests

**Files:**
- Create: `crates/mcclawd-channel-slack/tests/slack_integration.rs`

Tests: full normalize→channel flow, registry integration, threaded message flow.

---

## Group C: WhatsApp Channel (reqwest Cloud API)

### Task 9: Create mcclawd-channel-whatsapp crate scaffold

**Files:**
- Create: `crates/mcclawd-channel-whatsapp/Cargo.toml`
- Create: `crates/mcclawd-channel-whatsapp/src/lib.rs`
- Create: `crates/mcclawd-channel-whatsapp/src/error.rs`
- Modify: `Cargo.toml` (workspace members + deps)

**Cargo.toml deps:** mcclawd-channels, mcclawd-core, reqwest = { version = "0.12", features = ["json"] }, tokio, async-trait, serde, serde_json, tracing, chrono, uuid, thiserror, tokio-util, anyhow

### Task 10: WhatsApp normalize module

**Files:**
- Create: `crates/mcclawd-channel-whatsapp/src/normalize.rs`

Intermediate types: `WhatsAppMessage { message_id, from, from_name, text, media, timestamp }` and `WhatsAppMedia { id, mime_type, filename }`.

`normalize(msg: WhatsAppMessage) -> Envelope` maps:
- peer.id = from (phone number), peer.display_name = from_name, peer.platform = WhatsApp (new variant)
- thread = None (WhatsApp doesn't have threads)
- content = Text(text) or Command if starts with "/"
- attachments from WhatsAppMedia → Attachment { filename, mime_type, media_ref: PlatformId(id) }
- platform_meta = json!({ "message_id" })

Tests: text, media, command, no media.

### Task 11: WhatsApp adapter + Channel trait impl

**Files:**
- Create: `crates/mcclawd-channel-whatsapp/src/adapter.rs`

Pattern: Same mpsc pattern + webhook receiver.
- `WhatsAppConfig { phone_number_id, access_token, verify_token, allowed_numbers: Option<Vec<String>> }`
- `WhatsAppChannel` with inbox_rx/tx, outbound_tx/rx
- `start_webhook()` placeholder for webhook endpoint
- `whatsapp_capabilities()`: supports_streaming: false, supports_edit: false, supports_markdown: false (limited formatting), max_message_len: 4096, supports_files: true, max_file_size: 16MB (media) / 100MB (document)
- Channel trait: kind=WhatsApp, platform=WhatsApp

Tests: inject message, send chunk, capabilities, config.

### Task 12: WhatsApp integration tests

**Files:**
- Create: `crates/mcclawd-channel-whatsapp/tests/whatsapp_integration.rs`

Tests: full normalize→channel flow, registry integration, media message.

---

## Group D: Email Channel (lettre + async-imap)

### Task 13: Create mcclawd-channel-email crate scaffold

**Files:**
- Create: `crates/mcclawd-channel-email/Cargo.toml`
- Create: `crates/mcclawd-channel-email/src/lib.rs`
- Create: `crates/mcclawd-channel-email/src/error.rs`
- Modify: `Cargo.toml` (workspace members + deps)

**Cargo.toml deps:** mcclawd-channels, mcclawd-core, lettre = { version = "0.11", features = ["tokio1-rustls-tls", "smtp-transport", "builder"] }, async-imap = "0.10", tokio, async-trait, serde, serde_json, tracing, chrono, uuid, thiserror, tokio-util, anyhow, mailparse = "0.15"

### Task 14: Email normalize module

**Files:**
- Create: `crates/mcclawd-channel-email/src/normalize.rs`

Intermediate types: `EmailMessage { message_id, from_address, from_name, subject, body_text, body_html, in_reply_to, attachments }` and `EmailAttachment { filename, content_type, data }`.

`normalize(msg: EmailMessage) -> Envelope` maps:
- peer.id = from_address, peer.display_name = from_name, peer.platform = Email
- thread = in_reply_to.map(|id| ThreadContext { thread_id: id, parent_message_id: None })
- content = Text(body_text) (prefer plain text, fall back to stripping HTML)
- attachments from EmailAttachment → Attachment { filename, mime_type, media_ref: Local(temp path) }
  (Note: for tests use PlatformId with base64 stub; real impl writes to temp files)
- platform_meta = json!({ "message_id", "subject" })

Tests: plain text, with subject, reply threading, attachments, HTML fallback.

### Task 15: Email adapter + Channel trait impl

**Files:**
- Create: `crates/mcclawd-channel-email/src/adapter.rs`

Pattern: Same mpsc pattern + IMAP polling.
- `EmailConfig { imap_host, imap_port, smtp_host, smtp_port, username, password, from_address, allowed_senders: Option<Vec<String>>, poll_interval_secs: u64 }`
- `EmailChannel` with inbox_rx/tx, outbound_tx/rx
- `start_listener()` placeholder for IMAP IDLE polling
- `email_capabilities()`: supports_streaming: false, supports_edit: false, supports_markdown: false, max_message_len: 0 (unlimited), supports_files: true, max_file_size: 25MB (typical SMTP limit)
- Channel trait: kind=Email, platform=Email

Tests: inject message, send chunk, capabilities, config.

### Task 16: Email integration tests

**Files:**
- Create: `crates/mcclawd-channel-email/tests/email_integration.rs`

Tests: full normalize→channel flow, registry integration, threaded reply, attachments.

---

## Group E: Enhance Telegram + Final Integration

### Task 17: Wire teloxide dispatcher in Telegram adapter

**Files:**
- Modify: `crates/mcclawd-channel-telegram/src/adapter.rs`

Replace `start_listener` placeholder with actual teloxide wiring:
1. Create `teloxide::Bot::new(config.bot_token)`
2. Build handler: `Update::filter_message().endpoint(handle_message)`
3. `handle_message` converts `teloxide::types::Message` → `TelegramMessage` → `normalize()` → send to inbox_tx
4. Spawn dispatcher with shutdown token via `select!`
5. Filter by allowed_chat_ids if configured

Test: verify the handler function logic (unit test with mock message struct).

### Task 18: API routes for channel management

**Files:**
- Create: `crates/mcclawd-api/src/server/channels.rs`
- Modify: `crates/mcclawd-api/src/server/routes.rs`

Routes:
- `GET /api/channels` — list registered channels with capabilities
- `GET /api/channels/:id` — get channel details
- `POST /api/channels/:id/send` — send a test message to a channel

Tests: list empty, register and list, send to unknown channel.

### Task 19: Final integration tests

**Files:**
- Create: `tests/phase3_integration.rs` (workspace-level)

Tests:
- Register all 5 channel types in a ChannelRegistry
- Verify each channel's capabilities are distinct and correct
- Normalize a message from each platform and verify Envelope fields
- Outbound routing: send OutboundChunk to each channel type
