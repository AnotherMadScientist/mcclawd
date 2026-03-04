# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

McClawd v5 — a Rust agent platform that runs single agents or coordinated swarms, uses ClawHub skills and MCP tools, executes in Docker containers, with encrypted secrets and JWT identity. Built as a Cargo workspace with Rig as the LLM layer.

## Current State

Pre-implementation. The architecture is defined in `mcclawd-v5-architecture.md`. The Phase 0 implementation plan is at `docs/plans/2026-03-04-phase0-one-agent-completes-task.md`.

## Architecture (v5)

- **Design doc:** `mcclawd-v5-architecture.md` — the source of truth for all design decisions
- **Rig provides the agent loop** — no manual ReAct loop. Use Rig's `.prompt().max_turns(N)` with streaming
- **rmcp** for MCP client connections (stdio + SSE transports)
- **6 crates:** mcclawd-core, mcclawd-agent, mcclawd-tools, mcclawd-channels, mcclawd-tasks, mcclawd-api
- **Binary name:** `mc` (not `mcclawd`)
- **Workspace files:** SOUL.md, AGENTS.md, USER.md (OpenClaw compatible)
- **Secrets:** AES-256-GCM-SIV + argon2, never in LLM context, never in env vars

## Build Phases

- **Phase 0:** One agent completes a task (CLI only, no sandbox, no skills)
- **Phase 1:** Sandboxed + skills + daemon + web channel
- **Phase 2:** Swarms + Telegram + multi-channel
- **Phase 3:** Full channel ecosystem (Discord, Slack, WhatsApp, Email, Matrix)

## Build & Test (once scaffolded)

```bash
cargo build --release -p mcclawd-api    # build mc binary
cargo test --workspace                   # all tests
cargo test -p mcclawd-core               # single crate
cargo test -p mcclawd-core -- secrets    # filter by test name
```

## Run (once Phase 0 is implemented)

```bash
./target/release/mc workspace init
./target/release/mc secrets set ANTHROPIC_API_KEY
./target/release/mc run "your prompt"
```

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
