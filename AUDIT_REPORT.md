# McClawd v5 Phase 0 Code Quality Audit

**Date:** 2026-03-04
**Scope:** 6 Rust crates + React/TypeScript UI
**Auditor:** Claude Opus 4.6

---

## CRITICAL -- Runtime Failure Risk

### C1. Unwrap on production path in `create_task` handler
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/tasks.rs:64`
```rust
let task = mgr.get_task(&id).unwrap();
```
**Impact:** If the task was somehow removed between `start_task()` and `get_task()` (race condition with concurrent `delete_task`), this panics the Axum handler, killing the connection and potentially the server.
**Fix:** Replace with `.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?` or return a 500 error.

### C2. `new_empty` falls back to zero key on derive failure
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/secrets/encrypted_file.rs` (~line 27)
```rust
let key = derive_key(passphrase).unwrap_or([0u8; 32]);
```
**Impact:** If argon2 key derivation fails, the encryption key silently becomes all zeros. Any secrets encrypted with this key are trivially decryptable. This is a cryptographic integrity failure.
**Fix:** Propagate the error. Change `new_empty` to return `Result<Self>` and use `?` instead of `unwrap_or`.

### C3. `load_from_disk` uses `get_mut()` instead of async lock
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/secrets/encrypted_file.rs` (~line 53)
```rust
*self.cache.get_mut() = map;
```
**Impact:** `RwLock::get_mut()` requires `&mut self` and bypasses the async lock, which is correct in `new()` (exclusive ownership). However, if `load_from_disk` were ever called after construction (e.g., a reload feature), this would bypass the lock. Currently safe but fragile -- one call-site change away from a data race.
**Fix:** Either mark `load_from_disk` as requiring `&mut self` explicitly (it already does, but document the invariant) or switch to `.write().await` for defensive safety.

### C4. Hardcoded passphrase `"mcclawd-local-dev"` in production paths
**Files:**
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/auth.rs:48` (login handler)
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/commands/run.rs:13`
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/commands/secrets.rs:7`
**Impact:** The passphrase used to derive the AES-256 encryption key for the secrets vault is hardcoded to a constant string. The actual login password is accepted as "any non-empty string" but the vault is always opened with `"mcclawd-local-dev"`. This renders the encryption pointless in a deployed scenario.
**Fix:** For Phase 0 this is acknowledged, but add a compile-time `#[cfg(not(debug_assertions))]` guard that refuses to start with the hardcoded passphrase, forcing configuration of a real one.

### C5. Path traversal in workspace file endpoint
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/workspace.rs:35-48`
```rust
let file_path = workspace_dir.join(&file);
```
**Impact:** The `{file}` path parameter from the URL is joined directly with the workspace directory. A request like `GET /api/workspace/../../etc/passwd` could read arbitrary files. Similarly the `PUT` endpoint could write arbitrary files.
**Fix:** Validate that the resolved path is within the workspace directory:
```rust
let file_path = workspace_dir.join(&file);
if !file_path.starts_with(&workspace_dir) {
    return Err(StatusCode::BAD_REQUEST);
}
```

### C6. CORS allows any origin, any method, any header
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/commands/serve.rs:15-19`
```rust
CorsLayer::new()
    .allow_origin(Any)
    .allow_methods(Any)
    .allow_headers(Any),
```
**Impact:** Any website can make authenticated API calls to the McClawd server if a user has a valid token in their browser. Combined with C4 (trivial auth), this allows CSRF-like attacks from any origin.
**Fix:** Restrict to `http://localhost:8080` (the Vite dev server) and the configured deployment origin.

---

## IMPORTANT -- Tech Debt

### I1. `#[allow(dead_code)]` masking unused code
**Files:**
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/tasks.rs:17` -- `CreateTaskRequest` fields `workspace` and `model` are deserialized but the struct is marked `allow(dead_code)`. The `model` field is completely unused in `create_task()`.
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/auth.rs:80` -- `auth_middleware` is fully implemented but never wired into the router.
**Fix:** Remove `#[allow(dead_code)]`. For `auth_middleware`, either wire it into the router via `route_layer` or cfg-gate it for Phase 1. For `model`, either use it in agent construction or remove the field.

### I2. `put_config` is a stub that silently discards input
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/config_routes.rs:17-20`
```rust
pub async fn put_config(Json(body): Json<serde_json::Value>) -> StatusCode {
    tracing::info!("Config update requested: {body}");
    StatusCode::NO_CONTENT
}
```
**Impact:** The UI settings page may present a "save" button that appears to work (204 response) but actually persists nothing. Users will think their config changes were saved.
**Fix:** Either implement config persistence or return `StatusCode::NOT_IMPLEMENTED` (501) so the UI can show an appropriate message.

### I3. Auth middleware not applied to protected routes
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/routes.rs`
The `auth_middleware` exists in `auth.rs` but is never applied via `.route_layer()` on the protected router. All "protected" routes are actually publicly accessible with no JWT validation.
**Fix:** Add `.route_layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware))` to the protected router.

### I4. Inconsistent response types across API endpoints
**Files:** Various in `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/`
- `list_secrets` returns `Result<Json<Vec<SecretEntry>>, StatusCode>`
- `create_secret` returns `Result<StatusCode, StatusCode>`
- `create_task` returns `(StatusCode, Json<TaskResponse>)` (tuple, not Result)
- `list_files` returns `Json<Vec<WorkspaceFile>>` (no Result at all)
- `put_config` returns bare `StatusCode`
**Fix:** Standardize on `Result<Json<T>, StatusCode>` or create an `ApiError` type that implements `IntoResponse` with structured error bodies.

### I5. `TaskManager` uses `Vec` for O(n) lookups
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-tasks/src/manager.rs`
All lookups (`get_task`, `complete_task`, `fail_task`, `delete_task`) use `.iter().find()` or `.retain()` on a `Vec<TaskRecord>`. This is O(n) per operation.
**Fix:** Use `HashMap<TaskId, TaskRecord>` for O(1) lookups. Keep a separate `Vec<TaskId>` for ordering if needed.

### I6. `save_to_disk` uses blocking `std::fs::write` inside async context
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/secrets/encrypted_file.rs`
```rust
async fn save_to_disk(&self) -> Result<()> {
    // ...
    std::fs::write(&self.path, &output)?;
}
```
**Impact:** Blocking file I/O inside an async function blocks the tokio runtime thread. Under load, this can starve other async tasks.
**Fix:** Use `tokio::fs::write` instead of `std::fs::write`. Similarly, `load_from_disk` uses `std::fs::read` (though called only at construction, so less critical).

### I7. No WebSocket authentication
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/ws.rs`
The WebSocket upgrade handler `task_stream` does not validate a JWT token. Even if auth middleware were applied to the HTTP upgrade request (I3), the WS protocol itself carries no ongoing authentication.
**Fix:** Validate the token from either query params or the initial HTTP upgrade headers before upgrading.

### I8. Secrets revealed in plaintext via API without rate limiting
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/secrets.rs`
`GET /api/secrets/{name}` returns the full secret value in plaintext JSON. No rate limiting, no audit logging for reads, no differentiation between masked/unmasked responses.
**Fix:** Add rate limiting middleware, log secret access events, and consider requiring re-authentication for reveal.

### I9. `TaskDetailPage` uses non-null assertion on `id`
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/pages/TaskDetailPage.tsx`
```tsx
queryFn: () => api.tasks.get(id!),
```
**Impact:** The `id!` non-null assertion will throw at runtime if `id` is undefined, even though there is already a check via `enabled: !!id`.
**Fix:** Guard the function body or use a type guard to narrow `id` before the query call.

### I10. Secrets page stores revealed values in React state
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/pages/SecretsPage.tsx`
```tsx
const [revealed, setRevealed] = useState<Record<string, string>>({});
```
**Impact:** Revealed secret values persist in component state (and React DevTools) until the page is navigated away from. They could also appear in memory dumps.
**Fix:** Auto-clear revealed values after a timeout (e.g., 30 seconds). Consider using a ref instead of state to avoid React DevTools exposure.

---

## MINOR -- Style / Consistency

### M1. Missing `Default` derive on `TaskManager`
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-tasks/src/manager.rs`
`TaskManager::new()` is identical to what `Default` would generate. Add `#[derive(Default)]` for consistency.

### M2. Unnecessary `clone()` in `config_routes::get_config`
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/config_routes.rs:13`
```rust
Json(config.clone())
```
Clones the entire config struct on every GET request. Could use `Arc<McclawdConfig>` in `AppState` to make this zero-cost.

### M3. `McpConnection` fields are private but methods are needed
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-tools/src/mcp.rs`
The `McpConnection` struct has private fields accessed only through methods, which is good encapsulation. But `McpBundle` in `mcp_integration.rs` makes `tools` and `peer` pub, breaking the encapsulation.

### M4. `CliChannel::new()` should implement `Default`
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-channels/src/cli.rs`
`CliChannel` is a unit-like struct with `new() -> Self { Self }`. Should derive or implement `Default`.

### M5. `api.tasks.cancel` called but not verified to exist
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/pages/TaskDetailPage.tsx`
The cancel button calls `api.tasks.cancel(id)` but this method was not found in `client.ts`. If it exists, the endpoint is `DELETE /api/tasks/{id}` which actually fully deletes the task rather than cancelling it.

### M6. Duplicated `McclawdConfig::default()` instantiation pattern
**Files:**
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/commands/run.rs:9`
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/commands/secrets.rs:5`
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/commands/workspace.rs:5`

Each command creates `McclawdConfig::default()` independently. The `serve` command loads from file. This inconsistency means CLI commands ignore `config.toml` while the web server respects it.
**Fix:** All commands should load config the same way (file with fallback to default).

### M7. Empty `identity.rs` module declared but file missing
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/lib.rs:4`
```rust
pub mod identity;
```
But `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/identity.rs` does not exist (file read returned error). This would cause a compilation error, suggesting the file exists but may be empty, or the module is defined elsewhere.

### M8. `useTaskStream` accumulates events indefinitely
**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/hooks/useTaskStream.ts`
The `events` array grows without bound as stream events arrive. For long-running tasks with many tool calls, this could consume significant memory.
**Fix:** Cap the events array or implement virtualized rendering.

---

## Summary

| Severity | Count | Key Themes |
|----------|-------|------------|
| Critical | 6 | Unwrap panics, zero-key fallback, path traversal, hardcoded passphrase, open CORS |
| Important | 10 | Dead code masks, no auth enforcement, blocking I/O in async, inconsistent API patterns |
| Minor | 8 | Missing Default derives, unnecessary clones, missing module files, UI memory growth |

**Top 3 actions for Phase 1 readiness:**
1. Fix C5 (path traversal) and C6 (CORS) -- these are exploitable security issues
2. Wire I3 (auth middleware) into the router -- all "protected" routes are currently unprotected
3. Fix C2 (zero-key fallback) -- silent crypto failure is unacceptable
