# Phase 1 Design: Sandboxed Execution + Skills + Daemon + Web Channel

**Date:** 2026-03-04
**Status:** Approved

## Overview

Phase 1 extends McClawd from a single CLI agent (Phase 0) to a full daemon-based platform with sandboxed Docker execution, ClawHub skills, and web channel streaming.

## Architecture

### Unified Daemon (`mc serve`)

Single process runs: Axum API + WebSocket + Agent Supervisor + Sandbox Orchestrator.

- `mc serve` starts the daemon on :9090
- `mc run "prompt"` becomes a thin client — POSTs to daemon API, subscribes to WebSocket for streaming
- Falls back to in-process execution if daemon isn't running (dev mode)

### Data Flow

```
Channel (CLI/Web) → InboundMessage → InboundPipeline → TaskManager → Agent Supervisor
                                                                          │
                    ┌─────────────────────────────────────────────────────┘
                    ▼
              1. Resolve skills (AGENTS.md → SKILL.md files)
              2. Build sandbox image (base + skill layers, cached)
              3. Create container (bollard): workspace bind, tmpfs secrets, network
              4. Start agent in container (Rig + rmcp → AgentGateway)
              5. Stream OutboundChunks back through channel
              6. Monitor lifecycle (crash → restart with backoff 1s→2s→4s→8s)
              7. Cleanup container on completion
```

### Task State Machine

```
Pending → Building → Running → Complete
                       │          ▲
                       └─ crash → Restarting (backoff) ─┘
                                  max retries → Failed
```

## Component Details

### 1. Channel System

Phase 0 has CLI channel. Phase 1 adds Web channel.

**InboundMessage:** `{ channel_id, peer_id, agent_id, content, attachments }`

**Session dedup:** same (agent, channel, peer) = same session.

**Web Channel:**
- `POST /api/tasks` — create task from InboundMessage, returns task_id
- `GET /ws?task_id=xxx` — WebSocket stream of OutboundChunks (per-token JSON)
- `GET /api/tasks/:id` — poll task status + history
- JWT auth on all endpoints

**CLI Channel update:**
- `mc run` POSTs to daemon, streams via WebSocket
- Fallback to in-process if daemon unavailable

### 2. Agent Supervisor

Central orchestrator tying skills, sandbox, and task lifecycle.

**Responsibilities:**
- Spawn/restart agents with exponential backoff
- Manage sandbox creation/teardown via bollard
- Route OutboundChunks from agent back through channels
- Track running agents with max concurrency limit
- PID file management for daemon

### 3. Sandbox Orchestrator (bollard)

Agents run inside Docker sibling containers.

**Container setup:**
- Bind mounts: `/workspace` (host workspace), `/var/run/docker.sock` (if needed)
- tmpfs: `/run/secrets/` with decrypted secret files (never env vars, never disk)
- Network: `mcclawd_default` — can reach AgentGateway at `agentgateway:3000`
- Environment: `MCCLAWD_AGENT_ID`, `MCCLAWD_TASK_ID`, `MCCLAWD_MCP_URL` (no secret values)

**Image strategy (layered):**
- Base image: `mcclawd-sandbox` (Python, Node, common tools)
- Per-skill layers: each skill's install_steps → cached Docker layer
- Per-agent image built from base + skill layers, cached for reuse

**MCP access:**
- Agent in container connects to AgentGateway via rmcp StreamableHTTP
- AgentGateway runs as a service on the Docker network
- Skills filter which tools are visible (tool name prefix matching)

### 4. Skills System (ClawHub format)

**SKILL.md format:**
```markdown
# Skill: <name>
version: <semver>
author: <author>

## Description
<description text>

## MCP Tools
- <tool_name_1>
- <tool_name_2>

## Install
```bash
<install commands>
```

## Context
<context injected into agent preamble>
```

**LoadedSkill struct:**
```rust
LoadedSkill {
    name: String,
    version: String,
    description: String,
    mcp_tools: Vec<String>,       // tool name prefixes for filtering
    install_steps: Vec<String>,    // shell commands for Docker layer
    context: String,               // injected into agent preamble
}
```

**Lifecycle:**
1. Discovery: `.mcclawd/skills/<name>/SKILL.md`
2. Resolution: `SkillLoader.resolve_for_agent(agent_id)` reads AGENTS.md skill list
3. Install: skill install_steps become Docker layers (cached)
4. Context: skill context blocks injected into agent preamble
5. Tool filtering: skill mcp_tools filter AgentGateway tools per agent

**CLI commands:**
- `mc skills list` — show installed skills
- `mc skills install <name>` — install from local or registry
- `mc skills info <name>` — show SKILL.md contents

### 5. Secret Injection

- Supervisor decrypts secrets from encrypted store
- Writes to tmpfs mount at `/run/secrets/<KEY_NAME>`
- Agent reads secret values from files
- tmpfs cleaned up on container stop — never persisted to disk
- AgentGateway env vars (e.g. GOOGLE_API_KEY) set in docker-compose, not per-agent

## Crate Changes

| Crate | Phase 1 additions |
|-------|-------------------|
| mcclawd-core | SkillLoader, LoadedSkill, SandboxConfig, SKILL.md parser |
| mcclawd-agent | Skill-based tool filtering, preamble context injection |
| mcclawd-tools | (unchanged — MCP client already works) |
| mcclawd-channels | WebChannel (Axum WS), CLI channel update (daemon client mode) |
| mcclawd-tasks | Task state machine (Building/Running/Restarting/Complete/Failed) |
| mcclawd-api | `mc serve` daemon command, supervisor, sandbox orchestrator (bollard) |

## Dependencies

- `bollard` — Docker API client for Rust
- `axum` (already present) — WebSocket support via `axum::extract::ws`
- No new MCP dependencies — rmcp 0.13 + AgentGateway unchanged

## Success Criteria

1. `mc serve` starts daemon, `mc run "prompt"` submits to daemon and streams results
2. Agent runs inside Docker container with workspace + secrets mounted
3. Skills parsed from SKILL.md, installed as Docker layers, filter MCP tools
4. WebSocket streaming works from browser UI
5. Crash recovery: agent restart with exponential backoff
6. All existing tests continue to pass
