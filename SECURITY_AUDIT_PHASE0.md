# McClawd v5 Phase 0 -- Security Audit Report

**Date:** 2026-03-04
**Auditor:** Claude Opus 4.6 (Security Auditor)
**Scope:** Phase 0 codebase -- authentication, secrets vault, API endpoints, frontend client
**Commit:** Post v0.5.0 tag

---

## Executive Summary

The Phase 0 codebase has **2 critical, 4 high, 5 medium, and 4 low** severity findings. The most urgent issues are (1) the auth middleware exists but is NOT applied to any routes, leaving all "protected" endpoints fully unauthenticated, and (2) the login endpoint accepts any non-empty password, making authentication purely cosmetic. The encryption layer is well-implemented with proper nonce handling and memory zeroing, but is undermined by a hardcoded passphrase.

---

## CRITICAL

### C1. Auth Middleware Not Applied -- All "Protected" Routes Are Unauthenticated

**Files:**
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/auth.rs` (line 79-81)
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/routes.rs`

**Description:** The `auth_middleware` function is defined and fully implemented, but marked `#[allow(dead_code)]` with the comment "Available for route_layer integration in Phase 1." It is never applied to any route via `.route_layer()` or `.layer()`. The `protected` router in `routes.rs` has no middleware attached -- anyone can call `/api/secrets/{name}`, `/api/tasks`, `/api/workspace/{file}`, `/api/config`, etc. without any token.

**Attack Scenario:** Any network-reachable attacker can read, create, update, and delete all secrets (including API keys like `ANTHROPIC_API_KEY`), read/write workspace files, create agent tasks, and modify configuration -- all without authentication.

**Evidence:**
```rust
// routes.rs -- "protected" routes have NO middleware layer
let protected = Router::new()
    .route("/api/secrets", get(secrets::list_secrets).post(secrets::create_secret))
    // ... all routes ...
    .route_layer(/* NOTHING -- no auth layer applied */);
```

**Fix:**
```rust
use axum::middleware;

let protected = Router::new()
    // ... routes ...
    .route_layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware));
```

### C2. Login Accepts Any Non-Empty Password

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/auth.rs` (lines 34-40)

**Description:** The login handler only checks `if body.password.is_empty()`. Any non-empty string ("a", "x", "anything") returns a valid JWT. Combined with C1, this means even if the middleware were applied, authentication is still bypassed trivially.

**Attack Scenario:** Attacker sends `POST /api/auth/login` with `{"password": "x"}` and receives a valid 24-hour JWT token.

**Fix:** Validate the password against a stored hash (argon2 or bcrypt). For Phase 0 local-dev, at minimum validate against a configurable passphrase or the vault passphrase itself.

---

## HIGH

### H1. Hardcoded Vault Passphrase in Login Handler

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/auth.rs` (line 43)

**Description:** The secrets vault passphrase is hardcoded as `"mcclawd-local-dev"`. This string is compiled into the binary and visible via `strings` on the executable. Anyone with access to the binary can derive the vault encryption key.

**Attack Scenario:** Attacker obtains the binary (e.g., from a Docker image layer or shared build artifact), extracts the passphrase, and decrypts the secrets vault file directly.

**Fix:** Accept the passphrase from the login request (derive vault key from user password) or from an environment variable/keychain at startup. Never hardcode cryptographic passphrases.

### H2. Hardcoded Salt in Key Derivation

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/secrets/encrypted_file.rs` (line 120)

**Description:** `derive_key()` uses `let salt = b"mcclawd-secrets-v1"` -- a fixed, hardcoded salt. Combined with the hardcoded passphrase (H1), this means every McClawd installation derives the identical encryption key. The comment says "acceptable for local-only use" but the server binds to `0.0.0.0` (see M3).

**Attack Scenario:** Attacker who knows the passphrase (or reads the source) can decrypt any McClawd vault file from any installation.

**Fix:** Generate a random 16-byte salt on first vault creation, store it as the first 16 bytes of the vault file (before the nonce), and use it in argon2 derivation.

### H3. Path Traversal in Workspace File Endpoints

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/workspace.rs` (lines 36-37, 56-57)

**Description:** The `{file}` path parameter from `GET/PUT /api/workspace/{file}` is used directly in `workspace_dir.join(&file)` with no validation. While Axum does URL-decode the path, there is no check for `..`, absolute paths, or symlinks.

**Attack Scenario:** `GET /api/workspace/../../.ssh/id_rsa` or `PUT /api/workspace/../../.bashrc` could read/write arbitrary files on the server's filesystem, constrained only by process permissions.

**Fix:**
```rust
// Validate file parameter
if file.contains("..") || file.contains('/') || file.contains('\\') || file.starts_with('.') {
    return Err(StatusCode::BAD_REQUEST);
}
// Optionally: verify canonicalized path is within workspace_dir
let canonical = tokio::fs::canonicalize(&file_path).await?;
if !canonical.starts_with(&workspace_dir) { return Err(StatusCode::FORBIDDEN); }
```

### H4. Zero-Key Fallback in `new_empty()`

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/secrets/encrypted_file.rs` (line 35)

**Description:** `new_empty()` calls `derive_key(passphrase).unwrap_or([0u8; 32])`. If key derivation fails for any reason, the encryption key becomes 32 zero bytes -- a completely predictable key. Any secrets stored while in this state are encrypted with a known key.

**Attack Scenario:** If argon2 derivation fails (e.g., OOM on constrained system), secrets are encrypted with an all-zero key. Attacker who obtains the vault file can decrypt everything.

**Fix:** Propagate the error instead of falling back to a zero key. Make `new_empty()` return `Result<Self>`.

---

## MEDIUM

### M1. CORS Allows Any Origin

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/commands/serve.rs` (lines 16-20)

**Description:**
```rust
CorsLayer::new()
    .allow_origin(Any)
    .allow_methods(Any)
    .allow_headers(Any),
```
This allows any website to make authenticated cross-origin requests to the API. Combined with localStorage token storage (M2), a malicious page can exfiltrate all secrets.

**Attack Scenario:** User visits `evil.com` which runs JavaScript calling `fetch("http://localhost:9090/api/secrets/ANTHROPIC_API_KEY")` with the stored JWT -- CORS allows it, secrets are exfiltrated.

**Fix:** Restrict to `http://localhost:8080` for dev. Use environment-based CORS configuration for production.

### M2. JWT Token Stored in localStorage (XSS-Vulnerable)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/api/client.ts` (lines 5-7)

**Description:** The JWT is stored in `localStorage` under key `mcclawd_token`. Any XSS vulnerability in the React app or its dependencies allows token theft.

**Fix:** Use `httpOnly` + `Secure` + `SameSite=Strict` cookies for token storage. This also eliminates the CORS issue since cookies are not sent cross-origin with `SameSite=Strict`.

### M3. Server Binds to 0.0.0.0 (All Interfaces)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/commands/serve.rs` (line 23)

**Description:** `TcpListener::bind(format!("0.0.0.0:{port}"))` exposes the API to all network interfaces, not just localhost. For a local development tool managing encrypted secrets, this exposes the attack surface to the entire LAN (or beyond if port-forwarded).

**Fix:** Bind to `127.0.0.1` by default. Add a `--bind` flag for explicit opt-in to network exposure.

### M4. No Security Headers

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/commands/serve.rs`

**Description:** The Axum server does not set any security headers: no `Content-Security-Policy`, `X-Content-Type-Options`, `X-Frame-Options`, `Strict-Transport-Security`, or `Referrer-Policy`.

**Fix:** Add a middleware layer with security headers:
```rust
use tower_http::set_header::SetResponseHeaderLayer;
// X-Content-Type-Options: nosniff
// X-Frame-Options: DENY
// Content-Security-Policy: default-src 'self'
```

### M5. WebSocket Endpoint Has No Authentication

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/ws.rs` (lines 14-19)

**Description:** The WebSocket upgrade handler `task_stream` does not validate any token. Even if `auth_middleware` were applied to HTTP routes, WebSocket upgrades are handled before middleware in many frameworks. The handler accepts any connection and streams agent output.

**Attack Scenario:** Attacker connects to `ws://localhost:9090/api/tasks/{id}/stream` and receives all agent output including potentially sensitive data processed by the agent.

**Fix:** Validate JWT from query parameter or `Sec-WebSocket-Protocol` header during the upgrade handshake.

---

## LOW

### L1. JWT Secret Has Low Entropy

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/state.rs` (line 20)

**Description:** `jwt_secret: uuid::Uuid::new_v4().to_string()` generates a UUID v4 (122 bits of randomness) as the JWT signing key. While sufficient entropy, the key is regenerated on every server restart, invalidating all active sessions. Also, UUIDs have a predictable format that reduces effective entropy.

**Fix:** Use `rand::thread_rng().gen::<[u8; 32]>()` for 256-bit entropy. For session persistence across restarts, store the key in the secrets vault or a config file.

### L2. No Rate Limiting on Login Endpoint

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/auth.rs`

**Description:** No rate limiting on `POST /api/auth/login`. While the current "any password works" issue (C2) makes this moot, once real authentication is added, this enables brute-force attacks.

**Fix:** Add `tower::limit::RateLimitLayer` or use `governor` crate for per-IP rate limiting.

### L3. No Request Body Size Limits

**Files:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/commands/serve.rs`

**Description:** No `DefaultBodyLimit` is configured on the Axum server. An attacker can send arbitrarily large JSON payloads to any POST/PUT endpoint, potentially causing OOM.

**Fix:**
```rust
use axum::extract::DefaultBodyLimit;
app.layer(DefaultBodyLimit::max(1024 * 1024)) // 1MB
```

### L4. Secret Values Logged in Error Context

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/secrets/encrypted_file.rs`

**Description:** Error messages from `map_err` include the underlying error which could contain partial plaintext or key material in debug builds. The `tracing::error!` calls in `secrets.rs` API handlers log full error strings.

**Fix:** Use opaque error messages for cryptographic operations. Never log the error detail from cipher/argon2 operations to user-facing channels.

---

## Positive Findings

The audit also identified several well-implemented security practices:

1. **Nonce handling is correct**: `save_to_disk()` generates a fresh random 12-byte nonce via `rand::random()` on every write. No nonce reuse risk.
2. **AES-256-GCM-SIV chosen well**: Nonce-misuse resistant AEAD -- even if a nonce were reused, it would not be catastrophic (unlike plain GCM).
3. **Memory zeroing**: Key material uses `Zeroizing<[u8; 32]>` from the `zeroize` crate, preventing key persistence in freed memory.
4. **JWT uses Validation::default()**: The `jsonwebtoken` crate's default validation checks expiry and algorithm (HS256), preventing algorithm confusion attacks.
5. **Error responses are opaque**: API endpoints return `StatusCode` (e.g., 401, 500) without leaking internal details to the client.
6. **Secrets stored in-memory as HashMap**: Decrypted secrets remain in process memory only, not written to temp files.
7. **Type-safe deserialization**: Axum's `Json<T>` extractor with serde provides automatic input validation against expected schemas.

---

## Remediation Priority

| Priority | Finding | Effort | Impact |
|----------|---------|--------|--------|
| Immediate | C1 -- Apply auth middleware | 5 min | All endpoints exposed |
| Immediate | C2 -- Real password validation | 30 min | Auth is cosmetic |
| This sprint | H1 -- Remove hardcoded passphrase | 1 hr | Vault key is public |
| This sprint | H3 -- Path traversal validation | 30 min | Arbitrary file R/W |
| This sprint | H4 -- Remove zero-key fallback | 15 min | Predictable encryption |
| Next sprint | H2 -- Random salt per vault | 1 hr | All vaults share key |
| Next sprint | M1 -- Restrict CORS | 15 min | Cross-origin attacks |
| Next sprint | M2 -- httpOnly cookie tokens | 2 hr | XSS token theft |
| Next sprint | M3 -- Bind to 127.0.0.1 | 5 min | LAN exposure |
| Phase 1 | M4 -- Security headers | 30 min | Defense in depth |
| Phase 1 | M5 -- WebSocket auth | 1 hr | Unauthenticated streaming |
| Phase 1 | L1-L4 -- Hardening | 2 hr | Various |

---

## Methodology

Files audited:
- `crates/mcclawd-core/src/secrets/encrypted_file.rs` -- AES-256-GCM-SIV encryption, argon2 KDF
- `crates/mcclawd-core/src/secrets/mod.rs` -- SecretBackend trait
- `crates/mcclawd-core/src/identity/jwt.rs` -- JWT provider
- `crates/mcclawd-api/src/server/auth.rs` -- login + auth middleware
- `crates/mcclawd-api/src/server/routes.rs` -- route registration
- `crates/mcclawd-api/src/server/secrets.rs` -- secrets CRUD API
- `crates/mcclawd-api/src/server/workspace.rs` -- file read/write API
- `crates/mcclawd-api/src/server/tasks.rs` -- task creation API
- `crates/mcclawd-api/src/server/ws.rs` -- WebSocket streaming
- `crates/mcclawd-api/src/server/state.rs` -- AppState, JWT secret
- `crates/mcclawd-api/src/server/config_routes.rs` -- config API
- `crates/mcclawd-api/src/server/mcp_routes.rs` -- MCP servers API
- `crates/mcclawd-api/src/commands/serve.rs` -- server startup, CORS
- `crates/mcclawd-api/src/main.rs` -- CLI entrypoint
- `ui/packages/app/src/api/client.ts` -- frontend API client
- `ui/packages/app/vite.config.ts` -- Vite proxy config
- `docker-compose.yml` -- container configuration
- `Cargo.toml` + crate `Cargo.toml` files -- dependency versions
