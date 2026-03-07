# McClawd Task Container Lifecycle — Complete Data Flow

## Overview
Task execution flows from API creation → Docker sandbox → agent execution → event persistence → WebSocket broadcast.

---

## 1. TASK CREATION FLOW

**Endpoint:** `POST /api/tasks` → `create_task()` in `tasks.rs:81-144`

```
Client POST /api/tasks {prompt, workspace, delay_start, tags}
    ↓
create_task():
  1. Sanitize prompt (strip injection patterns)
  2. Create TaskRecord in memory (TaskManager::start_task_with_tags)
  3. Broadcast channel created (AppState::create_task_stream)
  4. Persist to Postgres (pg_save_task)
  5. If !delay_start: Spawn tokio task → run_agent()
    ↓
Return: StatusCode::CREATED, TaskResponse {id, prompt, status, tags}
```

**Key Decision:** `delay_start=true` pauses agent execution until client uploads attachments and calls POST /api/tasks/{id}/message

---

## 2. AGENT EXECUTION DISPATCHER

**Function:** `run_agent()` in `tasks.rs:151-170`

```rust
async fn run_agent() {
    // TRY Docker sandbox first (production)
    if let Ok(orch) = SandboxOrchestrator::new() {
        if orch.health_check().await {
            run_agent_sandboxed(...).await
            return
        }
    }
    // FALLBACK: Host execution (dev only)
    tracing::warn!("Docker unavailable — falling back to host execution")
    run_agent_host(...).await
}
```

**Execution Paths:**
- **Sandboxed (Production):** Docker container created, agent runs inside, connects to host MCP servers
- **Host (Dev Only):** Agent runs in-process on developer machine, no Docker

---

## 3. DOCKER SANDBOX EXECUTION

**Function:** `run_agent_sandboxed()` in `tasks.rs:172-282`

```
1. Send UserMessage event to broadcast channel
2. Set status to "Building" (Postgres: UPDATE tasks SET status='Building')
3. Broadcast StatusIndicator::Processing
4. Retrieve config from AppState.config
5. SandboxOrchestrator::new() → creates Docker client
6. orchestrator.run_agent_task(MCPPayload) → container.rs:run_agent_task()
7. Stream events from container output via broadcast tx
8. On completion: status → "Complete", send Done event
9. Clean up resources (drop container)
```

**Container Creation:** `crates/mcclawd-api/src/sandbox/container.rs:run_agent_task()`

```
run_agent_task(payload) {
    1. Build Docker image (UVR binary in container)
    2. create_container(image_id, payload)
       - Volume mount: workspace files
       - Env vars: AGENT_TASK_ID, MCP_GATEWAY_URL, JWT_TOKEN, secrets
       - Working dir: /agent
    3. Container.start() → spawns agent subprocess
    4. Tail container logs → parse as OutboundChunk JSON
    5. Persist each chunk to Postgres (task_events table)
    6. Broadcast via tx.send()
    7. Container.wait() for completion
    8. Container.remove() cleanup
}
```

---

## 4. MCP CONNECTION FROM INSIDE CONTAINER

**File:** `crates/mcclawd-agent/src/mcp_integration.rs`

**Environment-Based Discovery:**

```
Inside container:
  env MCP_GATEWAY_URL = "http://host.docker.internal:3000" (or localhost on Docker Desktop)
  env AGENT_TASK_ID = task_id
  env JWT_TOKEN = JWT signed by server

mcp_integration::connect_from_env() {
    1. Read MCP_GATEWAY_URL from env
    2. Create StreamableHttp client → AgentGateway at :3000
    3. List available MCP servers
    4. Build Rig agent with .tool() for each MCP server tool
    5. Agent ready to execute tool calls
}
```

**Tool Discovery:** Agent queries AgentGateway for all available MCP tools before execution.

---

## 5. EVENT PERSISTENCE LAYER

**Database Schema:** `crates/mcclawd-core/migrations/003_tasks.sql`

```sql
-- Core task record
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    prompt TEXT NOT NULL,
    status TEXT (Running|Building|Complete|Restarting|Failed),
    error_message TEXT,
    created_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ
);

-- Event streaming — all OutboundChunk variants
CREATE TABLE task_events (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    chunk JSONB,  -- Serialized OutboundChunk enum
    created_at TIMESTAMPTZ
);

-- Conversation history — user + assistant messages
CREATE TABLE task_chat_history (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    role TEXT (user|assistant),
    content JSONB,
    seq INT,
    created_at TIMESTAMPTZ
);
```

**Event Persistence Flow:**

```
OutboundChunk generated in agent
    ↓
send_and_persist(&task_id, &tx, chunk) in state.rs
    ↓
Broadcast: tx.send(chunk)  ← WebSocket subscribers get immediate update
Persist:   pg_save_event(task_id, chunk)  ← INSERT into task_events
    ↓
Frontend retrieves history via GET /api/tasks/{id}/events
```

---

## 6. DATA FLOW DIAGRAM

```
┌─────────────────────────────────────────────────────────────┐
│ FRONTEND (React WebSocket)                                  │
│ - WebSocket /api/stream/:task_id                           │
│ - GET /api/tasks/:id/events (history on reconnect)         │
└────────────────┬────────────────────────────────────────────┘
                 │
                 ↓
    ┌────────────────────────────────┐
    │  AppState (broadcast channel)   │
    │  - tasks: TaskManager           │
    │  - pg: PgStore                  │
    │  - mcp_gateway: MCP client      │
    └────────┬──────────────────┬─────┘
             │                  │
      ┌──────▼─────────┐      ┌─▼──────────────────┐
      │ run_agent()    │      │ Postgres (event DB)│
      │ Docker check   │      │ - tasks            │
      │      │         │      │ - task_events      │
      │      ↓         │      │ - task_chat_hist   │
      │ ┌─────────────┐│      └────────────────────┘
      │ │ SandboxOrch ││
      │ │             ││
      │ │ ┌─────────┐ ││
      │ │ │Container││ ││
      │ │ │ (Docker)││ ││
      │ │ │         ││ ││
      │ │ │ Agent   ││ ││
      │ │ │ Process ││ ││
      │ │ └────┬────┘ ││
      │ │      │      ││
      │ │ ┌────▼────┐ ││
      │ │ │MCP Integ││ ││
      │ │ │ connect ││ ││
      │ │ │ to :3000││ ││
      │ │ └─────────┘ ││
      │ └─────────────┘│
      └───────────────┘
```

---

## 7. KEY INSIGHTS & GAPS

### What Works ✓
1. **Task Creation:** Atomic (TaskManager + Postgres in sync)
2. **Event Streaming:** Broadcast + persistence (dual-write pattern)
3. **Container Lifecycle:** Proper cleanup (container.remove after completion)
4. **MCP Discovery:** Environment-based, agent learns available tools at startup

### Potential Gaps ⚠️

| Gap | Severity | Impact |
|-----|----------|--------|
| **No container image caching** | Medium | Rebuild image per task (slow) |
| **MCP env vars passed as-is** | Medium | No validation before container creation |
| **Event replay incomplete** | Low | Frontend may miss early events if WS reconnects mid-stream |
| **No task resource limits** | High | Container can consume unbounded memory/CPU |
| **Secrets in container env** | High | Decrypted secrets in Docker env (should use volume mounts) |
| **No container crash recovery** | Medium | If container exits early, events lost |

---

## 8. CRITICAL CODE PATHS

| Flow | File | Lines | Entry |
|------|------|-------|-------|
| **Task Creation** | `tasks.rs` | 81-144 | POST /api/tasks |
| **Agent Dispatch** | `tasks.rs` | 151-170 | run_agent() |
| **Sandboxed Exec** | `tasks.rs` | 172-282 | run_agent_sandboxed() |
| **Container Lifecycle** | `container.rs` | TBD | run_agent_task() |
| **Event Persist** | `pg_store.rs` | TBD | pg_save_event() |
| **MCP Connect** | `mcp_integration.rs` | TBD | connect_from_env() |
| **State Management** | `state.rs` | TBD | AppState fields + methods |

---

## 9. VERIFICATION CHECKLIST

- [ ] Container always cleaned up (even on panic)
- [ ] Events atomically persisted before broadcast
- [ ] Postgres schema includes all OutboundChunk variants
- [ ] MCP URL correctly resolves inside Docker (host.docker.internal)
- [ ] Secrets never leak to container logs
- [ ] Task status transitions are idempotent
- [ ] WebSocket reconnect replays full history
- [ ] Container resource limits enforced

---

## 10. NEXT STEPS

1. **Read `container.rs`** — Understand exact Docker client calls
2. **Read `pg_store.rs`** — Verify event serialization roundtrip
3. **Read `state.rs`** — Full AppState field inventory
4. **Test container lifecycle** — Verify cleanup on success/failure/timeout
5. **Audit secrets handling** — Ensure no leaks to logs/env
