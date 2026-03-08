# McClawd Security Review

**Date:** 2026-03-08
**Branch:** `agent_security`
**Reviewer:** Automated security audit (Claude Code)
**Scope:** Network isolation, process isolation, container boundaries, secret handling, attack surface

---

## Executive Summary

McClawd's agent security infrastructure provides **defense-in-depth** through a multi-layer HookPipeline (DLP + secret scanner + security sidecar + audit), WebAuthn authentication, encrypted vault, and prompt injection sanitization. This review identified **3 critical**, **2 high**, and **6 medium** risk findings, alongside strong existing mitigations.

---

## Architecture Under Review

```
Client (Browser)
  │ HTTPS/WSS
  ▼
McClawd API (:8081) ─── JWT auth ─── WebAuthn
  │
  ├── HookPipeline (in-process)
  │     ├── DlpHook (regex patterns)
  │     ├── SecretScannerHook (entropy)
  │     ├── SecuritySidecarHook → HTTP → security-sidecar (:8082)
  │     └── AuditHook → PostgreSQL
  │
  ├── AgentGateway (:3000) ── MCP tool containers
  │
  └── PostgreSQL (:5432) ── security_events, dlp_findings, dlp_policies
```

---

## Findings

### CRITICAL (3)

#### C1: Postgres Credentials Hardcoded in docker-compose.yml

**File:** `docker-compose.yml` lines 8-10
**Issue:** `POSTGRES_PASSWORD: mcclawd` in plaintext, same credentials in `DATABASE_URL`
**Risk:** Any developer or CI leak exposes full database access
**Recommendation:**
- Use Docker secrets (`docker secret create`) or `.env` file excluded from git
- Rotate credentials for any non-local deployment
- Add `docker-compose.yml` to secret scanning rules

#### C2: Database Port Exposed to All Interfaces

**File:** `docker-compose.yml` — `ports: "5432:5432"`
**Issue:** PostgreSQL bound to `0.0.0.0:5432`, accessible from any network interface
**Risk:** Remote database access with hardcoded credentials (combines with C1)
**Recommendation:**
- Bind to localhost only: `ports: ["127.0.0.1:5432:5432"]`
- Or remove host binding entirely — McClawd API can use Docker network

#### C3: API Keys Passed as Container Environment Variables

**File:** `crates/mcclawd-api/src/sandbox/container.rs` lines 315-325
**Issue:** `ANTHROPIC_API_KEY` and other secrets passed via `Env` in container config, visible in `docker inspect`
**Risk:** Any process with Docker socket access can read all API keys
**Recommendation:**
- Use Docker secrets mounted as files (`/run/secrets/`)
- Or use a short-lived token exchange (agent requests key via authenticated API call)
- Never pass long-lived secrets as environment variables

### HIGH (2)

#### H1: AgentGateway Exposed Without Authentication

**File:** `docker-compose.yml` — `ports: "3000:3000"`
**Issue:** AgentGateway listens on all interfaces with no visible auth layer
**Risk:** Any local process (or network neighbor) can invoke MCP tools directly, bypassing security pipeline
**Recommendation:**
- Bind to localhost only: `ports: ["127.0.0.1:3000:3000"]`
- Or remove host binding — agent containers access via Docker network
- Consider adding API key auth header to gateway requests

#### H2: Agent Containers Can Reach PostgreSQL

**Issue:** Agent containers join Docker bridge network, can resolve `postgres:5432`
**Risk:** Malicious agent (via prompt injection) could connect to database with hardcoded credentials
**Recommendation:**
- Create isolated network for agent containers (separate from `mcclawd-net`)
- Agent containers should ONLY reach AgentGateway, not postgres/sidecar
- Use Docker network policies or separate compose networks

### MEDIUM (6)

#### M1: Audit Logs Store Raw Tool Arguments

**File:** `crates/mcclawd-core/src/hooks/audit.rs` lines 156-178
**Issue:** `PgAuditSink.record()` stores `args_summary` in plaintext JSONB
**Risk:** Database breach exposes tool call arguments that may contain sensitive data
**Recommendation:** Hash or truncate args before storage; store only metadata

#### M2: Security Sidecar Receives Plaintext Data

**File:** `crates/mcclawd-core/src/hooks/security_sidecar.rs` lines 69-101
**Issue:** Full tool args/results sent via HTTP to sidecar for scanning
**Risk:** Network sniffing on Docker bridge could capture sensitive data
**Mitigation (existing):** Sidecar bound to localhost only (127.0.0.1:8082)
**Recommendation:** Add TLS for non-localhost deployments; consider pre-redacting known secret patterns before transmission

#### M3: No Container Capability Restrictions

**File:** `crates/mcclawd-api/src/sandbox/container.rs`
**Issue:** Agent containers created without `--cap-drop=all` or seccomp profiles
**Recommendation:** Add `CapDrop: ["all"]`, `SecurityOpt: ["no-new-privileges"]` to container config

#### M4: No Container Resource Limits

**Issue:** Agent containers have no memory/CPU limits
**Risk:** Runaway agent (infinite loop, memory leak) could DoS the host
**Recommendation:** Set `Memory: 512MB`, `CpuShares: 512` (configurable per workspace)

#### M5: DLP Policy Regex Not Validated

**File:** `crates/mcclawd-api/src/server/security.rs` (create_policy handler)
**Issue:** User-provided `tag_pattern` and `tool_pattern` stored without regex validation
**Risk:** ReDoS attack via malicious regex patterns
**Recommendation:** Validate with `regex::Regex::new()` before storing; reject invalid patterns

#### M6: No Inter-Service TLS

**Issue:** All internal communication (API↔sidecar, API↔gateway, API↔postgres) uses plaintext HTTP/TCP
**Risk:** Acceptable for localhost development, not for production deployment
**Recommendation:** Document as dev-only; add TLS for any multi-host deployment

---

## Positive Findings

### Strong Encryption (vault)
- AES-256-GCM-SIV authenticated encryption with argon2 KDF
- Random 12-byte nonce per encryption operation
- Atomic writes (temp + rename) prevent corruption
- File permissions: 0600 (owner-only)
- **File:** `crates/mcclawd-core/src/secrets/encrypted_file.rs`

### SQL Injection Prevention
- **ALL** database queries use `sqlx::query().bind()` parameterized queries
- Zero string interpolation in SQL across 100+ queries in `pg_store.rs`
- Compile-time query validation via sqlx

### WebAuthn Authentication
- Passwordless, phishing-resistant authentication
- Challenge/response with device-bound credentials
- JWT tokens with 24h expiration, HMAC-SHA256
- JWT secret stored with 0600 file permissions
- **File:** `crates/mcclawd-api/src/server/webauthn_auth.rs`

### Prompt Injection Defense
- 33 injection patterns blocked (13 phrase + 6 marker patterns)
- Word-boundary matching prevents false positives
- Applied at API entry points before LLM context
- **File:** `crates/mcclawd-core/src/sanitizer.rs`

### DLP & Secret Scanning
- DLP hook reports pattern_name and context, not raw secret values
- Secret scanner truncates previews (first 6 + last 4 chars only)
- Fail-fast before_tool_call blocks dangerous content before execution
- Shannon entropy threshold (4.5 bits, min 20 chars) catches novel secrets
- **File:** `crates/mcclawd-core/src/hooks/dlp.rs`, `secret_scanner.rs`

### Security Sidecar Isolation
- Runs as non-root `security` user
- Bound to localhost only (`127.0.0.1:8082`)
- Fail-open pattern: sidecar unavailability doesn't block tool calls (warns only)
- **File:** `docker/security-sidecar/Dockerfile`, `entrypoint.py`

### Agent Container Isolation
- Runs as non-root `agent` user
- Attachments mounted read-only
- Separate container per task
- **File:** `docker/agent-runner/Dockerfile`

### Route Protection
- All security API endpoints behind JWT auth middleware
- Public routes limited to: health check, auth flow endpoints
- **File:** `crates/mcclawd-api/src/server/routes.rs`

---

## Recommendations (Priority Order)

| Priority | Finding | Fix |
|----------|---------|-----|
| P0 | C1+C2: Postgres exposed | Localhost-only binding, externalize credentials |
| P0 | C3: API keys as env vars | Docker secrets or token exchange |
| P1 | H1: Gateway no auth | Localhost-only binding |
| P1 | H2: Agent→Postgres access | Separate Docker networks |
| P2 | M3: No cap-drop | Add `--cap-drop=all` to container creation |
| P2 | M4: No resource limits | Add memory/CPU limits |
| P2 | M5: Regex validation | Validate patterns before storage |
| P3 | M1: Raw args in audit | Hash/truncate sensitive args |
| P3 | M2: Plaintext to sidecar | TLS for non-localhost |
| P3 | M6: No inter-service TLS | Document dev-only, add for production |

---

## Scope Limitations

This review covers the `agent_security` branch implementation. Not in scope:
- Production deployment hardening (reverse proxy, WAF, rate limiting)
- Dependency vulnerability scanning (cargo-audit, npm audit)
- Penetration testing of the security sidecar Python code
- Performance impact of security pipeline on tool call latency
- Invariant flow policy effectiveness testing
