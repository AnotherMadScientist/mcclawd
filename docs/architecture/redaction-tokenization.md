# Redaction Tokenization Architecture

> Unified system for replacing sensitive data with typed tokens at trust boundaries.
> Covers secrets, PII, DLP matches, OAuth/A2A tokens, and env var secrets.

## Token Format

```
{TYPE:LABEL:…SUFFIX}
```

| Type | Example | What it replaces |
|------|---------|-----------------|
| SECRET | `{SECRET:ANTHROPIC_API_KEY:…3kF9}` | API key from SecretBackend |
| SECRET | `{SECRET:GITHUB_TOKEN:…xQ2m}` | Rotated key (different suffix) |
| SECRET | `{SECRET:OAUTH_ACCESS_TOKEN:…xyz}` | OAuth bearer token |
| SECRET | `{SECRET:A2A_AGENT_TOKEN:…890}` | Agent-to-agent auth token |
| PII | `{PII:CREDIT_CARD:…4242}` | Credit card number |
| PII | `{PII:PHONE:…7890}` | Phone number |
| PII | `{PII:EMAIL:…@acme.com}` | Email address (domain as suffix) |
| PII | `{PII:SSN:…6789}` | Social security number |
| DLP | `{DLP:AWS_ACCESS_KEY:…WZYX}` | Detected by DLP pattern match |
| DLP | `{DLP:PRIVATE_KEY:…a7b2}` | SHA256 last 4 of key |

### Suffix Rules

- Numbers (card, phone, SSN): last 4 digits
- API keys/tokens: last 4 alphanumeric characters
- Email: `…@domain.tld`
- Private keys: last 4 hex chars of SHA256 hash
- Generic strings: last 4 characters

### Collision Handling

Same label + different value = different suffix. If suffixes collide:
```
{PII:CREDIT_CARD:…4242}       (first card ending 4242)
{PII:CREDIT_CARD:…4242:a3}    (second card ending 4242, disambiguated)
```

---

## DLP + Audit Dataflow (current system — 109 patterns)

```
┌─────────────────────────────────────────────────────────────────────┐
│                        USER PROMPT                                  │
│  "Summarize this doc, my AWS key is AKIA..."                       │
└────────────────────────────────┬────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│  1. INBOUND PROMPT SCAN (tasks.rs)                                  │
│                                                                     │
│  pipeline.set_task_context(task_id)     ← associates all findings  │
│  pipeline.before_tool_call("user_prompt", {prompt: "..."})          │
│                                                                     │
│  Hook chain executes in order (run-all semantics):                  │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ ① DlpHook (109 regex patterns)                              │   │
│  │   Scans: Cloud keys, AI/ML keys, SaaS tokens, package       │   │
│  │   registry tokens, crypto, auth, PII (global + US + HIPAA), │   │
│  │   prompt injection, command injection, SQL injection,        │   │
│  │   encoding bypass, social engineering, data exfiltration     │   │
│  │   → Pushes PendingFinding to shared SecurityContext          │   │
│  │   → Elevates threat_level ("safe" → "dangerous")            │   │
│  │   → Returns Err on Block action                             │   │
│  ├──────────────────────────────────────────────────────────────┤   │
│  │ ② SecretScannerHook (Shannon entropy)                        │   │
│  │   Runs even after DlpHook errors (run-all semantics)         │   │
│  │   Shannon entropy on tokens ≥ 20 chars: ≥ 4.5 bits → flag   │   │
│  │   → Pushes findings to SecurityContext                       │   │
│  ├──────────────────────────────────────────────────────────────┤   │
│  │ ③ SecuritySidecarHook (POST localhost:8082)                  │   │
│  │   External container with prompt injection detection          │   │
│  │   → Findings merged into SecurityContext                     │   │
│  ├──────────────────────────────────────────────────────────────┤   │
│  │ ④ AuditHook (last — reads all accumulated findings)          │   │
│  │   Enriches AuditEvent from SecurityContext                   │   │
│  │   Sinks to: TracingAuditSink | FileAuditSink | PgAuditSink  │   │
│  │   PgAuditSink: INSERT security_events + dlp_findings         │   │
│  │   (fire-and-forget via tokio::spawn)                         │   │
│  │   Only persists if findings non-empty AND action ≠ "allowed" │   │
│  └──────────────────────────────────────────────────────────────┘   │
└────────────────────────────────┬────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│  2. AGENT EXECUTION                                                 │
│                                                                     │
│  Two paths:                                                         │
│                                                                     │
│  PATH A: Direct Rig agent (non-sandboxed)                          │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ On each tool_call from LLM:                                  │   │
│  │   pipeline.set_task_context(task_id)                         │   │
│  │   pipeline.before_tool_call(tool_name, args_json)            │   │
│  │   → DLP scans tool arguments                                 │   │
│  │                                                              │   │
│  │ After tool executes (GuardedTool wrapper in engine.rs):      │   │
│  │   pipeline.after_tool_call(tool_name, result_json)           │   │
│  │   → DLP scans tool result for leaked secrets                 │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  PATH B: Container execution (sandboxed)                           │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ Container streams OutboundChunks back to host:               │   │
│  │                                                              │   │
│  │ ToolStart{name} →                                            │   │
│  │   pipeline.before_tool_call(name, {tool: name})              │   │
│  │   (post-hoc audit — tool already ran in container)           │   │
│  │                                                              │   │
│  │ ToolEnd{name, summary} →                                     │   │
│  │   pipeline.after_tool_call(name, {result: summary})          │   │
│  │   (DLP scans output crossing container→host boundary)        │   │
│  │                                                              │   │
│  │ TextBlock(text) →                                            │   │
│  │   pipeline.after_tool_call("llm_response", {text})           │   │
│  │   (DLP scans LLM response before it reaches user)           │   │
│  └──────────────────────────────────────────────────────────────┘   │
└────────────────────────────────┬────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│  3. PERSISTENCE (PgAuditSink)                                       │
│                                                                     │
│  INSERT INTO security_events (                                      │
│    task_id, user_id, event_type, tool_name,                         │
│    direction, threat_level, details, action_taken                   │
│  ) RETURNING id                                                     │
│                                                                     │
│  For each PendingFinding:                                           │
│  INSERT INTO dlp_findings (                                         │
│    security_event_id, finding_type, tag,                            │
│    pattern_name, confidence, redacted_preview,                      │
│    source_text, match_offset, match_length                          │
│  )                                                                  │
│                                                                     │
│  Configurable policies (dlp_policies table):                        │
│    block_private_keys  → action: block                              │
│    block_db_urls       → action: block                              │
│    warn_pii            → action: warn                               │
│    warn_api_keys       → action: warn                               │
│    block_injection     → action: block                              │
└────────────────────────────────┬────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│  4. UI (SecurityEventsPage + SecurityAuditTrail)                    │
│                                                                     │
│  GET /api/security/events → paginated security_events + findings    │
│  GET /api/security/patterns → list all 109 DLP patterns             │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Redaction Tokenization Dataflow (prevention layer)

The existing DLP pipeline **detects and logs** secrets. Redaction tokenization adds
a **prevention layer** upstream — secrets never enter LLM context at all.

```
                         DETECTION ONLY (current)
                         ═══════════════════════
User prompt ──► DlpHook scans ──► finds AKIA... ──► PgAuditSink logs
                                                     (secret already in LLM context)

                         PREVENTION + DETECTION (with tokenization)
                         ══════════════════════════════════════════
User prompt ──► RedactionTokenizer.tokenize()
                  replaces "AKIA..." → "{DLP:AWS_ACCESS_KEY:…MPLE}"
                  ──► DlpHook scans (clean — no raw secret)
                  ──► LLM sees only "{DLP:AWS_ACCESS_KEY:…MPLE}"
                  ──► Tool call uses "{DLP:AWS_ACCESS_KEY:…MPLE}"
                         │
                         ▼ host→container boundary
                  RedactionVault.resolve()
                  replaces "{DLP:AWS_ACCESS_KEY:…MPLE}" → "AKIAIOSFODNN7EXAMPLE"
                  writes to /run/secrets/AWS_ACCESS_KEY (tmpfs, 0400)
```

### Full Task Lifecycle with Tokenization

```
┌────────────────────────────────────────────────────────────────────────┐
│  TASK START                                                            │
│                                                                        │
│  1. Create RedactionVault for this task                                │
│  2. Ingest all secrets:                                                │
│     vault.ingest_secret_backend(&encrypted_file_backend)               │
│     vault.ingest_env_vars()         ← catches *_KEY, *_TOKEN, etc.    │
│     vault.ingest_dotenv(".env")     ← if .env file exists             │
│  3. Register OAuth/A2A tokens:                                         │
│     vault.register_auth_token("OAUTH_ACCESS_TOKEN", bearer_token)     │
│     vault.register_auth_token("A2A_AGENT_TOKEN", agent_token)         │
│  4. Wire vault into RedactionTokenizer → HookPipeline (position 0)    │
└────────────────────────────────┬───────────────────────────────────────┘
                                 │
                                 ▼
┌────────────────────────────────────────────────────────────────────────┐
│  USER PROMPT ARRIVES                                                   │
│  "charge card 4111111111114242 using key sk-ant-xxx..."               │
│                                                                        │
│  RedactionTokenizer (hook #0 in pipeline):                             │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │ 1. tokenize_secrets(): known secret "sk-ant-xxx..."              │  │
│  │    → "{SECRET:ANTHROPIC_API_KEY:…xxx}"                           │  │
│  │ 2. DLP pattern scan: credit card regex matches 4111...4242       │  │
│  │    → "{PII:CREDIT_CARD:…4242}"                                   │  │
│  │ 3. Both stored in vault: token → original value                  │  │
│  │ 4. Push redaction_applied findings to SecurityContext            │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                                                                        │
│  Tokenized: "charge card {PII:CREDIT_CARD:…4242} using key            │
│              {SECRET:ANTHROPIC_API_KEY:…xxx}"                          │
│                                                                        │
│  DlpHook (hook #1): scans tokenized text — clean, no raw secrets      │
│  SecretScannerHook (hook #2): no high-entropy tokens                   │
│  AuditHook (hook #4): logs redaction_applied findings                  │
└────────────────────────────────┬───────────────────────────────────────┘
                                 │
                                 ▼
┌────────────────────────────────────────────────────────────────────────┐
│  LLM PROCESSING                                                       │
│                                                                        │
│  LLM sees: "charge card {PII:CREDIT_CARD:…4242} using key             │
│             {SECRET:ANTHROPIC_API_KEY:…xxx}"                           │
│  LLM responds: "I'll charge the card ending in 4242"                  │
│  LLM generates: tool_call("stripe_charge",                            │
│                   {card: "{PII:CREDIT_CARD:…4242}"})                  │
└────────────────────────────────┬───────────────────────────────────────┘
                                 │
                                 ▼
┌────────────────────────────────────────────────────────────────────────┐
│  EXECUTION BOUNDARY (host → MCP / container)                           │
│                                                                        │
│  Before dispatching to AgentGateway or container:                      │
│  vault.resolve_all(tool_args)                                          │
│    "{PII:CREDIT_CARD:…4242}" → "4111111111114242"                     │
│                                                                        │
│  For containers:                                                       │
│    Secret values → /run/secrets/{KEY} (tmpfs, mode 0400)              │
│    OAuth tokens → /run/secrets/OAUTH_ACCESS_TOKEN                     │
│                                                                        │
│  For MCP calls via AgentGateway:                                       │
│    Token in args substituted with real value JIT                       │
│    HTTP Authorization header injected from vault                       │
└────────────────────────────────┬───────────────────────────────────────┘
                                 │
                                 ▼
┌────────────────────────────────────────────────────────────────────────┐
│  TASK END                                                              │
│                                                                        │
│  Drop RedactionVault → zeroize all original values in memory           │
└────────────────────────────────────────────────────────────────────────┘
```

---

## OAuth / A2A / AgentAuth Token Handling

OAuth tokens, agent-to-agent (A2A) tokens, and AgentAuth credentials use the
same `RedactionVault` mechanism. The LLM never sees the actual bearer token.

### OAuth Flow

```
┌────────────────────┐
│  OAuth Provider     │
│  (Google, GitHub)   │
└────────┬───────────┘
         │ access_token, refresh_token
         ▼
┌────────────────────────────────────────────────┐
│  vault.register_auth_token(                    │
│    "OAUTH_ACCESS_TOKEN",                       │
│    "ya29.a0ARrdaM...xyz"                       │
│  )                                             │
│  → {SECRET:OAUTH_ACCESS_TOKEN:…xyz}            │
│                                                │
│  vault.register_auth_token(                    │
│    "OAUTH_REFRESH_TOKEN",                      │
│    "1//0eXXXXXXXXXXXX"                         │
│  )                                             │
│  → {SECRET:OAUTH_REFRESH_TOKEN:…XXXX}          │
└────────────────────────────────────────────────┘
         │
         ▼ LLM context
  "Use {SECRET:OAUTH_ACCESS_TOKEN:…xyz} to access Google API"
         │
         ▼ tool_call("google_api", {auth: "{SECRET:OAUTH_ACCESS_TOKEN:…xyz}"})
         │
         ▼ execution boundary
  vault.resolve("{SECRET:OAUTH_ACCESS_TOKEN:…xyz}")
  → "ya29.a0ARrdaM...xyz"
  → Authorization: Bearer ya29.a0ARrdaM...xyz
```

### Token Refresh

When an OAuth token expires and is refreshed:

```
# Original token
vault.register_auth_token("OAUTH_ACCESS_TOKEN", "ya29...old_xyz")
→ {SECRET:OAUTH_ACCESS_TOKEN:…_xyz}

# After refresh — new value, new suffix
vault.register_auth_token("OAUTH_ACCESS_TOKEN", "ya29...new_abc")
→ {SECRET:OAUTH_ACCESS_TOKEN:…_abc}

# Both tokens remain valid in the vault until task end
# Old token still resolvable (for in-flight requests)
# New token used for new requests
```

### A2A (Agent-to-Agent) Auth

```
vault.register_auth_token("A2A_AGENT_TOKEN", "agt_sk_live_xxx...yyy")
→ {SECRET:A2A_AGENT_TOKEN:…yyy}

# Agent sees the token placeholder in tool descriptions
# Real value injected only when making inter-agent calls
```

---

## Secret Ingestion Sources

The vault ingests secrets from all available sources at task start:

```
┌──────────────────────────┐
│  SecretBackend            │  vault.ingest_secret_backend()
│  (EncryptedFileBackend)   │  Lists + fetches all stored secrets
└──────────┬───────────────┘
           │
┌──────────▼───────────────┐
│  Environment Variables    │  vault.ingest_env_vars()
│  Matches: *_KEY, *_TOKEN, │  Scans all env vars matching
│  *_SECRET, *_PASSWORD,    │  secret name patterns
│  DATABASE_URL, etc.       │
└──────────┬───────────────┘
           │
┌──────────▼───────────────┐
│  .env File                │  vault.ingest_dotenv(".env")
│  KEY=VALUE lines          │  Parses dotenv format, same
│  (comments + quotes ok)   │  pattern matching as env vars
└──────────┬───────────────┘
           │
┌──────────▼───────────────┐
│  OAuth / A2A Tokens       │  vault.register_auth_token()
│  Bearer tokens, refresh   │  Registered on-demand during
│  tokens, agent tokens     │  auth flows
└──────────────────────────┘
```

---

## Database Schema

```sql
-- security_events: every scan event (allowed, warned, blocked, redacted)
CREATE TABLE security_events (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT,
    user_id TEXT NOT NULL DEFAULT 'admin',
    event_type TEXT NOT NULL,        -- 'dlp_match', 'secret_detected', 'redaction_applied'
    tool_name TEXT,
    direction TEXT,                  -- 'inbound', 'outbound'
    threat_level TEXT,               -- 'safe', 'suspicious', 'dangerous', 'critical'
    details JSONB NOT NULL DEFAULT '{}',
    action_taken TEXT NOT NULL,      -- 'allowed', 'warned', 'blocked', 'redacted'
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- dlp_findings: individual tagged detections
CREATE TABLE dlp_findings (
    id BIGSERIAL PRIMARY KEY,
    security_event_id BIGINT REFERENCES security_events(id) ON DELETE CASCADE,
    finding_type TEXT NOT NULL,      -- 'dlp_match', 'secret_detected', 'redaction_applied'
    tag TEXT NOT NULL,               -- 'redaction:PII:CREDIT_CARD', 'entropy:high'
    pattern_name TEXT,
    confidence REAL,
    redacted_preview TEXT,           -- '{PII:CREDIT_CARD:…4242}' (safe to store)
    source_text TEXT,                -- excerpt with match context
    match_offset INTEGER,
    match_length INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

---

## Pipeline Hook Order

```
HookPipeline (shared SecurityContext per tool call)
│
├── #0 RedactionTokenizer     ← NEW: prevention (tokenizes before others see it)
│   └── Uses: RedactionVault (per-task)
│   └── Pushes: redaction_applied findings
│
├── #1 DlpHook                ← 109 regex patterns (detection on tokenized text)
│   └── Pushes: dlp_match findings
│
├── #2 SecretScannerHook      ← Shannon entropy (catches unknown secrets)
│   └── Pushes: secret_detected findings
│
├── #3 SecuritySidecarHook    ← External container (prompt injection)
│   └── Pushes: injection findings
│
└── #4 AuditHook              ← Always last: reads ALL findings, persists to Postgres
    └── Sinks: TracingAuditSink | FileAuditSink | PgAuditSink
```

---

## Key Files

| File | Purpose |
|------|---------|
| `crates/mcclawd-core/src/hooks/redaction_vault.rs` | Token↔value vault with suffix generation |
| `crates/mcclawd-core/src/hooks/secret_tokenizer.rs` | SecurityHook wrapper (tokenizes in pipeline) |
| `crates/mcclawd-core/src/hooks/dlp.rs` | 109 DLP regex patterns |
| `crates/mcclawd-core/src/hooks/secret_scanner.rs` | Shannon entropy detection |
| `crates/mcclawd-core/src/hooks/audit.rs` | Audit hook + PgAuditSink |
| `crates/mcclawd-core/src/hooks/pipeline.rs` | HookPipeline + SecurityContext |
| `crates/mcclawd-core/src/hooks/mod.rs` | Module exports |
| `crates/mcclawd-core/src/secrets/mod.rs` | SecretBackend trait |
| `crates/mcclawd-core/src/secrets/env.rs` | Env var secret backend |
| `crates/mcclawd-core/migrations/008_security_events.sql` | DB schema |
| `crates/mcclawd-core/migrations/010_dlp_finding_context.sql` | Source context columns |
