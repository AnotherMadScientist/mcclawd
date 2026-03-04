# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

McClawd v5 — a Rust agent platform that runs single agents or coordinated swarms, uses ClawHub skills and MCP tools, executes in Docker containers, with encrypted secrets and JWT identity. Built as a Cargo workspace with Rig as the LLM layer.

## Current State

Phase 0 implemented. The binary `mc` supports `run`, `secrets`, and `workspace` commands.

## Architecture (v5)

- **Design doc:** `mcclawd-v5-architecture.md` — the source of truth for all design decisions
- **Rig provides the agent loop** — no manual ReAct loop. Use Rig's agent builder with `.tool()` and default_max_turns
- **rmcp** for MCP client connections (stdio + SSE transports)
- **6 crates:** mcclawd-core, mcclawd-agent, mcclawd-tools, mcclawd-channels, mcclawd-tasks, mcclawd-api
- **Binary name:** `mc` (not `mcclawd`)
- **Workspace files:** SOUL.md, AGENTS.md, USER.md (OpenClaw compatible)
- **Secrets:** AES-256-GCM-SIV + argon2, never in LLM context, never in env vars

## Build & Test

```bash
cargo build --release -p mcclawd-api    # build mc binary
cargo test --workspace                   # all tests
cargo test -p mcclawd-core               # single crate
cargo test -p mcclawd-core -- secrets    # filter by test name
```

## Run

```bash
./target/release/mc workspace init
./target/release/mc secrets set ANTHROPIC_API_KEY
./target/release/mc run "your prompt"
```

## UI Development

```bash
cd ui && pnpm install                   # install frontend deps
cd ui && pnpm dev                       # start Vite dev server (:8080)
cargo run -p mcclawd-api -- serve       # start Axum API server (:9090)
```

The Vite dev server proxies `/api` requests to the Axum backend.

### UI Tech Stack
- React 19 + TypeScript + Vite + Tailwind CSS + shadcn/ui
- Located in `ui/packages/app/`
- API client: `ui/packages/app/src/api/client.ts`
- Pages: `ui/packages/app/src/pages/`

## Docker (MCP Infrastructure)

```bash
docker compose build --no-cache          # build MCP server images
docker compose up -d                     # start AgentGateway + 3 MCP containers
docker compose logs -f agentgateway      # watch MCP tool discovery
docker compose down                      # stop everything
cargo test -p mcclawd-tools --test mcp_integration -- --ignored  # E2E test
```

Three core MCP servers run as separate Docker containers with supergateway (stdio→HTTP). AgentGateway (unmodified official image) routes tool calls to each container. The `mc` binary connects to AgentGateway at `http://localhost:3000` via rmcp 0.13 StreamableHttp.

MCP servers are config-driven (`McpConfig` in `mcclawd-core`). Phase 1+ adds `mc mcp add/remove` CLI and ClawHub registry.

## Crate Structure

- `mcclawd-core` — types, config, secrets (AES-256-GCM-SIV + argon2), identity (JWT), hooks
- `mcclawd-agent` — workspace loader, AGENTS.md parser, context assembly, Rig agent builder
- `mcclawd-tools` — builtin tools (memory.store/recall), MCP client stub
- `mcclawd-channels` — Channel trait, InboundPipeline, CLI adapter
- `mcclawd-tasks` — task lifecycle (Phase 0: single interactive)
- `mcclawd-api` — `mc` binary (CLI entrypoint with clap)

## Build Phases

- **Phase 0:** One agent completes a task (CLI only, no sandbox, no skills) ✓
- **Phase 1:** Sandboxed + skills + daemon + web channel
- **Phase 2:** Swarms + Telegram + multi-channel
- **Phase 3:** Full channel ecosystem (Discord, Slack, WhatsApp, Email, Matrix)

## Key Decisions

| Decision | Rationale |
|----------|-----------|
| Rig for LLM, not raw API | 20+ providers, built-in tool calling, streaming, agent loop |
| No manual ReAct loop | Rig's agent handles think/tool/observe natively |
| Workspace markdown files | OpenClaw compatibility, human+LLM friendly |
| AGENTS.md is structural | Parsed for skill assignments + swarm planning, not just context |
| Encrypted file secrets (Phase 0) | Vault/keychain backends added later via SecretBackend trait |
| JWT identity (Phase 0) | SPIFFE upgrade path for Phase 3+ |
| Channel trait with 5 transport patterns | Hides polling/WS/sidecar behind uniform async interface |
| Skills are SKILL.md (ClawHub format) | 100% OpenClaw ecosystem compatible |
| No native Rust plugin SDK (yet) | All external tools via MCP through AgentGateway |
