# Phase 3b: Live SDKs, Persistence, Security — Implementation Plan

> **For Claude:** Use subagent-driven development with parallel execution.

**Goal:** Wire live platform SDKs into channel adapters, add Postgres persistence for sessions/turns, and implement security hooks (DLP, secret scanning) plus cloud secret backends.

**Architecture:** Three independent workstreams executed in parallel. Each builds on existing traits (Channel, SecretBackend, SecurityHook) with minimal cross-stream dependencies.

---

## Workstream A: Wire Live Platform SDKs

Wire real SDK dispatchers into the placeholder `start_listener()` methods. Each adapter gets actual message handling.

### Task A1: Telegram — teloxide dispatcher

**Files:** Modify `crates/mcclawd-channel-telegram/Cargo.toml`, `crates/mcclawd-channel-telegram/src/adapter.rs`

Replace `start_listener` placeholder:
1. Add teloxide dependency (already in workspace)
2. Create `teloxide::Bot::new(config.bot_token)`
3. Build message handler that converts `teloxide::types::Message` → `TelegramMessage` → `normalize()` → inbox_tx
4. Filter by `allowed_chat_ids` if configured
5. Spawn dispatcher with shutdown token via `tokio::select!`
6. Handle outbound: spawn task reading outbound_rx, sending via `bot.send_message()`

### Task A2: Discord — serenity event handler

**Files:** Modify `crates/mcclawd-channel-discord/Cargo.toml`, `crates/mcclawd-channel-discord/src/adapter.rs`

Add serenity 0.12 dependency. Wire `start_listener`:
1. Create serenity `Client::builder(token, intents)` with GatewayIntents::MESSAGE_CONTENT
2. Implement `EventHandler` that converts serenity Message → DiscordMessage → normalize() → inbox_tx
3. Filter by allowed_guild_ids/allowed_channel_ids
4. Outbound: spawn task reading outbound_rx, sending via `channel_id.say()`

### Task A3: Slack — Socket Mode handler

**Files:** Modify `crates/mcclawd-channel-slack/Cargo.toml`, `crates/mcclawd-channel-slack/src/adapter.rs`

Add slack-morphism dependency. Wire `start_listener`:
1. Create `SlackClient` with bot_token
2. Connect via Socket Mode using app_token
3. Handle `message` events: convert to SlackMessage → normalize() → inbox_tx
4. Filter by allowed_channel_ids
5. Outbound: read outbound_rx, send via `chat.postMessage` API

### Task A4: WhatsApp — webhook handler

**Files:** Modify `crates/mcclawd-channel-whatsapp/Cargo.toml`, `crates/mcclawd-channel-whatsapp/src/adapter.rs`

Add reqwest + axum (for webhook receiver). Wire `start_webhook`:
1. Start small axum server on configurable port
2. GET webhook verification (verify_token check)
3. POST webhook receives messages: parse Cloud API payload → WhatsAppMessage → normalize() → inbox_tx
4. Filter by allowed_numbers
5. Outbound: POST to `graph.facebook.com/v18.0/{phone_number_id}/messages`

### Task A5: Email — IMAP + SMTP

**Files:** Modify `crates/mcclawd-channel-email/Cargo.toml`, `crates/mcclawd-channel-email/src/adapter.rs`

Add async-imap + lettre. Wire `start_listener`:
1. Connect to IMAP server with credentials
2. Poll INBOX (or IDLE if supported) at poll_interval_secs
3. Parse new emails with mailparse → EmailMessage → normalize() → inbox_tx
4. Filter by allowed_senders
5. Outbound: build lettre Message from OutboundChunk, send via SMTP

---

## Workstream B: Postgres Persistence

### Task B1: Add sqlx + migrations infrastructure

**Files:**
- Create `crates/mcclawd-core/migrations/` directory
- Modify `crates/mcclawd-core/Cargo.toml` — add sqlx with postgres feature
- Create `crates/mcclawd-core/src/persistence/mod.rs`
- Create `crates/mcclawd-core/src/persistence/postgres.rs`

Setup:
1. Add `sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "chrono", "uuid", "json"] }` to workspace deps
2. Create `PgPool` wrapper with connection management
3. Create migration infrastructure

### Task B2: Session + Turn persistence

**Files:**
- Create `crates/mcclawd-core/migrations/001_sessions.sql`
- Create `crates/mcclawd-core/src/persistence/sessions.rs`

Schema:
```sql
CREATE TABLE sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id TEXT NOT NULL,
    peer_id TEXT NOT NULL,
    platform TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at TIMESTAMPTZ,
    metadata JSONB DEFAULT '{}'
);

CREATE TABLE turns (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id UUID NOT NULL REFERENCES sessions(id),
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system', 'tool')),
    content TEXT NOT NULL,
    tool_calls JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sessions_peer ON sessions(peer_id, platform);
CREATE INDEX idx_turns_session ON turns(session_id, created_at);
```

Implement `SessionStore` trait + `PgSessionStore`:
- `create_session(channel_id, peer_id, platform) -> Session`
- `end_session(session_id)`
- `add_turn(session_id, role, content, tool_calls) -> Turn`
- `get_turns(session_id) -> Vec<Turn>`
- `get_recent_sessions(peer_id, limit) -> Vec<Session>`

### Task B3: Agent config persistence

**Files:**
- Create `crates/mcclawd-core/migrations/002_agent_configs.sql`
- Create `crates/mcclawd-core/src/persistence/agent_configs.rs`

Schema:
```sql
CREATE TABLE agent_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT UNIQUE NOT NULL,
    soul_md TEXT,
    agents_md TEXT,
    user_md TEXT,
    model_config JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

Implement `AgentConfigStore` trait + `PgAgentConfigStore`.

### Task B4: Postgres SecretBackend

**Files:**
- Create `crates/mcclawd-core/migrations/003_secrets.sql`
- Create `crates/mcclawd-core/src/secrets/postgres.rs`
- Modify `crates/mcclawd-core/src/secrets/mod.rs`

Schema:
```sql
CREATE TABLE secrets (
    name TEXT PRIMARY KEY,
    encrypted_value BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

Implement `SecretBackend` for `PgSecretBackend` — stores AES-encrypted values in Postgres (encryption key from vault key, not stored in DB).

---

## Workstream C: Security Hooks + Cloud Secret Backends

### Task C1: DLP scanning hook

**Files:**
- Create `crates/mcclawd-core/src/hooks/mod.rs` (refactor from hooks.rs)
- Create `crates/mcclawd-core/src/hooks/dlp.rs`

`DlpHook` implements `SecurityHook`:
- `before_tool_call`: scan args for PII patterns (SSN, credit card, email, phone, API keys)
- `after_tool_call`: scan results for secret leakage patterns
- Configurable: `DlpConfig { patterns: Vec<DlpPattern>, action: DlpAction }` where action = Warn | Block | Redact
- Built-in patterns: AWS keys (AKIA...), API tokens, SSNs, credit cards, emails
- Uses regex for pattern matching

Tests: detect API key in args, detect SSN in result, pass clean data, configurable action.

### Task C2: Secret scanning hook

**Files:**
- Create `crates/mcclawd-core/src/hooks/secret_scanner.rs`

`SecretScannerHook` implements `SecurityHook`:
- Entropy-based detection (Shannon entropy > threshold for base64/hex strings)
- Known secret patterns (AWS, GitHub, Slack, Stripe tokens)
- Cross-references with loaded secret values to detect leaks
- Configurable entropy threshold (default 4.5)

Tests: high entropy string detected, known patterns, low entropy passes.

### Task C3: Audit log to structured output

**Files:**
- Create `crates/mcclawd-core/src/hooks/audit.rs` (refactor AuditHook from hooks.rs)

Enhanced `AuditHook`:
- Structured JSON audit events (not just tracing)
- `AuditEvent { timestamp, tool_name, action: PreCall|PostCall, peer_id, session_id, args_hash, result_size, duration_ms, dlp_flags }`
- Write to configurable sink: file (JSONL), stderr, or future DB
- Composable with DLP hook (audit records DLP findings)

### Task C4: Hook pipeline (compose multiple hooks)

**Files:**
- Create `crates/mcclawd-core/src/hooks/pipeline.rs`

`HookPipeline` holds `Vec<Arc<dyn SecurityHook>>`, runs all hooks in order:
- `before_tool_call`: runs all hooks, first error stops chain
- `after_tool_call`: runs all hooks, collects all results
- Builder pattern: `HookPipeline::new().add(AuditHook).add(DlpHook::new(config))`

### Task C5: Environment variable backend

**Files:**
- Create `crates/mcclawd-core/src/secrets/env.rs`

`EnvSecretBackend` implements `SecretBackend`:
- Simple read-only backend that reads from environment variables
- `get()`: std::env::var, `set()`: error (read-only), `delete()`: error, `list()`: empty (can't enumerate)
- Useful for CI/CD and container deployments

---

## Integration

### Task I1: Wire hooks into agent supervisor

**Files:** Modify `crates/mcclawd-api/src/supervisor/agent_supervisor.rs`

- Accept `HookPipeline` in supervisor
- Call `pipeline.before_tool_call()` / `pipeline.after_tool_call()` around tool dispatch
- Default pipeline: AuditHook + DlpHook(default patterns)

### Task I2: API routes for hook + backend config

**Files:**
- Create `crates/mcclawd-api/src/server/security.rs`
- Modify routes.rs

Routes:
- `GET /api/security/hooks` — list active hooks
- `GET /api/security/audit` — recent audit events
- `GET /api/secrets/backends` — list available backends
