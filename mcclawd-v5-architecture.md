# McClawd v5: Agent-First Architecture

**Date:** 2026-03-03
**Approach:** ZeroClaw's Rust trait architecture + OpenClaw ecosystem compatibility + containerized execution

---

## 1. What We're Building

A Rust agent platform that:

- Runs single agents or coordinated swarms to complete tasks
- Uses ClawHub skills and MCP tools natively (100% OpenClaw compatible)
- Executes agent code in Docker containers (separate from app containers)
- Supports both interactive (streaming) and background (headless) tasks
- SOUL.md agent identity/personality from day one
- Proper secrets management (encrypted at rest, scoped delivery, never in LLM context)
- JWT identity with upgrade path to SPIFFE
- Daemon supervisor mode for self-healing

What we're NOT building (yet): DLP pipeline, Graphiti memory, SPIRE identity, taint tracking, native Rust plugin SDK, channel adapters (Slack/Telegram/etc). Those layer on later.

**Key simplification:** No native Rust plugin SDK in this version. All external tools come via MCP through AgentGateway. Skills are SKILL.md markdown files (ClawHub format). If you need a custom tool, write an MCP server in any language, containerize it, register it with the gateway. The native SDK is a Phase 4+ concern — it's a lot of API surface to design before we know what agents actually need.

---

## 2. Rig's Role — And Its Limits

**Rig is the LLM plumbing layer. Not the agent platform.**

What Rig gives us (and why we use it):

- **20+ provider implementations** behind `CompletionModel` trait (Anthropic, OpenAI, Ollama, Groq, Gemini, Cohere, etc.)
- **Tool calling with `#[tool]` derive macro** — auto-generates JSON schema, converts to provider-specific format
- **ToolServer** — Tokio-spawned task with message passing, shared across agents via `ToolServerHandle`
- **MCP client via rmcp** — official Rust MCP SDK, plugs into ToolServer
- **Streaming** — per-token callbacks for interactive mode
- **RAG** — `VectorStoreIndex` trait with 10+ backends
- **OpenTelemetry** — GenAI Semantic Convention tracing out of the box

What Rig does NOT provide (we build these):

| Missing | Why It Matters | Our Solution |
|---------|---------------|--------------|
| Agent loop (ReAct/planning) | Rig calls LLM once; no autonomous reasoning | `mcclawd-agent` crate with ReAct + planner |
| Swarm orchestration | No multi-agent coordination | `mcclawd-swarm` with DAG scheduler |
| Container sandbox | Tools run in-process, no isolation | Docker sibling containers via socket proxy |
| ClawHub/skill loading | No awareness of SKILL.md format | `mcclawd-skills` parser + loader |
| Session management | No concept of persistent sessions | `mcclawd-session` with SQLite/Postgres |
| Security hooks | No DLP, no tool policy, no audit | Trait-based middleware on tool dispatch boundaries |
| Task management | No background tasks, no queue | `mcclawd-tasks` with interactive + headless modes |

**Comparison to OpenClaw derivatives:**

| | Rig | ZeroClaw | OpenClaw | McClawd (this) |
|---|---|---|---|---|
| Language | Rust lib | Rust binary | Node.js monorepo | Rust workspace |
| LLM layer | 20+ providers | 22+ providers (traits) | Anthropic-first | **Rig** (20+ providers) |
| Agent loop | ❌ None | Basic loop | Full ReAct + subagents | **ReAct + planner/worker** |
| Multi-agent | ❌ | ❌ | Emerging | **DAG-based swarms** |
| MCP | rmcp client | ❌ | Native (@mcp/sdk 1.25.3) | **rmcp via AgentGateway** |
| ClawHub skills | ❌ | ❌ | Native (SKILL.md) | **Full compat parser** |
| Container sandbox | ❌ | ❌ (app-level allowlists) | Apple Container/Docker | **Docker sibling containers** |
| Security | ❌ | Encrypted secrets, allowlists | Soft guardrails (broken) | **Trait hooks, future DLP** |
| Deployment | Embed in your app | Single 3.4MB binary | Docker + node_modules | **Single binary + compose** |

---

## 3. Architecture

```
                    ┌──────────────────────────────────────┐
                    │      Channel Adapters (data plane)    │
                    │                                      │
                    │  CLI  Web/WS  Telegram  Discord      │
                    │  Email  WhatsApp  Signal  Matrix ... │
                    │  (each implements Channel trait)      │
                    │  (Transport A-E hidden inside)        │
                    └──────────────┬───────────────────────┘
                                   │ InboundMessage (normalized)
                                   ▼
┌──────────────────────────────────────────────────────────────────────┐
│  mc binary (Rust, axum)                                              │
│                                                                      │
│  ┌──────────────────────────────────────────────────────┐           │
│  │           Inbound Pipeline                            │           │
│  │  normalize → dedup → access → route → debounce →     │           │
│  │  dispatch                                             │           │
│  └──────────────────────┬───────────────────────────────┘           │
│                          │                                           │
│  ┌────────────┐  ┌──────┴──────────────────────────────┐           │
│  │ REST API   │  │         Task Manager                 │           │
│  │ (control   │─→│  Interactive ←→ Background           │           │
│  │  plane)    │  │  Sessions keyed (agent,channel,peer) │           │
│  │ POST /api/ │  └────────────────┬────────────────────┘           │
│  │ tasks      │                    │                                │
│  └────────────┘       ┌────────────┴───────────┐                   │
│                        ▼                        ▼                   │
│              ┌─────────────────┐   ┌──────────────────┐            │
│              │  Agent Engine    │   │  Swarm Engine     │            │
│              │  ReAct loop      │   │  Planner → DAG   │            │
│              │  Workspace files │   │  → workers        │            │
│              │  Per-agent skills│   │  → shared memory  │            │
│              └───────┬──────────┘   └────────┬──────────┘            │
│                      └───────────┬───────────┘                       │
│                                  ▼                                   │
│  ┌──────────────────────────────────────────────────────┐           │
│  │         Tool Dispatch (trait SecurityHook)             │           │
│  │  Builtins │ Skills (SKILL.md) │ MCP (AgentGateway)    │           │
│  └──────────────────────┬───────────────────────────────┘           │
│                          │                                           │
│  ┌───────────────────────┴──────────────────────────────┐           │
│  │  Provider Pool (Rig)  │  Secrets  │  Identity         │           │
│  └───────────────────────────────────────────────────────┘           │
└──────────────────────────────────────────────────────────────────────┘
      │              │                 │
      │ Docker       │ MCP/HTTP        │ OutboundChunk (streaming)
      ▼              ▼                 ▼
┌───────────┐ ┌──────────────┐ ┌───────────────────────────┐
│ Sandbox   │ │ AgentGateway │ │ Channel Adapters (outbound)│
│ Containers│ │ MCP servers  │ │ Stream → chunk → send      │
│ per-task  │ │ behind RBAC  │ │ (edit msg, SMTP, WS, etc.) │
└───────────┘ └──────────────┘ └───────────────────────────┘
```

Two entry points: **Channels** (data plane — messaging platforms) feed through the InboundPipeline with dedup, access control, routing, and session management. **REST API** (control plane — programmatic) submits tasks directly to the TaskManager. Both produce OutboundChunks that stream back through the originating channel.

---

## 4. Crate Structure

```
mcclawd/
├── Cargo.toml                          # workspace
├── crates/
│   ├── mcclawd-core/                   # shared types, config, error, security
│   │   ├── src/
│   │   │   ├── config.rs               # TOML config → typed structs
│   │   │   ├── types.rs                # TaskId, AgentId, SessionId, etc.
│   │   │   ├── error.rs                # thiserror error types
│   │   │   ├── hooks.rs               # SecurityHook, AuditHook traits
│   │   │   ├── secrets/
│   │   │   │   ├── mod.rs             # SecretBackend trait + Secret<T> type
│   │   │   │   ├── encrypted_file.rs  # AES-256-GCM-SIV + argon2 (Phase 0)
│   │   │   │   ├── vault.rs           # HashiCorp Vault KV v2 (Phase 2+)
│   │   │   │   └── keychain.rs        # OS keychain via `keyring` crate (Phase 1+)
│   │   │   └── identity/
│   │   │       ├── mod.rs             # AgentIdentity trait
│   │   │       └── jwt.rs            # JWT-based identity (upgrade to SPIFFE later)
│   │   └── Cargo.toml
│   │
│   ├── mcclawd-agent/                  # agent engine
│   │   ├── src/
│   │   │   ├── engine.rs               # ReAct loop
│   │   │   ├── planner.rs              # planning agent (decomposes tasks)
│   │   │   ├── context.rs              # context window assembly (workspace files + skills)
│   │   │   ├── workspace.rs            # workspace loader (SOUL.md + AGENTS.md + USER.md)
│   │   │   ├── agents_parser.rs       # AGENTS.md parser → AgentSpec + skill assignments
│   │   │   ├── memory.rs               # working memory (HashMap, future: taint)
│   │   │   └── session.rs              # session state
│   │   └── Cargo.toml
│   │
│   ├── mcclawd-swarm/                  # multi-agent orchestration
│   │   ├── src/
│   │   │   ├── orchestrator.rs         # DAG scheduler
│   │   │   ├── worker.rs               # worker agent wrapper
│   │   │   ├── dag.rs                  # task dependency graph
│   │   │   └── shared_memory.rs        # Arc<DashMap> shared state
│   │   └── Cargo.toml
│   │
│   ├── mcclawd-tools/                  # tool dispatch + implementations
│   │   ├── src/
│   │   │   ├── registry.rs             # ToolRegistry (dispatch layer)
│   │   │   ├── builtin/
│   │   │   │   ├── memory.rs           # memory.store, memory.recall
│   │   │   │   ├── task.rs             # task.spawn, task.status
│   │   │   │   └── fs.rs              # read_file, write_file (in sandbox)
│   │   │   ├── mcp.rs                  # MCP client via rmcp → AgentGateway
│   │   │   ├── skills.rs              # ClawHub SKILL.md parser + loader
│   │   │   └── sandbox.rs             # Docker container code execution
│   │   └── Cargo.toml
│   │
│   ├── mcclawd-tasks/                  # task lifecycle management
│   │   ├── src/
│   │   │   ├── manager.rs              # TaskManager (interactive + background)
│   │   │   ├── interactive.rs          # streaming via WS
│   │   │   └── background.rs           # headless execution
│   │   └── Cargo.toml
│   │
│   ├── mcclawd-channels/              # channel framework + pipeline
│   │   ├── src/
│   │   │   ├── lib.rs                 # InboundMessage, OutboundChunk, ChannelKind
│   │   │   ├── traits.rs             # Channel trait + ChannelStartContext + ConnectionModel
│   │   │   ├── router.rs             # Binding-based routing → agents
│   │   │   ├── session.rs            # SessionKey, SessionManager
│   │   │   ├── pipeline.rs           # 6-stage inbound pipeline
│   │   │   ├── access.rs             # DmPolicy, GroupPolicy, AccessController
│   │   │   ├── chunker.rs            # Per-channel streaming/chunking
│   │   │   ├── dedup.rs              # LRU dedup cache
│   │   │   ├── debounce.rs           # Per-channel debounce
│   │   │   ├── state.rs              # Encrypted channel state persistence (save/restore)
│   │   │   ├── cli.rs                # CLI channel (Phase 0, built-in)
│   │   │   └── compat.rs             # OpenClaw channel config import
│   │   └── Cargo.toml
│   │
│   ├── mcclawd-channel-web/           # Web/WS channel (Phase 1)
│   ├── mcclawd-channel-telegram/      # Telegram channel (Phase 2)
│   ├── mcclawd-channel-discord/       # Discord channel (Phase 3)
│   ├── mcclawd-channel-slack/         # Slack channel (Phase 3)
│   ├── mcclawd-channel-whatsapp/      # WhatsApp channel (Phase 3+, Baileys sidecar)
│   ├── mcclawd-channel-signal/        # Signal channel (Phase 3+, signal-cli sidecar)
│   ├── mcclawd-channel-email/         # Email IMAP+SMTP channel (Phase 3+)
│   ├── mcclawd-channel-matrix/        # Matrix channel (Phase 3+)
│   │
│   └── mcclawd-api/                    # control plane + daemon (NOT the data plane)
│       ├── src/
│       │   ├── main.rs                 # binary entrypoint + CLI (mc run, mc start, etc.)
│       │   ├── daemon.rs               # daemon supervisor (fork, monitor, restart)
│       │   ├── server.rs               # axum router (control plane REST API)
│       │   ├── channels.rs             # channel lifecycle orchestrator (start, state, shutdown)
│       │   ├── routes/
│       │   │   ├── tasks.rs            # POST /api/tasks, GET /api/tasks/:id (control plane)
│       │   │   ├── agents.rs           # agent config CRUD
│       │   │   ├── skills.rs           # skill install/list/search
│       │   │   ├── channels.rs         # GET /api/channels (status, health)
│       │   │   └── health.rs           # GET /health
│       └── Cargo.toml
│
├── config/
│   ├── mcclawd.toml                    # main config
│   ├── agentgateway.yaml               # AgentGateway config
│   └── secrets.enc                     # encrypted secrets (Phase 0 default backend)
│
├── workspaces/                         # agent workspace directories (OpenClaw compat)
│   └── default/                        # default agent workspace
│       ├── SOUL.md                     # personality, rules, identity
│       ├── AGENTS.md                   # team awareness, delegation rules
│       └── USER.md                     # user context, preferences
│
├── skills/                             # local skill directory (ClawHub format)
│   └── .gitkeep
│
└── docker-compose.yml
```

---

## 5. OpenClaw Compatibility Layer

### 5a. ClawHub Skill Format (SKILL.md)

OpenClaw skills are markdown files with YAML frontmatter. The format is simple:

```markdown
---
name: todoist-cli
description: Manage Todoist tasks from the command line.
version: 1.2.0
metadata:
  openclaw:
    requires:
      env:
        - TODOIST_API_KEY
      bins:
        - curl
    primaryEnv: TODOIST_API_KEY
    emoji: "✅"
    install:
      - kind: brew
        formula: todoist-cli
        bins: [todoist]
---

# Todoist CLI

## Usage Instructions
When the user requests task management, use the todoist CLI...

## Rules
- Never delete tasks without explicit confirmation
- Always show task ID in responses
```

Our parser handles:

1. **Frontmatter extraction** — YAML between `---` markers → `SkillManifest` struct
2. **Requirement resolution** — check `requires.bins` exist (in sandbox container), `requires.env` are set
3. **Install orchestration** — `install` specs (brew, node, go, uv) run in sandbox setup phase
4. **Context injection** — skill body (markdown below frontmatter) injected into agent context when skill matches task
5. **Aliases** — `metadata.clawdbot`, `metadata.clawdis`, `metadata.openclaw` all accepted (per ClawHub spec)

```rust
// crates/mcclawd-tools/src/skills.rs

#[derive(Debug, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub tags: Option<Vec<String>>,
    pub metadata: Option<SkillMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct SkillMetadata {
    // Accept all three aliases
    #[serde(alias = "clawdbot", alias = "clawdis")]
    pub openclaw: Option<OpenClawMeta>,
}

#[derive(Debug, Deserialize)]
pub struct OpenClawMeta {
    pub requires: Option<SkillRequirements>,
    pub install: Option<Vec<InstallSpec>>,
    pub emoji: Option<String>,
    #[serde(rename = "primaryEnv")]
    pub primary_env: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SkillRequirements {
    pub env: Option<Vec<String>>,
    pub bins: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
pub enum InstallSpec {
    #[serde(rename = "brew")]
    Brew { formula: String, bins: Vec<String> },
    #[serde(rename = "node")]
    Node { package: String, bins: Vec<String> },
    #[serde(rename = "go")]
    Go { module: String, bins: Vec<String> },
    #[serde(rename = "uv")]
    Uv { package: String, bins: Vec<String> },
}
```

### 5b. ClawHub CLI Compatibility

We implement a `clawhub` subcommand that wraps ClawHub's REST API:

```
mc skills search "calendar management"
mc skills install <slug>
mc skills install <slug> --version 1.2.3
mc skills list
mc skills update --all
mc skills uninstall <slug>
mc skills inspect <slug>
```

Skills install to `~/.mcclawd/skills/` (managed) or `<workspace>/skills/` (per-agent). Same precedence as OpenClaw: workspace > managed > bundled.

### 5c. Workspace CLI

```
mc workspace init [name]           # scaffold new workspace with template SOUL/AGENTS/USER
mc workspace list                  # list all workspaces
mc workspace show [name]           # show workspace files and status
mc workspace edit <name> <file>    # open SOUL.md / AGENTS.md / USER.md in $EDITOR
mc workspace copy <src> <dst>      # clone a workspace (for new agent variant)
```

`mc workspace init coding` creates:
```
~/.mcclawd/workspaces/coding/
├── SOUL.md      # template with placeholders
├── AGENTS.md    # template with default agent list
└── USER.md      # template (or copies from default workspace if exists)
```

### 5d. MCP Server Configuration (OpenClaw-compatible)

OpenClaw configures MCP servers in `openclaw.json` or `.mcp.json`. We accept both formats:

```json
{
  "mcpServers": {
    "notion": {
      "command": "npx",
      "args": ["-y", "@notionhq/mcp"],
      "env": { "NOTION_TOKEN": "${NOTION_TOKEN}" }
    },
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-fs", "/allowed/path"]
    },
    "github": {
      "type": "sse",
      "url": "https://mcp.github.com/sse"
    }
  }
}
```

**How it maps internally:**

- **stdio servers** (`command` + `args`) → spawned as Docker containers behind AgentGateway. The MCP process runs inside a sandbox container, not on the host.
- **SSE/HTTP servers** (`type: "sse"`, `url`) → registered as remote targets in AgentGateway config.
- **Environment variables** → resolved from vault (future) or env, injected into container only — never into agent context.

This is the key security improvement over OpenClaw: MCP servers run in containers, not as bare processes on the host.

---

## 6. Agent Engine

### 6a. ReAct Loop

```rust
// crates/mcclawd-agent/src/engine.rs

pub struct AgentEngine {
    provider: Arc<dyn CompletionModel>,  // Rig provider
    tools: ToolRegistry,
    max_iterations: usize,
    hooks: Vec<Arc<dyn SecurityHook>>,   // future DLP, audit, etc.
}

impl AgentEngine {
    pub async fn run(&self, task: &Task, session: &mut Session) -> Result<AgentResult> {
        let mut iterations = 0;

        loop {
            if iterations >= self.max_iterations {
                return Ok(AgentResult::max_iterations(session.summary()));
            }

            // 1. Assemble context
            let context = self.build_context(task, session).await?;

            // 2. Call LLM (via Rig)
            let response = self.provider
                .completion(context)
                .await?;

            // 3. Parse response
            match self.parse_response(&response) {
                AgentAction::ToolCall(call) => {
                    // Run pre-hooks (future: DLP scan on tool input)
                    for hook in &self.hooks {
                        hook.before_tool_call(&call).await?;
                    }

                    // Dispatch tool
                    let result = self.tools.dispatch(&call, session).await?;

                    // Run post-hooks (future: DLP scan on tool output)
                    for hook in &self.hooks {
                        hook.after_tool_call(&call, &result).await?;
                    }

                    session.add_observation(call, result);
                }
                AgentAction::FinalAnswer(answer) => {
                    return Ok(AgentResult::completed(answer));
                }
                AgentAction::Thinking(thought) => {
                    session.add_thought(thought);
                }
            }

            iterations += 1;
        }
    }
}
```

### 6b. Workspace Files (OpenClaw-Compatible)

Every agent has a **workspace directory** containing markdown files that define its identity, team awareness, and user context. This is the same model OpenClaw uses — the trio of `SOUL.md`, `AGENTS.md`, and `USER.md` in a workspace folder.

```
~/.mcclawd/
├── skills/                         # managed skills (shared across all agents)
│   ├── git-workflow/
│   │   └── SKILL.md
│   ├── code-review/
│   │   └── SKILL.md
│   └── academic-search/
│       └── SKILL.md
│
├── workspaces/
│   ├── default/                    # default agent workspace
│   │   ├── SOUL.md                 # personality, rules, identity
│   │   ├── AGENTS.md               # team awareness, delegation rules, skill assignments
│   │   ├── USER.md                 # info about the human being served
│   │   ├── skills/                 # per-agent skills (override managed)
│   │   │   └── custom-tool/
│   │   │       └── SKILL.md
│   │   └── notes/                  # agent working notes (persisted across sessions)
│   ├── coding/
│   │   ├── SOUL.md
│   │   ├── AGENTS.md
│   │   ├── USER.md                 # can symlink to default/USER.md
│   │   └── skills/                 # coding-specific skills
│   │       └── rust-analyzer/
│   │           └── SKILL.md
│   └── research/
│       ├── SOUL.md
│       ├── AGENTS.md
│       └── USER.md
│
├── state/                          # encrypted channel connection state (§10)
│   ├── telegram/
│   │   └── main.enc               # update_id cursor
│   ├── discord/
│   │   └── main.enc               # session_id + seq + resume_url
│   ├── email/
│   │   └── personal.enc           # IMAP UID validity + last seen UID
│   └── matrix/
│       └── home.enc               # since sync token
```

**Skill resolution order** (per agent):
1. **Workspace skills** — `<workspace>/skills/` (highest priority, agent-specific overrides)
2. **Managed skills** — `~/.mcclawd/skills/` (installed via `mc skills install`)
3. **Bundled skills** — `./skills/` (shipped with binary)

Same precedence as OpenClaw: workspace > managed > bundled.

**SOUL.md** — Agent personality, communication style, ethical rules, persistent identity. Loaded first in every reasoning cycle — this is who the agent *is*.

```markdown
# Soul

You are McClawd, a security-focused AI assistant.

## Personality
- Direct and technical. Skip pleasantries when the user is in flow.
- When uncertain, say so. Never fabricate tool output.
- Prefer showing code over describing code.

## Rules
- Never execute destructive operations (rm -rf, DROP TABLE) without explicit confirmation.
- Always explain security implications of suggested changes.
- Refuse to store secrets in plaintext, even if asked.

## Identity
- Name: McClawd
- Emoji: 🦞
- Theme: Security-first engineering
```

**AGENTS.md** — Team awareness. Tells this agent who else exists, what they specialize in, which skills they use, and when to delegate. In Phase 0 this is informational (single agent reads it but can't delegate yet). In Phase 2 (swarms), the planner reads AGENTS.md to decide which workers to spawn and which skills each worker gets.

```markdown
# Agents

## Default Skills
Skills loaded for ALL agents unless overridden:
- memory-management
- task-status

## Available Agents

### coding
- **Specialty:** Code generation, debugging, refactoring
- **Model:** claude-sonnet-4-5 (fast, good at code)
- **Tools:** exec, read, write, edit, mcp:github
- **Skills:**
  - git-workflow
  - code-review
  - rust-analyzer
  - typescript-lint
- **Delegate when:** User asks for code changes, debugging, PR reviews

### research
- **Specialty:** Deep research, analysis, report writing
- **Model:** claude-opus-4-5 (thorough reasoning)
- **Tools:** web_search, read, mcp:notion
- **Skills:**
  - academic-search
  - competitor-analysis
  - report-writing
- **Delegate when:** User asks for research, comparisons, analysis docs

### scout
- **Specialty:** Monitoring, alerts, periodic checks
- **Model:** claude-haiku-4-5 (fast, cheap)
- **Tools:** web_search, read
- **Skills:**
  - news-monitor
  - dependency-audit
- **Delegate when:** Background monitoring tasks, status checks

## Delegation Rules
- Always confirm with user before delegating to another agent
- The coding agent should never have web_search (attack surface)
- Research tasks over 3 sub-questions should use swarm mode
- Skills listed under an agent are loaded into that agent's context only
```

**USER.md** — Context about the human. Preferences, background, recurring projects. This replaces the "user profile" pattern — it's a markdown file the user can edit directly, not an opaque database.

```markdown
# User

## Identity
- Name: Steve
- Role: Software engineer / entrepreneur

## Preferences
- Languages: Rust (primary), Python, TypeScript
- Python frameworks: FastAPI, Flask
- CLI style: rich, colorama, tqdm for color/panels/progress
- Code organization: distinct modules and services
- When fixing: focus on the specific module before refactoring the whole app

## Current Projects
- McClawd: security-first AI agent framework (Rust)
- Heimdall: graph-native platform
- IronBox: secure code execution engine
- gCoder: graph-native AI coding platform (startup)

## Working Context
- Hardware: MacBook Pro M2, 96GB RAM
- Prioritizes: local execution, privacy, security-first design
```

### 6c. Workspace Loader

```rust
// crates/mcclawd-agent/src/workspace.rs

use std::path::PathBuf;

/// The three core workspace files, loaded as raw markdown.
pub struct Workspace {
    pub soul: Option<String>,
    pub agents: Option<String>,
    pub user: Option<String>,
    pub path: PathBuf,
}

pub struct WorkspaceLoader {
    /// Search paths in priority order: explicit → agent-specific → default → bundled
    search_paths: Vec<PathBuf>,
}

impl WorkspaceLoader {
    pub fn new(config: &WorkspaceConfig) -> Self {
        let mut paths = vec![];

        // Explicit workspace path from agent config (highest priority)
        // Agent-specific: ~/.mcclawd/workspaces/<agent_id>/
        // Default: ~/.mcclawd/workspaces/default/
        // Bundled: ./workspaces/ (shipped with binary)
        paths.push(config.managed_dir.join("default"));

        Self { search_paths: paths }
    }

    /// Load workspace for a named agent. Falls back through search paths.
    pub fn load(&self, agent_id: &str) -> Workspace {
        for base in &self.search_paths {
            let agent_dir = base.parent().unwrap().join(agent_id);
            let dir = if agent_dir.exists() { &agent_dir } else { base };

            let soul = Self::read_file(dir, "SOUL.md");
            // Only return if we found at least SOUL.md
            if soul.is_some() {
                return Workspace {
                    soul,
                    agents: Self::read_file(dir, "AGENTS.md"),
                    user: Self::read_file(dir, "USER.md"),
                    path: dir.to_path_buf(),
                };
            }
        }

        // Empty workspace (no files found)
        Workspace {
            soul: None,
            agents: None,
            user: None,
            path: PathBuf::new(),
        }
    }

    fn read_file(dir: &std::path::Path, name: &str) -> Option<String> {
        let path = dir.join(name);
        std::fs::read_to_string(path).ok()
    }
}
```

### 6d. AGENTS.md Parser & Skill Resolution

AGENTS.md is both a context document (injected into the LLM as markdown) AND a structural definition (parsed to drive skill loading and swarm planning). The parser extracts agent specs from the markdown without requiring a separate config format.

```rust
// crates/mcclawd-agent/src/agents_parser.rs

/// Parsed from AGENTS.md markdown. Extracted via lightweight markdown parsing
/// (heading + bullet pattern matching), not a full AST.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub id: String,                    // heading name (e.g. "coding")
    pub specialty: Option<String>,
    pub model: Option<String>,
    pub tools: Vec<String>,            // ["exec", "read", "mcp:github"]
    pub skills: Vec<String>,           // ["git-workflow", "code-review"]
    pub delegate_when: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentsConfig {
    pub default_skills: Vec<String>,   // skills loaded for ALL agents
    pub agents: Vec<AgentSpec>,
    pub delegation_rules: Vec<String>, // free-text rules (injected as context)
    pub raw_markdown: String,          // original AGENTS.md (for LLM context injection)
}

impl AgentsConfig {
    /// Parse AGENTS.md markdown into structured agent specs.
    /// Extracts: agent IDs from ### headings, skills/tools from bullet lists,
    /// default_skills from ## Default Skills section.
    pub fn parse(markdown: &str) -> Self {
        // Lightweight parser: scan headings + bullet patterns
        // ### <agent_id> → new AgentSpec
        // - **Skills:** → parse sub-bullets as skill names
        // - **Tools:** → parse as tool names
        // ## Default Skills → parse bullets as default skill names
        // ## Delegation Rules → capture bullets as rules
        todo!()
    }

    /// Get skills for a specific agent (default_skills + agent-specific).
    pub fn skills_for(&self, agent_id: &str) -> Vec<String> {
        let mut skills = self.default_skills.clone();
        if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
            skills.extend(agent.skills.clone());
        }
        skills.dedup();
        skills
    }

    /// Get the spec for the current agent (or None for default/unspecified).
    pub fn agent_spec(&self, agent_id: &str) -> Option<&AgentSpec> {
        self.agents.iter().find(|a| a.id == agent_id)
    }
}
```

**Skill loading per agent:**

```rust
// crates/mcclawd-tools/src/skills.rs (extended)

pub struct SkillResolver {
    /// Per-agent workspace skills dir
    workspace_skills: PathBuf,       // <workspace>/skills/
    /// Shared managed skills
    managed_skills: PathBuf,         // ~/.mcclawd/skills/
    /// Bundled skills
    bundled_skills: PathBuf,         // ./skills/
}

impl SkillResolver {
    /// Resolve and load skills for a specific agent based on AGENTS.md config.
    /// Returns skill manifests + bodies ready for context injection.
    pub fn resolve_for_agent(
        &self,
        agent_id: &str,
        agents_config: &AgentsConfig,
    ) -> Vec<LoadedSkill> {
        let skill_names = agents_config.skills_for(agent_id);
        let mut loaded = vec![];

        for name in &skill_names {
            // Search order: workspace > managed > bundled
            if let Some(skill) = self.find_skill(name) {
                loaded.push(skill);
            } else {
                tracing::warn!(skill = %name, agent = %agent_id, "Skill not found");
            }
        }

        loaded
    }

    fn find_skill(&self, name: &str) -> Option<LoadedSkill> {
        for base in [&self.workspace_skills, &self.managed_skills, &self.bundled_skills] {
            let skill_dir = base.join(name);
            let skill_md = skill_dir.join("SKILL.md");
            if skill_md.exists() {
                let content = std::fs::read_to_string(&skill_md).ok()?;
                let manifest = parse_skill_frontmatter(&content)?;
                let body = extract_skill_body(&content);
                return Some(LoadedSkill { manifest, body, path: skill_md });
            }
        }
        None
    }
}

pub struct LoadedSkill {
    pub manifest: SkillManifest,
    pub body: String,           // markdown body (instructions for the LLM)
    pub path: PathBuf,
}
```

**Swarm skill composition (Phase 2):**

When the planner decomposes a task into a DAG of workers, it reads AGENTS.md to determine:
1. Which agent type to use for each subtask (based on specialty + delegate_when)
2. Which skills that agent gets (from the agent's skills list + default skills)
3. Which model to use (from agent spec)
4. Which tools to allow (from agent spec)

```rust
// crates/mcclawd-swarm/src/orchestrator.rs (extended)

impl SwarmOrchestrator {
    fn build_worker_engine(&self, spec: &WorkerSpec) -> AgentEngine {
        let agents_config = AgentsConfig::parse(
            &self.workspace.agents.as_deref().unwrap_or("")
        );

        // Get the agent spec from AGENTS.md
        let agent_spec = agents_config.agent_spec(&spec.agent_type);

        // Resolve skills for this worker based on AGENTS.md
        let skills = self.skill_resolver.resolve_for_agent(
            &spec.agent_type,
            &agents_config,
        );

        // Load the worker's workspace (may have its own SOUL.md)
        let workspace = self.workspace_loader.load(&spec.agent_type);

        // Build engine with agent-specific skills, tools, model
        AgentEngine::builder()
            .workspace(workspace)
            .skills(skills)
            .tools(self.filter_tools(&agent_spec))
            .provider(self.select_provider(&agent_spec))
            .hooks(self.hooks.clone())
            .build()
    }
}
```

This means a single swarm can have workers with completely different skill sets: the coding worker loads `git-workflow` + `code-review` + `rust-analyzer`, while the research worker loads `academic-search` + `report-writing`. Each worker only sees the skills assigned to it in AGENTS.md.

### 6d. Context Assembly

Priority-ordered context window injection:

1. **SOUL.md** — agent personality, rules, identity (always present, always first)
2. **USER.md** — user context, preferences, background
3. **AGENTS.md** — team awareness, delegation rules (informs tool selection + swarm planning)
4. **System prompt** — generated capabilities summary (available tools, active skills)
5. **Active skills** — matched SKILL.md bodies (just-in-time, like OpenClaw)
6. **Working memory** — key-value pairs from current session
7. **Conversation history** — recent turns (sliding window)
8. **Task description** — the current task

```rust
// crates/mcclawd-agent/src/context.rs

pub struct ContextBuilder {
    workspace: Workspace,
    agents_config: AgentsConfig,       // parsed from AGENTS.md
    agent_id: String,                  // which agent we're building context for
    loaded_skills: Vec<LoadedSkill>,   // pre-resolved for this agent
    memory: HashMap<String, Value>,
    history: Vec<Turn>,
    max_tokens: usize,
}

impl ContextBuilder {
    pub fn build(&self, task: &str) -> Context {
        let mut sections = vec![];

        // 1. SOUL.md (always first — this is who the agent IS)
        if let Some(soul) = &self.workspace.soul {
            sections.push(ContextSection::soul(soul));
        }

        // 2. USER.md (user context — shapes HOW the agent responds)
        if let Some(user) = &self.workspace.user {
            sections.push(ContextSection::user(user));
        }

        // 3. AGENTS.md (team awareness — shapes WHAT the agent can delegate)
        if let Some(agents) = &self.workspace.agents {
            sections.push(ContextSection::agents(agents));
        }

        // 4. System prompt (generated: tool list, capabilities)
        sections.push(ContextSection::system(&self.build_system_prompt()));

        // 5. Active skills — resolved per-agent from AGENTS.md skill assignments.
        //    All assigned skills have manifests in context (name + description).
        //    Full SKILL.md body injected just-in-time when task matches.
        for skill in self.matched_skills(task) {
            sections.push(ContextSection::skill(&skill.manifest.name, &skill.body));
        }

        // 6. Skill manifest index (names + descriptions of ALL assigned skills,
        //    so the LLM knows what's available even if not yet loaded)
        let skill_index = self.loaded_skills.iter()
            .map(|s| format!("- **{}**: {}", s.manifest.name, s.manifest.description))
            .collect::<Vec<_>>()
            .join("\n");
        if !skill_index.is_empty() {
            sections.push(ContextSection::skill_index(&skill_index));
        }

        // 7. Working memory
        if !self.memory.is_empty() {
            sections.push(ContextSection::memory(&self.memory));
        }

        // 8. Conversation history (sliding window, fits within budget)
        let history_budget = self.max_tokens - sections.iter().map(|s| s.est_tokens()).sum::<usize>();
        sections.push(ContextSection::history(&self.history, history_budget));

        // 9. Task
        sections.push(ContextSection::task(task));

        Context { sections }
    }

    /// Match skills to the current task. Uses the pre-resolved skill list
    /// (already filtered per-agent from AGENTS.md), then scores relevance.
    fn matched_skills(&self, task: &str) -> Vec<&LoadedSkill> {
        // Phase 0: simple keyword matching against skill tags + description
        // Phase 2+: embedding-based relevance scoring
        self.loaded_skills.iter()
            .filter(|s| self.skill_matches_task(s, task))
            .collect()
    }
}
```

**OpenClaw compatibility notes:**
- Same file names (SOUL.md, AGENTS.md, USER.md) in same relative positions
- Same load semantics: workspace-specific → default → bundled
- SOUL.md loaded before everything else, same as OpenClaw
- AGENTS.md is informational in single-agent mode, structural in swarm mode
- USER.md is user-editable (it's just a markdown file, not a locked config)

Skills are loaded just-in-time: when the LLM determines a skill is relevant, it reads the SKILL.md body. The skill manifest (name + description) is always in context so the LLM knows what's available.

---

## 7. Swarm Engine

```rust
// crates/mcclawd-swarm/src/orchestrator.rs

pub struct SwarmOrchestrator {
    planner: AgentEngine,        // planning agent
    provider: Arc<dyn CompletionModel>,
    tools: ToolRegistry,
    max_workers: usize,
    shared_memory: Arc<DashMap<String, Value>>,
}

impl SwarmOrchestrator {
    pub async fn run(&self, task: &Task) -> Result<SwarmResult> {
        // 1. Planner decomposes task into DAG
        let dag = self.plan(task).await?;

        // 2. Execute DAG with bounded concurrency
        let mut completed: HashMap<WorkerId, WorkerResult> = HashMap::new();
        let semaphore = Arc::new(Semaphore::new(self.max_workers));

        for wave in dag.topological_waves() {
            let mut handles = vec![];

            for worker_spec in wave {
                let permit = semaphore.clone().acquire_owned().await?;
                let engine = self.build_worker_engine(&worker_spec);
                let deps = self.gather_deps(&worker_spec, &completed);
                let mem = self.shared_memory.clone();

                handles.push(tokio::spawn(async move {
                    let result = engine.run_with_deps(deps, mem).await;
                    drop(permit);
                    (worker_spec.id, result)
                }));
            }

            for handle in handles {
                let (id, result) = handle.await??;
                completed.insert(id, result);
            }
        }

        // 3. Aggregation (planner synthesizes results)
        self.aggregate(task, &completed).await
    }
}
```

**DAG example:** "Research top 5 competitors and write a comparison report"

```
plan → [research_1, research_2, ..., research_5] → synthesize → report
        (parallel, max 3 concurrent)                (depends on all 5)
```

Each worker is an `AgentEngine` instance with its own session but shared read/write access to `shared_memory`.

---

## 8. Container Sandbox

Every task gets its own Docker container. Agent code (tool execution, MCP server processes, skill binaries) runs inside it.

```rust
// crates/mcclawd-tools/src/sandbox.rs

pub struct SandboxConfig {
    pub image: String,              // "mcclawd/sandbox:latest"
    pub timeout: Duration,          // max task duration
    pub memory_limit: String,       // "512m"
    pub cpu_limit: f64,             // 1.0 = one core
    pub network_mode: NetworkMode,  // None | EgressOnly(allowlist) | Full
    pub volumes: Vec<VolumeMount>,  // filesystem resources
    pub env: HashMap<String, String>,
}

pub struct SandboxManager {
    docker: DockerClient,           // via socket proxy
}

impl SandboxManager {
    /// Spawn a sibling container for a task
    pub async fn create(&self, task_id: &TaskId, config: SandboxConfig) -> Result<Sandbox> {
        let container = self.docker.create_container(
            &format!("mcclawd-sandbox-{}", task_id),
            ContainerConfig {
                image: config.image,
                memory: config.memory_limit,
                cpu_quota: (config.cpu_limit * 100000.0) as i64,
                network_mode: match config.network_mode {
                    NetworkMode::None => "none",
                    NetworkMode::EgressOnly(_) => "mcclawd-egress",
                    NetworkMode::Full => "bridge",
                },
                volumes: config.volumes,
                env: config.env,
                labels: [("mcclawd.task", task_id.to_string())],
                ..Default::default()
            }
        ).await?;

        // Start with timeout watchdog
        self.docker.start_container(&container.id).await?;
        let watchdog = self.spawn_watchdog(container.id.clone(), config.timeout);

        Ok(Sandbox { container, watchdog })
    }

    /// Execute a command inside the sandbox
    pub async fn exec(&self, sandbox: &Sandbox, cmd: &[&str]) -> Result<ExecResult> {
        self.docker.exec_in_container(&sandbox.container.id, cmd).await
    }
}
```

**Resource attachment:** Volumes mount into the container at known paths:

```toml
# mcclawd.toml
[sandbox]
image = "mcclawd/sandbox:latest"
timeout = "10m"
memory = "512m"
cpu = 1.0

[[sandbox.volumes]]
source = "/data/projects"
target = "/workspace"
readonly = false

[[sandbox.volumes]]
source = "/data/reference"
target = "/reference"
readonly = true
```

MCP stdio servers also run inside sandbox containers. The AgentGateway connects to them via their exposed ports.

---

## 9. Task Manager

Handles multiple concurrent tasks — both interactive (human in the loop, streaming) and background (fire and forget).

```rust
// crates/mcclawd-tasks/src/manager.rs

pub struct TaskManager {
    tasks: Arc<DashMap<TaskId, TaskHandle>>,
    agent_factory: AgentFactory,
    swarm_factory: SwarmFactory,
    sandbox_manager: SandboxManager,
}

pub struct TaskHandle {
    pub id: TaskId,
    pub mode: TaskMode,
    pub status: watch::Receiver<TaskStatus>,
    pub stream: Option<broadcast::Sender<OutboundChunk>>,  // for interactive
    pub cancel: CancellationToken,
    pub join: JoinHandle<Result<TaskResult>>,
}

pub enum TaskMode {
    Interactive,    // streams OutboundChunks back to originating channel
    Background,     // headless, poll for status via REST
}

impl TaskManager {
    pub async fn spawn_task(&self, request: TaskRequest) -> Result<TaskId> {
        let id = TaskId::new();

        // 1. Create sandbox container
        let sandbox = self.sandbox_manager.create(&id, request.sandbox_config()).await?;

        // 2. Build engine (single agent or swarm)
        let engine = match request.mode {
            ExecutionMode::SingleAgent => self.agent_factory.build(&request),
            ExecutionMode::Swarm { .. } => self.swarm_factory.build(&request),
        };

        // 3. Wire up streaming if interactive
        let (stream_tx, mode) = if request.interactive {
            let (tx, _) = broadcast::channel(256);
            (Some(tx.clone()), TaskMode::Interactive)
        } else {
            (None, TaskMode::Background)
        };

        // 4. Spawn execution
        let cancel = CancellationToken::new();
        let (status_tx, status_rx) = watch::channel(TaskStatus::Running);

        let handle = tokio::spawn({
            let cancel = cancel.clone();
            async move {
                tokio::select! {
                    result = engine.run() => {
                        status_tx.send(TaskStatus::Completed)?;
                        result
                    }
                    _ = cancel.cancelled() => {
                        status_tx.send(TaskStatus::Cancelled)?;
                        Ok(TaskResult::cancelled())
                    }
                }
            }
        });

        self.tasks.insert(id, TaskHandle {
            id, mode, status: status_rx, stream: stream_tx, cancel, join: handle,
        });

        Ok(id)
    }
}
```

---

## 10. Channel Architecture (Pluggable, Streaming)

McClawd's channel system reverse-engineers OpenClaw's channel monitor pattern but fixes its key problems: eager loading all channel SDKs (3MB bundle, 22k filesystem ops on startup), plaintext credential storage, shared DM context between users, and race conditions between auto-reply and tool sends.

### 10a. First Principles

**Principle 1: The app sees only events.** Every channel — regardless of whether it internally polls, holds a persistent WebSocket, manages an IMAP session, or reads stdin — presents the same interface to the pipeline: an async stream of `InboundMessage` in and an async sink of `OutboundChunk` out. The transport mechanism is the channel adapter's private concern. The pipeline never polls, never manages connections, never knows whether it's talking to Telegram or an email inbox. It `recv()`s.

**Principle 2: Some channels own long-lived client sessions.** This is the critical pattern that most agent frameworks get wrong. WhatsApp (Baileys WebSocket with cryptographic session keys), Email (IMAP IDLE with UID validity state), Discord (Gateway WebSocket with sequence numbers and resume tokens), Matrix (sync loop with `since` token) — these all maintain persistent connections where outbound messages *must be routed through the same live connection* that receives inbound events. Contrast with Telegram Bot API where inbound (getUpdates or webhook) and outbound (sendMessage) are independent HTTP paths. The Channel trait must accommodate both models: stateless-API channels where `send_chunk()` is an independent HTTP call, and persistent-session channels where `send_chunk()` routes through the adapter's live connection state.

**Principle 3: Connection state survives daemon restarts.** WhatsApp needs its Baileys session keys to avoid re-scanning the QR code. Email needs its IMAP UID validity to resume from where it left off. Discord needs its session ID + sequence number to RESUME the gateway connection instead of re-IDENTIFYing. Matrix needs its `since` token to avoid re-syncing the entire room history. This state is *mutable session data*, distinct from secrets (which are static credentials). The Channel trait provides `save_state()` / `restore_state()` lifecycle methods and the framework persists this state encrypted alongside secrets.

**Principle 4: Reconnection is the channel's job.** If Discord's WebSocket drops, the Discord adapter reconnects internally with exponential backoff. If the IMAP connection times out, the Email adapter re-opens it. The pipeline only sees a `ChannelHealth` status change — it never "restarts" a channel. The adapter's `start()` method is conceptually an infinite loop that handles all reconnection logic internally, yielding `InboundMessage`s whenever the connection is live.

**Principle 5: Lazy loading.** Only configured channels compile and start. Each channel is a separate crate behind a Cargo feature flag. No eager loading of unused SDKs (OpenClaw bug #28587).

**Principle 6: Secrets via SecretBackend.** Bot tokens, OAuth client secrets, SMTP passwords — all through `SecretBackend`. Config references key names, never values.

**Principle 7: Per-tenant isolation.** Sessions keyed by `(agent_id, channel, account_id, peer_id)`. No cross-agent context leakage.

### 10b. Transport Patterns (Hidden Behind Trait)

The app sees one interface. Internally, channels use five distinct transport patterns. The Channel trait hides all of this.

```
┌───────────────────────────────────────────────────────────────────────┐
│              Channel Trait (uniform event-driven interface)            │
│                                                                       │
│  InboundMessage ←── all channels produce these uniformly              │
│  OutboundChunk  ──→ all channels consume these uniformly              │
├───────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  Transport A: Stateless API (inbound & outbound are independent)      │
│  ┌───────────────────────────────────────────────────────────┐       │
│  │ Telegram (webhook mode), Slack (Events API), LINE         │       │
│  │ Inbound:  HTTP POST callback from platform → normalize    │       │
│  │ Outbound: independent HTTP call (sendMessage API)         │       │
│  │ State to persist: webhook secret only                     │       │
│  └───────────────────────────────────────────────────────────┘       │
│                                                                       │
│  Transport B: Long-Poll (polling disguised as events)                 │
│  ┌───────────────────────────────────────────────────────────┐       │
│  │ Telegram (getUpdates), Email (IMAP poll / IMAP IDLE)      │       │
│  │ Inbound:  poll loop → yield when data arrives → normalize │       │
│  │ Outbound: independent API call (sendMessage / SMTP)       │       │
│  │ State to persist: poll cursor (update_id, IMAP UID)       │       │
│  └───────────────────────────────────────────────────────────┘       │
│                                                                       │
│  Transport C: Persistent Connection (shared inbound/outbound path)    │
│  ┌───────────────────────────────────────────────────────────┐       │
│  │ Discord (Gateway WS), WhatsApp (Baileys WS), Matrix       │       │
│  │ Inbound:  events arrive on persistent connection          │       │
│  │ Outbound: MUST route through same connection / session    │       │
│  │ State to persist: auth keys, sync token, sequence ID,     │       │
│  │   session ID, resume URL (survives daemon restart)        │       │
│  │ Reconnect: internal backoff, resume-from-state            │       │
│  └───────────────────────────────────────────────────────────┘       │
│                                                                       │
│  Transport D: Sidecar Process (separate runtime manages connection)   │
│  ┌───────────────────────────────────────────────────────────┐       │
│  │ Signal (signal-cli Java), WhatsApp (Baileys Node sidecar) │       │
│  │ Inbound:  SSE/HTTP from sidecar → normalize               │       │
│  │ Outbound: HTTP POST to sidecar                            │       │
│  │ State to persist: sidecar owns it, McClawd manages        │       │
│  │   lifecycle (start/stop/restart container)                │       │
│  └───────────────────────────────────────────────────────────┘       │
│                                                                       │
│  Transport E: Local I/O (no network)                                  │
│  ┌───────────────────────────────────────────────────────────┐       │
│  │ CLI (stdin/stdout), Web UI (axum WS upgrade on localhost) │       │
│  │ Inbound:  read from local source → normalize              │       │
│  │ Outbound: write to local sink                             │       │
│  │ State to persist: none                                    │       │
│  └───────────────────────────────────────────────────────────┘       │
│                                                                       │
│  The pipeline never knows which transport is active.                  │
│  It recv()s InboundMessages and sends OutboundChunks. That's it.     │
└───────────────────────────────────────────────────────────────────────┘
```

**Why Transport C is the hard one:** For persistent-connection channels, the outbound path goes *through the connection the channel adapter already holds*. You can't just make an independent HTTP POST — Discord requires you to send on the same Gateway connection, WhatsApp Baileys sends through its encrypted WS session. This is why the `Channel` trait co-locates inbound (`start()`) and outbound (`send_chunk()`) in the same adapter object — the adapter struct owns the connection, and both reads and writes go through it.

**Why Transport B must be invisible:** Email IMAP IDLE, Telegram getUpdates — these are fundamentally polling transports. But to the pipeline they must look event-driven. The adapter runs an internal poll loop (with configurable interval or IMAP IDLE push) and `send()`s normalized messages into the same `mpsc::Sender<InboundMessage>` as any event-driven channel. The pipeline has no concept of polling.

### 10c. Core Abstractions

```rust
// crates/mcclawd-channels/src/lib.rs

/// Normalized inbound message from any channel.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub id: String,                         // platform message ID (for dedup)
    pub channel: ChannelKind,               // which platform
    pub account_id: String,                 // which bot account received it
    pub peer: Peer,                         // who sent it
    pub chat: ChatContext,                  // DM vs group, thread info
    pub content: MessageContent,           // text, media, or command
    pub timestamp: DateTime<Utc>,
    pub reply_to: Option<String>,          // if replying to a specific message
    pub raw: serde_json::Value,            // original platform payload (for debugging)
}

#[derive(Debug, Clone)]
pub struct Peer {
    pub id: String,                         // platform-specific user ID
    pub display_name: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ChatContext {
    DirectMessage,
    Group {
        group_id: String,
        group_name: Option<String>,
        thread_id: Option<String>,          // Discord thread, Slack thread, etc.
    },
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    Command { name: String, args: String },  // /model, /new, /stop, etc.
    Media { mime_type: String, url: String, caption: Option<String> },
    Reaction { emoji: String, target_message_id: String },
}

/// Streaming outbound chunk to a channel.
#[derive(Debug, Clone)]
pub enum OutboundChunk {
    /// Partial text (streamed token-by-token)
    TextDelta(String),
    /// Complete text block (for channels that don't support streaming)
    TextBlock(String),
    /// Tool use started (show typing/working indicator)
    ToolStart { name: String },
    /// Tool use completed
    ToolEnd { name: String, summary: Option<String> },
    /// Media attachment
    Media { mime_type: String, data: Vec<u8>, caption: Option<String> },
    /// Agent finished (flush any buffered content)
    Done,
    /// Error
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChannelKind {
    Cli,
    Web,          // WebSocket-based web UI
    Telegram,
    WhatsApp,
    Discord,
    Slack,
    Signal,
    Matrix,
    Email,        // IMAP inbound + SMTP outbound
    Custom(String),  // extensible for future channels
}
```

### 10d. Channel Trait

Every channel implements a single async trait. This is the core abstraction — the equivalent of OpenClaw's channel monitor + outbound adapter, unified into one trait. The trait co-locates inbound and outbound in the same object because persistent-connection channels (Transport C) require both to route through the same live connection.

```rust
// crates/mcclawd-channels/src/traits.rs

#[async_trait]
pub trait Channel: Send + Sync + 'static {
    /// Channel identifier (e.g. "telegram", "discord", "cli", "email")
    fn kind(&self) -> ChannelKind;

    /// Start the channel. Runs for the lifetime of the daemon.
    ///
    /// Internally, this is an infinite loop that:
    ///   - For Transport A (stateless): listens for webhook callbacks
    ///   - For Transport B (poll): runs a poll loop (IMAP IDLE, getUpdates)
    ///   - For Transport C (persistent): maintains a persistent connection (WS, etc.)
    ///   - For Transport D (sidecar): connects to sidecar process SSE/HTTP
    ///   - For Transport E (local): reads from stdin or accepts WS upgrades
    ///
    /// All transports normalize into InboundMessage and send into `inbound_tx`.
    /// Reconnection is handled internally — the pipeline never restarts a channel.
    async fn start(
        &self,
        ctx: ChannelStartContext,
    ) -> Result<()>;

    /// Send a streaming chunk to a specific peer/chat.
    ///
    /// For stateless-API channels (Telegram, Slack): makes an independent HTTP call.
    /// For persistent-connection channels (Discord, WhatsApp, Matrix): routes through
    /// the adapter's live connection. If connection is down, queues or errors.
    async fn send_chunk(
        &self,
        target: &OutboundTarget,
        chunk: OutboundChunk,
    ) -> Result<()>;

    /// Platform-specific capabilities and limits.
    fn capabilities(&self) -> ChannelCapabilities;

    /// Healthcheck — is the channel connected and functional?
    async fn health(&self) -> ChannelHealth;

    // --- Connection State Lifecycle ---
    // These methods support channels that maintain long-lived client sessions
    // (Discord WS, WhatsApp Baileys, IMAP, Matrix sync) that must survive
    // daemon restarts without re-authentication.

    /// Save mutable connection state for persistence across daemon restarts.
    /// Called periodically by the framework and on graceful shutdown.
    ///
    /// Returns None for stateless channels (CLI, Telegram webhook).
    /// Returns opaque bytes for stateful channels — the channel owns the format.
    /// Framework encrypts and persists this alongside secrets.
    ///
    /// Examples of what gets saved:
    ///   - Discord: session_id + sequence number + resume_gateway_url
    ///   - WhatsApp/Baileys: full auth session keys (multi-device pairing state)
    ///   - Email/IMAP: UID validity + last seen UID per folder
    ///   - Matrix: sync since token
    ///   - Telegram (poll): last update_id offset
    async fn save_state(&self) -> Result<Option<Vec<u8>>> {
        Ok(None)  // default: stateless, nothing to persist
    }

    /// Restore connection state from a previous daemon run.
    /// Called once before `start()`. The channel uses this to resume
    /// its connection without re-authentication.
    ///
    /// If state is None or corrupt, the channel falls back to fresh init
    /// (which may require re-pairing for WhatsApp, re-auth for IMAP, etc.).
    async fn restore_state(&self, _state: Option<Vec<u8>>) -> Result<()> {
        Ok(())  // default: nothing to restore
    }
}

/// Everything a channel needs to start. Passed into `start()`.
pub struct ChannelStartContext {
    /// Send normalized inbound messages here. Pipeline recv()s from the other end.
    pub inbound_tx: mpsc::Sender<InboundMessage>,
    /// Graceful shutdown signal. Channel must select! on this.
    pub lifecycle: CancellationToken,
    /// Access to encrypted secrets (bot tokens, OAuth credentials, SMTP passwords).
    pub secrets: Arc<dyn SecretBackend>,
    /// Callback to persist connection state. Channel calls this when state changes
    /// (e.g. Discord sequence number advances, IMAP UID changes).
    /// Framework encrypts + writes to disk.
    pub persist_state: Arc<dyn Fn(Vec<u8>) -> BoxFuture<'static, Result<()>> + Send + Sync>,
}

#[derive(Debug, Clone)]
pub struct OutboundTarget {
    pub account_id: String,
    pub peer: Peer,
    pub chat: ChatContext,
    pub reply_to_message_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChannelCapabilities {
    pub supports_streaming: bool,       // can we send partial text updates?
    pub supports_editing: bool,         // can we edit sent messages? (Telegram, Discord)
    pub supports_threads: bool,         // Discord threads, Slack threads, Email threads
    pub supports_reactions: bool,
    pub supports_media: bool,
    pub max_message_length: usize,      // 4096 for Telegram, 2000 for Discord, etc.
    pub supports_typing_indicator: bool,
    pub supports_markdown: bool,        // can we send formatted text?
    pub connection_model: ConnectionModel,
}

/// Describes the channel's connection lifecycle. Used by the daemon supervisor
/// for health monitoring and by the config UI for setup guidance.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionModel {
    /// No persistent connection. Each inbound/outbound is an independent request.
    /// Channels: Telegram (webhook), Slack (Events API), LINE
    Stateless,

    /// Channel internally polls for new messages on a timer or IDLE push.
    /// Looks event-driven to the pipeline. Outbound is independent.
    /// Channels: Telegram (getUpdates), Email (IMAP IDLE / poll)
    Poll,

    /// Long-lived connection that carries both inbound and outbound.
    /// Connection state must be persisted across restarts.
    /// Channels: Discord (Gateway WS), WhatsApp (Baileys), Matrix (sync)
    PersistentSession,

    /// Managed sidecar process. McClawd starts/monitors the sidecar container.
    /// Channels: Signal (signal-cli), WhatsApp (Baileys Node sidecar)
    Sidecar,

    /// Local I/O, no network. Process lifetime.
    /// Channels: CLI (stdin/stdout), Web (localhost WS)
    Local,
}

pub enum ChannelHealth {
    Connected,
    Degraded(String),       // connected but issues (e.g. rate limited)
    Reconnecting(String),   // temporarily down, adapter is handling reconnect
    Disconnected(String),   // down, needs operator intervention (re-pair, re-auth)
}
```

### 10e. Channel Router (Bindings)

The router maps inbound messages to agents using **bindings** — the same concept as OpenClaw but parsed from TOML config instead of JSON5.

```rust
// crates/mcclawd-channels/src/router.rs

/// A binding maps a (channel, account, peer) pattern to an agent.
#[derive(Debug, Clone, Deserialize)]
pub struct Binding {
    pub agent_id: String,
    #[serde(rename = "match")]
    pub match_rule: MatchRule,
    pub priority: Option<i32>,  // higher = more specific, checked first
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatchRule {
    pub channel: Option<ChannelKind>,
    pub account_id: Option<String>,
    pub peer_id: Option<String>,
    pub chat_type: Option<ChatType>,  // dm | group
    pub group_id: Option<String>,
}

pub struct ChannelRouter {
    bindings: Vec<Binding>,
    default_agent: String,
}

impl ChannelRouter {
    /// Resolve which agent handles this message.
    /// Bindings evaluated by priority (desc), then specificity.
    /// Falls back to default_agent if no binding matches.
    pub fn resolve(&self, msg: &InboundMessage) -> &str {
        self.bindings.iter()
            .filter(|b| self.matches(b, msg))
            .max_by_key(|b| (b.priority.unwrap_or(0), self.specificity(b)))
            .map(|b| b.agent_id.as_str())
            .unwrap_or(&self.default_agent)
    }

    fn specificity(&self, binding: &Binding) -> i32 {
        let m = &binding.match_rule;
        let mut score = 0;
        if m.peer_id.is_some() { score += 8; }    // most specific
        if m.group_id.is_some() { score += 4; }
        if m.account_id.is_some() { score += 2; }
        if m.channel.is_some() { score += 1; }
        score
    }
}
```

### 10f. Session Management

Sessions are keyed by agent + channel + peer, ensuring complete isolation. This fixes OpenClaw's default `dmScope: "main"` which shares context between all DM senders.

```rust
// crates/mcclawd-channels/src/session.rs

/// Globally unique session key. Format matches OpenClaw for compat:
/// agent:{agent_id}:{channel}:{peer_id}
/// agent:{agent_id}:{channel}:group:{group_id}:thread:{thread_id}
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SessionKey(String);

impl SessionKey {
    pub fn for_dm(agent_id: &str, channel: &ChannelKind, peer_id: &str) -> Self {
        Self(format!("agent:{agent_id}:{}:{peer_id}", channel.as_str()))
    }

    pub fn for_group(
        agent_id: &str,
        channel: &ChannelKind,
        group_id: &str,
        thread_id: Option<&str>,
    ) -> Self {
        match thread_id {
            Some(tid) => Self(format!(
                "agent:{agent_id}:{}:group:{group_id}:thread:{tid}",
                channel.as_str()
            )),
            None => Self(format!(
                "agent:{agent_id}:{}:group:{group_id}",
                channel.as_str()
            )),
        }
    }
}

pub struct SessionManager {
    sessions: DashMap<SessionKey, Session>,
}

pub struct Session {
    pub key: SessionKey,
    pub agent_id: String,
    pub history: Vec<Turn>,
    pub memory: HashMap<String, Value>,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub model_override: Option<String>,     // /model command
    pub metadata: SessionMetadata,
}

pub struct SessionMetadata {
    pub channel: ChannelKind,
    pub peer: Peer,
    pub chat: ChatContext,
    pub total_tokens: usize,
    pub turn_count: usize,
}
```

### 10g. Inbound Pipeline

The message pipeline follows OpenClaw's proven 6-stage flow but adds proper dedup and security:

```
Inbound message
    │
    ▼
┌─────────────────┐
│ 1. Normalize     │  Channel adapter → InboundMessage
│                  │  (platform-specific → unified format)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 2. Deduplicate   │  LRU cache keyed by (channel, account, peer, msg_id)
│                  │  Drop if seen in last 60s (platforms redeliver)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 3. Access Check  │  DM policy: pairing | allowlist | open | disabled
│                  │  Group policy: allowlist | mention-only | open
│                  │  Per-channel + per-account overrides
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 4. Route         │  Bindings → which agent handles this?
│                  │  Session key → find or create session
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 5. Debounce      │  Batch rapid messages (configurable per channel)
│   (optional)     │  WhatsApp: 5000ms, Slack: 1500ms, Telegram: 2000ms
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 6. Dispatch      │  Command? → handle directly (no agent)
│                  │  Message? → TaskManager.spawn_task()
│                  │  → AgentEngine.run() with session context
│                  │  → stream OutboundChunks back to channel
└─────────────────┘
```

```rust
// crates/mcclawd-channels/src/pipeline.rs

pub struct InboundPipeline {
    router: ChannelRouter,
    sessions: Arc<SessionManager>,
    access: AccessController,
    dedup: DedupCache,
    debounce: DebounceManager,
    task_manager: Arc<TaskManager>,
    channels: HashMap<ChannelKind, Arc<dyn Channel>>,
}

impl InboundPipeline {
    /// Main message processing loop. Reads from all channel inbound senders.
    pub async fn run(
        &self,
        mut inbound_rx: mpsc::Receiver<InboundMessage>,
    ) -> Result<()> {
        while let Some(msg) = inbound_rx.recv().await {
            // 1. Already normalized by channel adapter

            // 2. Dedup
            if self.dedup.is_duplicate(&msg) {
                tracing::debug!(msg_id = %msg.id, "Duplicate message, skipping");
                continue;
            }

            // 3. Access check
            match self.access.check(&msg).await {
                AccessResult::Allowed => {}
                AccessResult::NeedsPairing => {
                    self.send_pairing_prompt(&msg).await?;
                    continue;
                }
                AccessResult::Denied(reason) => {
                    tracing::info!(peer = %msg.peer.id, %reason, "Access denied");
                    continue;
                }
            }

            // 4. Route to agent + resolve session
            let agent_id = self.router.resolve(&msg);
            let session_key = SessionKey::from_message(agent_id, &msg);
            let session = self.sessions.get_or_create(&session_key, &msg);

            // 5. Debounce (batches rapid messages)
            if let Some(batched) = self.debounce.submit(msg, &session_key).await {
                // 6. Dispatch
                self.dispatch(batched, &session_key, agent_id).await?;
            }
        }
        Ok(())
    }

    async fn dispatch(
        &self,
        msg: InboundMessage,
        session_key: &SessionKey,
        agent_id: &str,
    ) -> Result<()> {
        // Check for slash commands first
        if let MessageContent::Command { name, args } = &msg.content {
            return self.handle_command(name, args, session_key, &msg).await;
        }

        // Get the channel adapter for sending responses
        let channel = self.channels.get(&msg.channel)
            .ok_or_else(|| anyhow!("No channel adapter for {:?}", msg.channel))?;

        let target = OutboundTarget::from_message(&msg);

        // Show typing indicator
        if channel.capabilities().supports_typing_indicator {
            channel.send_chunk(&target, OutboundChunk::ToolStart {
                name: "thinking".into()
            }).await?;
        }

        // Spawn task with streaming callback that sends chunks to channel
        let channel = channel.clone();
        let target = target.clone();

        self.task_manager.spawn_task(TaskRequest {
            prompt: msg.content.as_text().unwrap_or_default(),
            agent_id: agent_id.to_string(),
            session_key: session_key.clone(),
            interactive: true,
            stream_callback: Some(Arc::new(move |chunk: OutboundChunk| {
                let channel = channel.clone();
                let target = target.clone();
                Box::pin(async move {
                    channel.send_chunk(&target, chunk).await
                })
            })),
            ..Default::default()
        }).await
    }
}
```

### 10h. Streaming & Chunking

Each channel handles streaming differently based on its capabilities:

```rust
// crates/mcclawd-channels/src/chunker.rs

/// Adapts streaming output to channel constraints.
pub struct ChannelChunker {
    capabilities: ChannelCapabilities,
    buffer: String,
    flush_interval: Duration,     // how often to flush partial text
}

impl ChannelChunker {
    /// Process a stream of OutboundChunks into channel-appropriate sends.
    pub async fn process(
        &mut self,
        chunk: OutboundChunk,
        send: &dyn Fn(OutboundChunk) -> BoxFuture<Result<()>>,
    ) -> Result<()> {
        match chunk {
            OutboundChunk::TextDelta(text) => {
                self.buffer.push_str(&text);

                if self.capabilities.supports_streaming {
                    // Edit-based streaming: Telegram, Discord
                    // Buffer until flush_interval, then edit the message
                    if self.should_flush() {
                        send(OutboundChunk::TextBlock(self.buffer.clone())).await?;
                    }
                }
                // Non-streaming channels (WhatsApp, SMS): buffer until Done
            }

            OutboundChunk::Done => {
                // Flush remaining buffer, splitting if over max_message_length
                for chunk_text in self.split_by_limit(&self.buffer) {
                    send(OutboundChunk::TextBlock(chunk_text)).await?;
                }
                self.buffer.clear();
            }

            other => send(other).await?,
        }
        Ok(())
    }

    /// Split text at message boundary (prefer newlines, then spaces)
    fn split_by_limit(&self, text: &str) -> Vec<String> {
        let max = self.capabilities.max_message_length;
        if text.len() <= max {
            return vec![text.to_string()];
        }
        // Split at last newline before limit, or last space, or hard split
        // Preserve code blocks across splits
        todo!()
    }
}
```

**Per-channel streaming behavior:**

| Channel | Transport | Streaming | Mechanism | Max Length | Flush | Persisted State |
|---------|-----------|-----------|-----------|------------|-------|-----------------|
| CLI | E: Local | Yes | stdout write | Unlimited | Per token | None |
| Web/WS | E: Local | Yes | WebSocket frames | Unlimited | Per token | None |
| Telegram | B: Poll | Yes | `editMessageText` | 4096 | 500ms | `update_id` offset |
| Discord | C: Persistent | Yes | `editMessage` | 2000 | 500ms | session_id, seq, resume_url |
| Slack | A: Stateless | Yes | `chat.update` | 40000 | 1000ms | None (webhook secret only) |
| WhatsApp | C/D: Persistent/Sidecar | No | Buffer → send on Done | 65536 | N/A | Baileys auth session keys |
| Signal | D: Sidecar | No | Buffer → send on Done | 6000 | N/A | Sidecar owns state |
| Email | B: Poll | No | Buffer → SMTP send | Unlimited | N/A | IMAP UID validity + last UID |
| Matrix | C: Persistent | Yes | Room event | 65536 | 500ms | `since` sync token |

**Channel lifecycle orchestration:**

The framework manages the full lifecycle of each configured channel. This is the code path that ensures Transport B/C channels resume cleanly across daemon restarts:

```rust
// In daemon startup (mcclawd-api/daemon.rs or main.rs)

async fn start_channels(
    channels: Vec<Arc<dyn Channel>>,
    state_store: Arc<ChannelStateStore>,  // encrypted on-disk state per channel
    secrets: Arc<dyn SecretBackend>,
    inbound_tx: mpsc::Sender<InboundMessage>,
    lifecycle: CancellationToken,
) -> Result<Vec<JoinHandle<()>>> {
    let mut handles = vec![];

    for channel in channels {
        let kind = channel.kind();

        // 1. Restore persisted connection state (if any)
        let saved = state_store.load(&kind).await?;
        channel.restore_state(saved).await?;

        // 2. Build start context with state persistence callback
        let state_store = state_store.clone();
        let kind_for_persist = kind.clone();
        let ctx = ChannelStartContext {
            inbound_tx: inbound_tx.clone(),
            lifecycle: lifecycle.clone(),
            secrets: secrets.clone(),
            persist_state: Arc::new(move |data| {
                let store = state_store.clone();
                let k = kind_for_persist.clone();
                Box::pin(async move { store.save(&k, data).await })
            }),
        };

        // 3. Spawn channel — it runs forever (handles reconnection internally)
        let handle = tokio::spawn(async move {
            if let Err(e) = channel.start(ctx).await {
                tracing::error!(channel = ?kind, "Channel exited with error: {e}");
            }
        });
        handles.push(handle);
    }
    Ok(handles)
}

// On graceful shutdown: save final state for all stateful channels
async fn shutdown_channels(channels: &[Arc<dyn Channel>], state_store: &ChannelStateStore) {
    for channel in channels {
        if let Ok(Some(state)) = channel.save_state().await {
            let _ = state_store.save(&channel.kind(), state).await;
        }
    }
}
```

`ChannelStateStore` encrypts state at rest using the same AES-256-GCM-SIV backend as secrets. State files live at `~/.mcclawd/state/<channel_kind>/<account_id>.enc`. This is distinct from secrets (static credentials) — state is mutable data that changes as the channel operates (sequence numbers, sync tokens, IMAP cursors).

### 10i. Access Control

```rust
// crates/mcclawd-channels/src/access.rs

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DmPolicy {
    Pairing,    // require pairing code exchange before first interaction
    Allowlist,  // only allow listed peer IDs
    Open,       // allow anyone (dangerous, but useful for public bots)
    Disabled,   // DMs disabled entirely
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupPolicy {
    Allowlist,     // only in listed groups
    MentionOnly,   // only respond when @mentioned
    Open,          // respond to all messages in group
}

pub struct AccessController {
    default_dm_policy: DmPolicy,
    default_group_policy: GroupPolicy,
    per_channel: HashMap<ChannelKind, ChannelAccessConfig>,
    paired_peers: DashMap<(ChannelKind, String), DateTime<Utc>>,  // persisted
    allowlist: HashSet<(ChannelKind, String)>,
}

pub enum AccessResult {
    Allowed,
    NeedsPairing,
    Denied(String),
}
```

### 10j. Channel Secrets & Per-Tenant Config

All channel credentials flow through the `SecretBackend`. Config references keys, not values.

```toml
# mcclawd.toml — channel configuration

[channels.cli]
enabled = true  # always on in dev

[channels.web]
enabled = true
bind = "127.0.0.1:8080"
cors_origins = ["http://localhost:3000"]

[channels.telegram]
enabled = true
dm_policy = "pairing"
group_policy = "mention-only"

[channels.telegram.accounts.main]
bot_token_secret = "TELEGRAM_BOT_TOKEN"       # resolved from SecretBackend
polling = true                                 # long-polling (no webhook needed)
mention_patterns = ["@mcclawd", "@mc"]

[channels.telegram.accounts.work]
bot_token_secret = "TELEGRAM_WORK_BOT_TOKEN"
polling = true

[channels.discord]
enabled = true
dm_policy = "allowlist"
group_policy = "mention-only"

[channels.discord.accounts.main]
bot_token_secret = "DISCORD_BOT_TOKEN"
intents = ["MESSAGE_CONTENT", "GUILD_MESSAGES", "DIRECT_MESSAGES"]

[channels.whatsapp]
enabled = true
dm_policy = "pairing"

[channels.whatsapp.accounts.personal]
# WhatsApp uses session-based auth (QR code pairing)
# Session data stored encrypted in secrets backend
session_secret = "WHATSAPP_SESSION"

[channels.email]
enabled = true
dm_policy = "allowlist"  # only respond to known senders

[channels.email.accounts.personal]
imap_host = "imap.gmail.com"
imap_port = 993
smtp_host = "smtp.gmail.com"
smtp_port = 587
from_address = "myagent@gmail.com"
credentials_secret = "EMAIL_OAUTH_TOKEN"     # OAuth2 token from SecretBackend
poll_mode = "idle"                            # "idle" (IMAP IDLE push) or "poll"
poll_interval_secs = 30                       # fallback if IDLE unsupported
folders = ["INBOX"]                           # which folders to monitor

# Multi-tenant: map accounts to agents via bindings
[[bindings]]
agent_id = "default"
match = { channel = "telegram", account_id = "main" }

[[bindings]]
agent_id = "coding"
match = { channel = "telegram", account_id = "work" }

[[bindings]]
agent_id = "default"
match = { channel = "discord" }

# Route a specific WhatsApp group to a specific agent
[[bindings]]
agent_id = "research"
match = { channel = "whatsapp", chat_type = "group", group_id = "120363..." }

# Per-channel debounce overrides
[channels.debounce]
default_ms = 2000
whatsapp = 5000
slack = 1500
discord = 1500
```

**How secrets work for channels:**

```rust
// Channel initialization fetches credentials from SecretBackend
impl TelegramChannel {
    pub async fn new(
        config: &TelegramChannelConfig,
        secrets: Arc<dyn SecretBackend>,
    ) -> Result<Self> {
        // Resolve bot token from secret backend (encrypted, not plaintext JSON)
        let token = secrets.get(&config.accounts["main"].bot_token_secret).await?;

        // token is Secret<String> — auto-zeroizes on drop, can't be logged
        let bot = teloxide::Bot::new(token.expose().clone());

        Ok(Self { bot, config: config.clone() })
    }
}
```

**Per-tenant isolation model:**

```
Tenant A (personal)                    Tenant B (work team)
┌──────────────────┐                  ┌──────────────────┐
│ WhatsApp account │─binding──→ agent:default   │ Slack workspace  │─binding──→ agent:work
│ Telegram @mybot  │─binding──→ agent:default   │ Discord server   │─binding──→ agent:work
│ Web UI           │─binding──→ agent:default   │                  │
└──────────────────┘                  └──────────────────┘
         │                                      │
         ▼                                      ▼
    Workspace: default/                    Workspace: work/
    SOUL.md (personal style)               SOUL.md (professional style)
    USER.md (my prefs)                     USER.md (team context)
    AGENTS.md                              AGENTS.md (different skills)
    Skills: personal set                   Skills: work set
    Sessions: isolated                     Sessions: isolated
    Secrets: scoped                        Secrets: scoped
```

### 10k. Channel Implementations (Phased)

**Phase 0: CLI Channel** — stdin/stdout, direct streaming, no network (Transport E: Local I/O)

```rust
// crates/mcclawd-channels/src/cli.rs

pub struct CliChannel {
    prompt: String,  // "mc> "
}

#[async_trait]
impl Channel for CliChannel {
    fn kind(&self) -> ChannelKind { ChannelKind::Cli }

    async fn start(&self, ctx: ChannelStartContext) -> Result<()> {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();

        loop {
            print!("{}", self.prompt);
            tokio::select! {
                line = lines.next_line() => {
                    match line? {
                        Some(text) if !text.is_empty() => {
                            ctx.inbound_tx.send(InboundMessage {
                                id: Uuid::new_v4().to_string(),
                                channel: ChannelKind::Cli,
                                account_id: "local".into(),
                                peer: Peer { id: "user".into(), display_name: None, username: None },
                                chat: ChatContext::DirectMessage,
                                content: MessageContent::Text(text),
                                timestamp: Utc::now(),
                                reply_to: None,
                                raw: serde_json::Value::Null,
                            }).await?;
                        }
                        None => break,  // EOF
                        _ => {}
                    }
                }
                _ = ctx.lifecycle.cancelled() => break,
            }
        }
        Ok(())
    }

    async fn send_chunk(&self, _target: &OutboundTarget, chunk: OutboundChunk) -> Result<()> {
        match chunk {
            OutboundChunk::TextDelta(text) => {
                print!("{text}");
                std::io::stdout().flush()?;
            }
            OutboundChunk::Done => println!(),
            OutboundChunk::ToolStart { name } => {
                // Rich/colorama style progress indicator
                println!("\x1b[90m⚙ Using {name}...\x1b[0m");
            }
            OutboundChunk::ToolEnd { name, summary } => {
                if let Some(s) = summary {
                    println!("\x1b[90m✓ {name}: {s}\x1b[0m");
                }
            }
            OutboundChunk::Error(e) => eprintln!("\x1b[31m✗ Error: {e}\x1b[0m"),
            _ => {}
        }
        Ok(())
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            supports_streaming: true,
            supports_editing: false,
            supports_threads: false,
            supports_reactions: false,
            supports_media: false,
            max_message_length: usize::MAX,
            supports_typing_indicator: false,
            supports_markdown: false,
            connection_model: ConnectionModel::Local,
        }
    }

    async fn health(&self) -> ChannelHealth { ChannelHealth::Connected }

    // save_state / restore_state: default (None) — CLI is stateless
}
```

**Phase 1: Web/WS Channel** — axum WebSocket, full streaming, web UI

```rust
// Sketch — streams via WebSocket frames
pub struct WebChannel { /* axum WS upgrade handler */ }

// WS frame protocol:
// Client → Server: { "type": "message", "text": "..." }
// Server → Client: { "type": "delta", "text": "..." }
// Server → Client: { "type": "tool_start", "name": "..." }
// Server → Client: { "type": "tool_end", "name": "...", "summary": "..." }
// Server → Client: { "type": "done" }
// Server → Client: { "type": "error", "message": "..." }
```

**Phase 2: Telegram Channel** — `teloxide` crate, long-polling, edit-based streaming

```rust
// Sketch — Telegram via teloxide (Rust)
pub struct TelegramChannel {
    bot: teloxide::Bot,
    // Edit-based streaming: send initial message, then editMessageText
    // every 500ms with accumulated text
}
```

**Phase 3: Discord, Slack, WhatsApp, Signal, Email** — each as separate crates

| Channel | Rust Crate | Transport Pattern | Auth | Persisted State |
|---------|-----------|-------------------|------|-----------------|
| Telegram | `teloxide` | B: Poll (getUpdates) or A: Webhook | Bot token | update_id cursor |
| Discord | `serenity` | C: Persistent (Gateway WS) | Bot token | session_id + seq + resume_url |
| Slack | `slack-morphism` | A: Socket Mode or Events API | App + Bot tokens | None |
| WhatsApp | Baileys sidecar (Node) | D: Sidecar → C: Persistent (inside sidecar) | QR session | Baileys auth keys (sidecar persists) |
| Signal | `signal-cli` sidecar (Java) | D: Sidecar | Registered phone | Sidecar owns state |
| Email | `async-imap` + `lettre` | B: Poll (IMAP IDLE / interval poll) | IMAP + SMTP creds (OAuth or password) | UID validity + last seen UID + folder list |
| Matrix | `matrix-sdk` | C: Persistent (sync loop) | Access token | `since` sync token |

**Email Channel Sketch** — IMAP IDLE inbound, SMTP outbound (Transport B: Poll):

```rust
// crates/mcclawd-channel-email/src/lib.rs

pub struct EmailChannel {
    imap_config: ImapConfig,  // host, port, folder, poll_interval
    smtp_config: SmtpConfig,  // host, port, from_address
}

#[async_trait]
impl Channel for EmailChannel {
    fn kind(&self) -> ChannelKind { ChannelKind::Email }

    async fn start(&self, ctx: ChannelStartContext) -> Result<()> {
        // Resolve IMAP/SMTP credentials from SecretBackend
        let imap_creds = ctx.secrets.get(&self.imap_config.credentials_secret).await?;
        let mut imap = AsyncImapSession::connect(&self.imap_config, imap_creds).await?;
        imap.select("INBOX").await?;

        // Restore cursor from previous run (UID validity + last seen UID)
        // If restored state doesn't match current UID validity, full re-scan
        let mut cursor = self.last_uid.load(Ordering::SeqCst);

        loop {
            tokio::select! {
                // IMAP IDLE — blocks until new mail arrives (or timeout)
                // If server doesn't support IDLE, falls back to poll interval
                result = imap.idle_or_poll(self.imap_config.poll_interval) => {
                    match result {
                        Ok(new_messages) => {
                            for msg in new_messages {
                                let inbound = self.normalize_email(&msg)?;
                                ctx.inbound_tx.send(inbound).await?;
                                cursor = msg.uid;
                            }
                            // Persist updated cursor so daemon restart resumes here
                            let state = serde_json::to_vec(&EmailState {
                                uid_validity: imap.uid_validity(),
                                last_uid: cursor,
                            })?;
                            (ctx.persist_state)(state).await?;
                        }
                        Err(e) => {
                            // Reconnect internally — pipeline doesn't know
                            tracing::warn!("IMAP error, reconnecting: {e}");
                            imap = AsyncImapSession::connect(
                                &self.imap_config, imap_creds.clone()
                            ).await?;
                            imap.select("INBOX").await?;
                        }
                    }
                }
                _ = ctx.lifecycle.cancelled() => break,
            }
        }
        Ok(())
    }

    async fn send_chunk(&self, target: &OutboundTarget, chunk: OutboundChunk) -> Result<()> {
        // Email is non-streaming: buffer all chunks, send on Done via SMTP
        match chunk {
            OutboundChunk::TextDelta(text) => self.outbound_buffer.write().push_str(&text),
            OutboundChunk::Done => {
                let body = self.outbound_buffer.write().drain(..).collect::<String>();
                let smtp_creds = self.smtp_creds.clone(); // cached from init
                send_email_smtp(&self.smtp_config, smtp_creds, target, &body).await?;
            }
            _ => {} // Email ignores tool indicators, media goes as attachment
        }
        Ok(())
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            supports_streaming: false,  // email is send-on-complete
            supports_editing: false,
            supports_threads: true,     // email threading via In-Reply-To / References
            supports_reactions: false,
            supports_media: true,       // attachments
            max_message_length: usize::MAX,
            supports_typing_indicator: false,
            supports_markdown: true,    // HTML email
            connection_model: ConnectionModel::Poll,
        }
    }

    async fn save_state(&self) -> Result<Option<Vec<u8>>> {
        Ok(Some(serde_json::to_vec(&EmailState {
            uid_validity: self.uid_validity.load(Ordering::SeqCst),
            last_uid: self.last_uid.load(Ordering::SeqCst),
        })?))
    }

    async fn restore_state(&self, state: Option<Vec<u8>>) -> Result<()> {
        if let Some(data) = state {
            let s: EmailState = serde_json::from_slice(&data)?;
            self.uid_validity.store(s.uid_validity, Ordering::SeqCst);
            self.last_uid.store(s.last_uid, Ordering::SeqCst);
        }
        Ok(())
    }

    async fn health(&self) -> ChannelHealth {
        // Check if IMAP connection is alive
        match self.imap_connected.load(Ordering::SeqCst) {
            true => ChannelHealth::Connected,
            false => ChannelHealth::Reconnecting("IMAP connection lost, retrying".into()),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct EmailState {
    uid_validity: u32,
    last_uid: u32,
}
```

### 10l. Crate Structure

```
crates/
├── mcclawd-channels/               # channel framework
│   ├── src/
│   │   ├── lib.rs                  # InboundMessage, OutboundChunk, ChannelKind
│   │   ├── traits.rs               # Channel trait + ChannelStartContext + ConnectionModel
│   │   ├── router.rs               # Binding-based routing
│   │   ├── session.rs              # SessionKey, SessionManager
│   │   ├── pipeline.rs             # InboundPipeline (6-stage)
│   │   ├── access.rs               # DmPolicy, GroupPolicy, AccessController
│   │   ├── chunker.rs              # Per-channel streaming/chunking
│   │   ├── dedup.rs                # LRU dedup cache
│   │   ├── debounce.rs             # Per-channel debounce
│   │   ├── state.rs                # Encrypted channel state persistence (save/restore)
│   │   └── cli.rs                  # CLI channel (Phase 0)
│   └── Cargo.toml
│
├── mcclawd-channel-web/            # Web/WS channel (Phase 1)
│   ├── src/
│   │   └── lib.rs
│   └── Cargo.toml
│
├── mcclawd-channel-telegram/       # Telegram channel (Phase 2)
│   ├── src/
│   │   └── lib.rs
│   └── Cargo.toml
│
├── mcclawd-channel-discord/        # Discord channel (Phase 3)
├── mcclawd-channel-slack/          # Slack channel (Phase 3)
├── mcclawd-channel-whatsapp/       # WhatsApp channel (Phase 3+)
├── mcclawd-channel-signal/         # Signal channel (Phase 3+)
├── mcclawd-channel-email/          # Email IMAP+SMTP channel (Phase 3+)
└── mcclawd-channel-matrix/         # Matrix channel (Phase 3+)
```

Each channel is a separate crate → separate feature flag → only compiled/loaded if configured. This is how we avoid OpenClaw's "load all 20 SDKs on startup" problem.

### 10m. OpenClaw Compatibility

McClawd reads OpenClaw's channel config format (`openclaw.json`) for migration:

```rust
// crates/mcclawd-channels/src/compat.rs

/// Parse OpenClaw's channel config and convert to McClawd format.
/// Handles: channels.telegram, channels.whatsapp, channels.discord, etc.
/// Converts plaintext tokens → SecretBackend references (prompts to import).
pub fn import_openclaw_channels(config: &OpenClawConfig) -> Result<ChannelsConfig> {
    // For each channel in openclaw.json:
    // 1. Extract bot token / credentials
    // 2. Prompt user to store in SecretBackend
    // 3. Generate McClawd TOML channel config with secret references
    // 4. Convert bindings from openclaw format to McClawd format
    todo!()
}
```

CLI channel management:

```bash
# List configured channels and their health/status
mc channels list
#  CHANNEL    ACCOUNT     STATUS       TRANSPORT    SESSIONS
#  cli        local       connected    local        1
#  web        main        connected    local        3
#  telegram   main        connected    poll         12
#  email      personal    connected    poll(idle)   5
#  discord    main        reconnecting persistent   0

# Show detailed health for a specific channel
mc channels health telegram
#  Account: main
#  Transport: Poll (getUpdates)
#  Status: Connected
#  Last poll: 2s ago
#  Persisted state: update_id=48291
#  Sessions: 12 active, 3 idle

# Import channel config from OpenClaw
mc channels import-openclaw ~/.openclaw/openclaw.json

# Trigger WhatsApp QR pairing (for sidecar setup)
mc channels pair whatsapp personal

# Approve a pending pairing request (like OpenClaw's pairing system)
mc channels pairing list
mc channels pairing approve --channel telegram --peer "@johndoe"

# Force save channel state (normally automatic)
mc channels save-state telegram main

# Reset channel state (forces re-auth / re-sync)
mc channels reset-state whatsapp personal
```

---

## 11. Secrets Architecture

Carried forward from the Feb 22 design. Secrets are a first-class concern, not a "later" problem.

### 11a. SecretBackend Trait

The core owns all secret access. No component reads secrets directly — everything goes through the backend.

```rust
// crates/mcclawd-core/src/secrets/mod.rs

use zeroize::Zeroize;

/// Secret value that auto-zeroizes on drop.
/// Cannot be Clone'd, Debug'd, or Serialize'd.
/// Call .expose() to get the inner value exactly once for use.
pub struct Secret<T: Zeroize> {
    inner: T,
}

impl<T: Zeroize> Secret<T> {
    pub fn new(value: T) -> Self { Self { inner: value } }
    pub fn expose(&self) -> &T { &self.inner }
}

impl<T: Zeroize> Drop for Secret<T> {
    fn drop(&mut self) { self.inner.zeroize(); }
}

// No Clone, no Debug, no Serialize — by omission.

#[async_trait]
pub trait SecretBackend: Send + Sync {
    /// Retrieve a secret by key.
    async fn get(&self, key: &str) -> Result<Secret<String>>;

    /// Store a secret.
    async fn set(&self, key: &str, value: Secret<String>) -> Result<()>;

    /// Delete a secret.
    async fn delete(&self, key: &str) -> Result<()>;

    /// List secret keys under a prefix (values not returned).
    async fn list(&self, prefix: &str) -> Result<Vec<String>>;
}
```

### 11b. Backends (Phased)

| Backend | Phase | Use Case | Crate |
|---|---|---|---|
| **Encrypted file** (`secrets.enc`) | Phase 0 | Dev / single machine | `aes-gcm-siv` + `argon2` key derivation |
| **OS keychain** | Phase 1 | Desktop dev | `keyring` (macOS Keychain, Linux secret-service) |
| **HashiCorp Vault** | Phase 2 | Production self-hosted | `vaultrs`, KV v2 + Transit engine |
| **AWS Secrets Manager / KMS** | Phase 3+ | AWS production | `aws-sdk-secretsmanager` |

Phase 0 default: encrypted file backend. Master key from `MCCLAWD_MASTER_KEY` env var or derived from passphrase via argon2.

```rust
// crates/mcclawd-core/src/secrets/encrypted_file.rs

pub struct EncryptedFileBackend {
    path: PathBuf,
    cipher: Aes256GcmSiv,  // derived from master key
}

impl EncryptedFileBackend {
    pub fn open(path: PathBuf, master_key: &[u8]) -> Result<Self> {
        // argon2id key derivation from master_key → 256-bit AES key
        // Load or create secrets.enc
        // File format: nonce (12 bytes) || ciphertext (JSON blob, encrypted)
        todo!()
    }
}

#[async_trait]
impl SecretBackend for EncryptedFileBackend {
    async fn get(&self, key: &str) -> Result<Secret<String>> {
        // Decrypt file → parse JSON → extract key → wrap in Secret<T>
        todo!()
    }
    // ...
}
```

### 11c. Secret Scoping & Delivery

Each MCP server and skill declares what secrets it needs. The config maps them:

```toml
# mcclawd.toml
[secrets]
backend = "file"              # "file" | "keychain" | "vault"

[secrets.file]
path = "config/secrets.enc"

[secrets.scopes]
# MCP server "github" gets these secrets injected as env vars
github = ["GITHUB_TOKEN"]
notion = ["NOTION_TOKEN"]
anthropic = ["ANTHROPIC_API_KEY"]

# Skills that need env vars (matched by skill name from SKILL.md frontmatter)
[secrets.skills]
todoist-cli = ["TODOIST_API_KEY"]
```

**Delivery to sandbox containers:** Secrets are written to a tmpfs mount inside the container at `/run/secrets/<KEY>` (mode 0400). They never appear in container env vars (which are visible via `docker inspect`), never in the agent's LLM context, and never in logs.

```rust
// In sandbox.rs, when creating a container:
let tmpfs_mounts = secrets_for_task
    .iter()
    .map(|(key, secret)| {
        // Write to tmpfs inside container, not env var
        TmpfsSecret {
            path: format!("/run/secrets/{key}"),
            value: secret.expose().clone(),
            mode: 0o400,
        }
    })
    .collect();
```

**Audit:** Every secret access emits a tracing event:
```
secret.accessed { key: "GITHUB_TOKEN", consumer: "mcp:github", task_id: "abc-123" }
```
The value is never logged.

---

## 12. Identity (JWT with SPIFFE Upgrade Path)

Phase 0 uses JWT tokens for agent and task identity. The trait is designed so SPIFFE/SPIRE can slot in later without changing consumers.

```rust
// crates/mcclawd-core/src/identity/mod.rs

/// Represents a verified identity for an agent or task.
#[derive(Clone, Debug)]
pub struct AgentIdentity {
    /// Unique identity URI. Format: "mcclawd://agent/<name>" now,
    /// "spiffe://cluster/agent/<name>" later.
    pub id: String,
    /// Claims about this identity (roles, scopes, etc.)
    pub claims: HashMap<String, Value>,
    /// When this identity expires (for token rotation)
    pub expires_at: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    /// Issue an identity for an agent.
    async fn issue_agent_identity(&self, agent_name: &str) -> Result<AgentIdentity>;

    /// Issue a delegated identity for a task (agent acts on behalf of task).
    async fn issue_task_identity(
        &self,
        parent: &AgentIdentity,
        task_id: &TaskId,
    ) -> Result<AgentIdentity>;

    /// Verify an identity token.
    async fn verify(&self, token: &str) -> Result<AgentIdentity>;
}
```

### JWT Implementation (Phase 0)

```rust
// crates/mcclawd-core/src/identity/jwt.rs

pub struct JwtIdentityProvider {
    signing_key: jsonwebtoken::EncodingKey,  // Ed25519 or HMAC
    verification_key: jsonwebtoken::DecodingKey,
    issuer: String,  // "mcclawd://local"
}

impl JwtIdentityProvider {
    /// Create from a key file or generate ephemeral key for dev.
    pub fn from_config(config: &IdentityConfig) -> Result<Self> { todo!() }
}

#[async_trait]
impl IdentityProvider for JwtIdentityProvider {
    async fn issue_agent_identity(&self, agent_name: &str) -> Result<AgentIdentity> {
        // JWT with sub: "mcclawd://agent/{name}", iat, exp
        // Claims: roles (from config), allowed_tools, etc.
        todo!()
    }

    async fn issue_task_identity(
        &self,
        parent: &AgentIdentity,
        task_id: &TaskId,
    ) -> Result<AgentIdentity> {
        // JWT with sub: "mcclawd://task/{id}", act: { sub: parent.id }
        // RFC 8693 "act" claim for delegation chain
        todo!()
    }

    async fn verify(&self, token: &str) -> Result<AgentIdentity> {
        // Standard JWT verification
        todo!()
    }
}
```

**SPIFFE upgrade path:** Replace `JwtIdentityProvider` with `SpiffeIdentityProvider` that gets SVIDs from a SPIRE agent workload API. The `AgentIdentity` struct stays the same — `id` becomes a real SPIFFE ID (`spiffe://cluster/ns/mcclawd/agent/planner`), and `claims` come from X.509 extensions. No consumer code changes.

**Where identity is used:**
- Task sandbox containers get their identity token mounted at `/run/identity/token`
- AgentGateway RBAC rules can match on identity claims (which tools an agent can access)
- Audit log records identity on every action
- Future: delegation chain tracks human → agent → sub-agent

---

## 13. Daemon Supervisor

McClawd runs as a supervised daemon that self-heals on crash.

```rust
// crates/mcclawd-api/src/daemon.rs

use nix::unistd::{fork, ForkResult};
use std::process::Command;

pub struct DaemonSupervisor {
    max_restarts: usize,
    restart_delay: Duration,
    pid_file: PathBuf,
}

impl DaemonSupervisor {
    /// Fork into background and monitor the child process.
    /// Restarts on crash with backoff. Gives up after max_restarts.
    pub fn run(self) -> Result<()> {
        // Write PID file
        std::fs::write(&self.pid_file, std::process::id().to_string())?;

        let mut restarts = 0;
        loop {
            match unsafe { fork() }? {
                ForkResult::Parent { child } => {
                    // Monitor child
                    let status = nix::sys::wait::waitpid(child, None)?;
                    match status {
                        WaitStatus::Exited(_, 0) => {
                            tracing::info!("Clean shutdown");
                            break;
                        }
                        WaitStatus::Signaled(_, signal, _) => {
                            tracing::warn!("Crashed with signal {signal}, restarting...");
                        }
                        _ => {
                            tracing::warn!("Unexpected exit, restarting...");
                        }
                    }

                    restarts += 1;
                    if restarts > self.max_restarts {
                        tracing::error!("Max restarts ({}) exceeded", self.max_restarts);
                        break;
                    }
                    std::thread::sleep(self.restart_delay * restarts as u32);
                }
                ForkResult::Child => {
                    // Run the actual server
                    return Ok(());  // caller proceeds to start axum
                }
            }
        }
        // Cleanup PID file
        let _ = std::fs::remove_file(&self.pid_file);
        Ok(())
    }
}
```

**CLI integration:**

```
mc start              # foreground (dev mode)
mc start --daemon     # fork + supervise (production)
mc stop               # read PID file, send SIGTERM
mc status             # check if running, show uptime + task count
mc run "do X"         # one-shot: start, run task, exit
```

Config:
```toml
[daemon]
pid_file = "/var/run/mcclawd.pid"
max_restarts = 10
restart_delay = "2s"
```

Also generates a **systemd unit file** on `mc install-service`:

```ini
[Unit]
Description=McClawd Agent Platform
After=network.target docker.service

[Service]
Type=simple
ExecStart=/usr/local/bin/mc start
Restart=on-failure
RestartSec=5
Environment=MCCLAWD_CONFIG=/etc/mcclawd/mcclawd.toml

[Install]
WantedBy=multi-user.target
```

---

## 14. Security Hook Architecture (Future-Ready)

Every tool call passes through a `SecurityHook` trait. Phase 0 ships with an `AuditHook` (tracing-based). Future phases add real scanners.

```rust
// crates/mcclawd-core/src/hooks.rs

#[async_trait]
pub trait SecurityHook: Send + Sync {
    /// Called before a tool executes. Return Err to block.
    async fn before_tool_call(&self, call: &ToolCall) -> Result<HookAction> {
        Ok(HookAction::Allow)
    }

    /// Called after a tool returns. Can redact/modify result.
    async fn after_tool_call(&self, call: &ToolCall, result: &mut ToolResult) -> Result<()> {
        Ok(())
    }

    /// Called before LLM prompt is sent. Can scan/redact context.
    async fn before_llm_call(&self, context: &mut Context) -> Result<()> {
        Ok(())
    }

    /// Called after LLM response. Can scan for leaked secrets.
    async fn after_llm_call(&self, response: &LlmResponse) -> Result<()> {
        Ok(())
    }
}

pub enum HookAction {
    Allow,
    Block(String),          // reason
    AllowWithWarning(String),
}

// Future implementations:
// - DlpHook: regex + entropy scanning on tool inputs/outputs
// - AuditHook: append to audit log
// - TaintHook: track data provenance through tool chains
// - RateLimitHook: per-agent, per-tool rate limiting
// - SecretScanHook: gitleaks + trufflehog patterns
```

The key: `SecurityHook` is `Vec<Arc<dyn SecurityHook>>` — multiple hooks chain. Order matters. Adding DLP later means implementing the trait and pushing it onto the vec. Zero changes to agent engine.

---

## 15. Config

```toml
[server]
host = "127.0.0.1"
port = 8000

[daemon]
pid_file = "/var/run/mcclawd.pid"
max_restarts = 10
restart_delay = "2s"

[providers.anthropic]
api_key_env = "ANTHROPIC_API_KEY"     # resolved from secrets backend
model = "claude-sonnet-4-5-20250929"
priority = 1

[providers.ollama]
base_url = "http://localhost:11434"
model = "llama3.2:3b"
priority = 10

[agentgateway]
url = "http://localhost:3000"
ui_port = 15000

[secrets]
backend = "file"                       # "file" | "keychain" | "vault"

[secrets.file]
path = "config/secrets.enc"
# Master key: MCCLAWD_MASTER_KEY env var or passphrase prompt

[secrets.vault]                        # Phase 2+
address = "https://vault.internal:8200"
auth = "kubernetes"                    # or "token", "approle"
mount = "secret/mcclawd"
ttl = "1h"

[secrets.scopes]
github = ["GITHUB_TOKEN"]
notion = ["NOTION_TOKEN"]
anthropic = ["ANTHROPIC_API_KEY"]

[secrets.skills]
todoist-cli = ["TODOIST_API_KEY"]

[identity]
provider = "jwt"                       # "jwt" | "spiffe" (Phase 4+)
issuer = "mcclawd://local"
key_file = "config/signing.key"        # Ed25519; auto-generated if missing

[sandbox]
image = "mcclawd/sandbox:latest"
timeout = "10m"
memory = "512m"
cpu = 1.0
docker_socket_proxy = "tcp://localhost:2375"

[sandbox.network]
mode = "egress-only"  # none | egress-only | full
egress_allowlist = [
    "api.github.com",
    "api.openai.com",
    "registry.npmjs.org",
]

[skills]
managed_dir = "~/.mcclawd/skills"
bundled_dir = "./skills"
clawhub_api = "https://api.clawhub.com"

[workspaces]
dir = "./workspaces"
managed_dir = "~/.mcclawd/workspaces"
default = "default"

# --- Channels (§10) ---
# Full per-channel config is in §10j. Summary here for reference.

[channels.cli]
enabled = true

[channels.web]
enabled = true
bind = "127.0.0.1:8080"

[channels.telegram]
enabled = false                        # enable when ready
dm_policy = "pairing"
group_policy = "mention-only"

[channels.telegram.accounts.main]
bot_token_secret = "TELEGRAM_BOT_TOKEN"
polling = true

[channels.email]
enabled = false
dm_policy = "allowlist"

[channels.email.accounts.personal]
imap_host = "imap.gmail.com"
imap_port = 993
smtp_host = "smtp.gmail.com"
smtp_port = 587
from_address = "myagent@gmail.com"
credentials_secret = "EMAIL_OAUTH_TOKEN"
poll_mode = "idle"
poll_interval_secs = 30
folders = ["INBOX"]

[channels.debounce]
default_ms = 2000
whatsapp = 5000
slack = 1500

# Channel → agent routing
[[bindings]]
agent_id = "default"
match = { channel = "telegram", account_id = "main" }

[[bindings]]
agent_id = "default"
match = { channel = "web" }

[[bindings]]
agent_id = "default"
match = { channel = "email" }

# Channel state persistence (for Transport B/C channels)
[channels.state]
dir = "~/.mcclawd/state"              # encrypted per-channel state files

[swarm]
max_workers = 5
max_iterations_per_worker = 20

[agent]
max_iterations = 50
context_window_max_tokens = 128000

# OpenClaw compat: also reads ~/.openclaw/openclaw.json and .mcp.json
[compat]
openclaw_config = true
```

---

## 16. Docker Compose

```yaml
services:
  agentgateway:
    image: ghcr.io/agentgateway/agentgateway:latest
    volumes:
      - ./config/agentgateway.yaml:/etc/agentgateway/config.yaml:ro
    ports:
      - "3000:3000"      # MCP endpoint
      - "15000:15000"    # Gateway UI
    networks:
      - mcclawd-internal
    restart: unless-stopped

  docker-socket-proxy:
    image: tecnativa/docker-socket-proxy:latest
    environment:
      CONTAINERS: 1
      POST: 1
      IMAGES: 1
      NETWORKS: 1
      VOLUMES: 1
      EXEC: 1
      SWARM: 0
      BUILD: 0
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
    networks:
      - mcclawd-internal
    restart: unless-stopped

  # Optional: local LLM
  ollama:
    image: ollama/ollama:latest
    volumes:
      - ollama-data:/root/.ollama
    networks:
      - mcclawd-internal
    profiles: ["local-llm"]

  # --- Channel Sidecars (Transport D) ---
  # These run external runtimes that McClawd communicates with via HTTP/SSE.
  # Only start the profiles you need.

  # WhatsApp via Baileys (Node.js sidecar)
  whatsapp-sidecar:
    image: mcclawd/whatsapp-baileys:latest
    volumes:
      - whatsapp-auth:/data/auth        # Baileys session keys (encrypted at rest)
    environment:
      - HTTP_PORT=3100
    ports:
      - "3100:3100"                     # McClawd connects here
    networks:
      - mcclawd-internal
    restart: unless-stopped
    profiles: ["whatsapp"]

  # Signal via signal-cli (Java sidecar)
  signal-sidecar:
    image: mcclawd/signal-cli:latest
    volumes:
      - signal-data:/data/signal        # signal-cli registration data
    environment:
      - HTTP_PORT=3101
    ports:
      - "3101:3101"                     # SSE events + HTTP API
    networks:
      - mcclawd-internal
    restart: unless-stopped
    profiles: ["signal"]

networks:
  mcclawd-internal:
    driver: bridge
  mcclawd-egress:
    driver: bridge
    # Sandbox containers join this network for filtered egress

volumes:
  ollama-data:
  whatsapp-auth:                        # Baileys session persistence
  signal-data:                          # signal-cli registration data
```

McClawd binary runs on host during dev (`cargo run`), in a container in prod. Channel adapters for Telegram, Discord, Slack, Email, and Matrix run inside the McClawd process (native Rust crates). WhatsApp and Signal use sidecar containers (Transport D) because they require Node.js/Java runtimes — enable them with `docker compose --profile whatsapp up`.

Sandbox containers are created as siblings — they join `mcclawd-egress` (filtered) but NOT `mcclawd-internal` (where the gateway, proxy, and channel sidecars live).

---

## 17. Build Phases

### Phase 0: "One agent completes a task" (1 week)

- `mcclawd-core`: config, types, error, hook trait (AuditHook via tracing)
- `mcclawd-core/secrets`: `SecretBackend` trait + encrypted file backend
- `mcclawd-core/identity`: `IdentityProvider` trait + JWT implementation
- `mcclawd-agent`: ReAct loop (basic), workspace loader (SOUL + AGENTS + USER), context assembly
- `mcclawd-tools`: builtin tools (memory.store/recall), MCP client via rmcp
- `mcclawd-channels`: Channel trait + InboundPipeline + CLI channel
- `mcclawd-api`: CLI mode only (`mc run "do something"`)
- Provider: Anthropic via Rig, API key from secrets backend
- Workspace files loaded from `./workspaces/default/` (SOUL.md, AGENTS.md, USER.md)
- No sandbox yet — tools run in-process
- No skills yet — MCP tools only via AgentGateway
- **CLI is the only channel** — stdin/stdout with per-token streaming

**What ships in Phase 0:**
- `Secret<T>` type with zeroize
- Encrypted file secret backend (AES-256-GCM-SIV + argon2)
- JWT identity issuance for agents
- Full workspace file loading (SOUL.md + AGENTS.md + USER.md)
- AGENTS.md parser → per-agent skill assignments (structural, not just context)
- Context assembly with workspace-first priority ordering
- SecurityHook trait with AuditHook (tracing events)
- Channel trait + InboundPipeline + SessionManager + CLI channel adapter
- `mc secrets set ANTHROPIC_API_KEY` CLI for managing encrypted secrets
- `mc workspace init [name]` CLI to scaffold a new workspace with template files

**Demo:** `mc run "Use GitHub MCP to list open issues on rust-lang/rust"` → CLI channel sends InboundMessage → pipeline routes to default agent → loads workspace → agent reasons → calls MCP tool → streams response tokens to stdout. API key came from secrets.enc, not an env var.

### Phase 1: "Sandboxed + skills + daemon + web" (2 weeks)

- `mcclawd-tools/sandbox.rs`: Docker sibling container execution
- `mcclawd-tools/skills.rs`: ClawHub SKILL.md parser + loader + per-agent resolver
- `mcclawd-channel-web`: WebSocket channel (web UI + REST API)
- `mcclawd-api`: daemon supervisor + serves web channel
- `mcclawd-api/daemon.rs`: fork, monitor, restart with backoff
- Skills installed via `mc skills install <slug>`
- Per-agent skill assignment driven by AGENTS.md (skills listed under each agent)
- MCP stdio servers run inside sandbox containers
- Filesystem volumes mount into sandboxes
- Secrets delivered to containers via tmpfs at `/run/secrets/`
- Task identity tokens mounted at `/run/identity/token`
- OS keychain secret backend option
- **Web/WS channel** — full streaming via WebSocket frames, multiple concurrent sessions

**Demo:** `mc start --daemon` → stays running, auto-restarts on crash. Open `http://localhost:8080` → web chat with streaming. `mc skills install todoist-cli` → `mc run "Add a task to buy groceries"` → agent loads workspace + skill, calls CLI in sandbox with secrets from tmpfs.

### Phase 2: "Swarms + Telegram + multi-channel" (2 weeks)

- `mcclawd-swarm`: DAG orchestrator, planner/worker
- `mcclawd-tasks`: TaskManager with interactive + background modes
- `mcclawd-channel-telegram`: Telegram channel via `teloxide` (long-polling, edit-based streaming)
- Channel routing: bindings map channels → agents
- Access control: pairing, allowlist, mention-only for groups
- Session isolation per (agent, channel, peer)
- Debounce + dedup for messaging platforms
- Provider pool with fallback routing
- Multiple concurrent tasks
- Per-agent workspaces (each swarm worker gets its own SOUL.md personality)
- AGENTS.md drives swarm planning (planner reads it to decide which workers to spawn, which skills each worker loads, which model each uses)
- Vault secret backend option
- `mc install-service` for systemd unit generation
- `mc channels import-openclaw` for migration from OpenClaw

**Demo:** Configure Telegram bot token via `mc secrets set TELEGRAM_BOT_TOKEN`. Send message to bot → InboundPipeline routes via binding to agent → agent reasons → streams response via `editMessageText` in Telegram. Swarm: `POST /api/tasks {"prompt": "Research 5 competitors", "mode": "swarm"}` → planner reads AGENTS.md → DAG → streamed to Telegram or web.

### Phase 3: "Full channel ecosystem" (ongoing)

- `mcclawd-channel-discord`: Discord via `serenity` (Gateway WS, Transport C — persistent session with resume)
- `mcclawd-channel-slack`: Slack via `slack-morphism` (Socket Mode, threads)
- `mcclawd-channel-whatsapp`: WhatsApp via Baileys sidecar (Transport D — sidecar owns Baileys WS session)
- `mcclawd-channel-signal`: Signal via `signal-cli` sidecar (Transport D — SSE inbound, HTTP outbound)
- `mcclawd-channel-email`: Email via `async-imap` + `lettre` (Transport B — IMAP IDLE/poll inbound, SMTP outbound)
- `mcclawd-channel-matrix`: Matrix via `matrix-sdk` (Transport C — persistent sync loop with `since` token)
- Channel state persistence: encrypted `save_state()` / `restore_state()` for all Transport B/C channels
- Postgres persistence (sessions, turns, agent configs)
- Security hooks: DLP (regex + entropy), secret scanning, audit log to DB
- OpenClaw config compat (`~/.openclaw/openclaw.json`, `.mcp.json`, channel migration)
- Provider pool metrics + budget controls
- Composio integration for managed SaaS tools
- Hot-reload config via file watcher
- AWS/GCP/Azure secret backends
- LINE, Mattermost, Nostr channels (community contributed)

---

## 18. Key Dependencies

```toml
[workspace.dependencies]
# LLM layer
rig-core = { version = "0.31", features = ["anthropic", "openai"] }

# MCP
rmcp = "0.1"

# Async runtime
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["rt"] }

# Web framework
axum = { version = "0.8", features = ["ws"] }

# Docker
bollard = "0.18"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"

# Config
toml = "0.8"

# Concurrent state
dashmap = "6"

# Error handling
thiserror = "2"
anyhow = "1"

# Tracing
tracing = "0.1"
tracing-subscriber = "0.3"

# YAML frontmatter parsing (for SKILL.md)
gray_matter = "0.2"

# Secrets
aes-gcm-siv = "0.11"       # AES-256-GCM-SIV encryption
argon2 = "0.5"              # Key derivation
zeroize = { version = "1", features = ["derive"] }

# Identity
jsonwebtoken = "9"          # JWT issuance + verification
chrono = { version = "0.4", features = ["serde"] }

# Daemon
nix = { version = "0.29", features = ["process", "signal"] }

# Channels
lru = "0.12"                    # dedup cache
uuid = { version = "1", features = ["v4"] }

# Channel-specific (feature-gated, Phase 2+):
# teloxide = "0.13"            # Telegram
# serenity = "0.12"            # Discord
# slack-morphism = "2"         # Slack
# async-imap = "0.10"          # Email (IMAP IDLE + poll)
# lettre = "0.11"              # Email (SMTP outbound)
# matrix-sdk = "0.7"           # Matrix (persistent sync)

# Future (Phase 2+):
# vaultrs = "0.7"           # HashiCorp Vault
# keyring = "3"             # OS keychain
```

---

## 19. What This Gets You vs. OpenClaw

| Dimension | OpenClaw | McClawd |
|-----------|----------|---------|
| Codebase | 430k+ lines Node.js | ~6k lines Rust (Phase 2) |
| Binary | Docker + node_modules + npm | Single static binary |
| ClawHub skills | Native | **Full compat** (same format) |
| SOUL.md | Native | **Full compat** (Phase 0) |
| AGENTS.md | Native (multi-agent routing) | **Full compat** (Phase 0 load, Phase 2 swarm-driven) |
| USER.md | Native | **Full compat** (Phase 0) |
| MCP servers | Bare process on host | **Containerized behind gateway** |
| Multi-agent | Basic/emerging | **DAG-based swarms** |
| Tool isolation | None (in-process) | **Docker containers per task** |
| Security | Soft guardrails (historically catastrophic) | **Trait hooks, containerized, future DLP** |
| Credentials | Plaintext JSON in ~/.openclaw/ | **Encrypted at rest (AES-256-GCM-SIV), tmpfs delivery** |
| Secret scanning | None built-in | **Hook ready (gitleaks + trufflehog patterns)** |
| Identity | None (trusts everything) | **JWT per agent/task, SPIFFE upgrade path** |
| Background tasks | Single foreground session | **Multiple concurrent interactive + background** |
| Self-healing | Crashes stay crashed | **Daemon supervisor with backoff** |
| Channels | 20+ (WhatsApp, Telegram, Discord, Slack...) | **Trait-based, lazy-loaded (CLI P0, Web P1, Telegram P2, Discord/Slack/WhatsApp P3)** |
| Channel streaming | Block streaming + chunking | **Per-token streaming, per-channel chunker, edit-based for Telegram/Discord** |
| Channel secrets | Plaintext bot tokens in JSON | **SecretBackend (encrypted), tmpfs delivery, scoped per channel account** |
| Channel state | Session files in workspace dir | **Encrypted state persistence per channel (save_state/restore_state), survives restarts** |
| Channel routing | Bindings (JSON5 in openclaw.json) | **Bindings (TOML), same semantics, OpenClaw import** |
| Memory | ~500MB+ RAM | **~20MB base** |

---

## 20. Decision Log

| Decision | Rationale |
|----------|-----------|
| All workspace files in Phase 0 (SOUL + AGENTS + USER) | Trivial to implement (load markdown, inject into context). Defines the agent's full identity from day one. OpenClaw compat. AGENTS.md is informational in P0, becomes structural in P2 swarms. |
| Markdown files not TOML for agent definitions | Full OpenClaw compatibility. Users can copy workspaces between OpenClaw and McClawd. Markdown is human-friendly and LLM-friendly. TOML is for infra config, markdown is for agent personality. |
| Skills assigned per agent in AGENTS.md | Each agent gets only the skills it needs — coding agent doesn't see research skills, scout doesn't see code review. Reduces context noise, improves tool selection accuracy, and enables security isolation (agent can't use tools it shouldn't have). Default skills cover shared capabilities. |
| CLI binary named `mc` not `mcclawd` | Short, fast to type, memorable. Internal crate names stay `mcclawd-*` for namespacing. URI scheme stays `mcclawd://` for formal identification. `mc` is the user-facing surface. |
| Channels as separate crates, not one monolith | Avoids OpenClaw's #28587 bug (3MB eager load of all channel SDKs, 22k filesystem ops). Each channel is feature-gated: only compiled/loaded if configured. CLI built-in, others optional. |
| Everything is a stream (OutboundChunk) | Unified output model for all channels. CLI gets per-token stdout. Telegram gets edit-based updates. WhatsApp gets buffered-then-sent. Agent engine doesn't know which channel it's talking to. |
| Session keys include channel + peer | Forces isolation. No accidental context sharing between DM senders (OpenClaw's `dmScope: "main"` default leaked context). McClawd defaults to per-channel-peer isolation. |
| Channel secrets via SecretBackend not env vars | Bot tokens, OAuth tokens are secrets. They go through the same encrypted backend as API keys. No plaintext JSON files like OpenClaw's approach. |
| Bindings route channels to agents | Same concept as OpenClaw but in TOML. Specificity-based matching: peer > group > account > channel. Default agent catches unmatched messages. |
| WhatsApp/Signal as sidecars not native | WhatsApp (Baileys) and Signal (signal-cli) require Node.js/Java runtimes. Run them as sidecar containers, communicate via HTTP/SSE. Don't pull Node.js into a Rust binary. |
| Five transport patterns, one trait | Stateless API, long-poll, persistent connection, sidecar, local I/O — all hidden behind Channel trait. Pipeline only sees InboundMessage/OutboundChunk. Channel owns reconnection, polling, session management internally. |
| Channel state persistence (save_state/restore_state) | Discord resume tokens, WhatsApp Baileys keys, IMAP UID cursors, Matrix sync tokens must survive daemon restarts. Without this, every restart means re-QR-scanning WhatsApp, re-syncing Matrix rooms, missing emails. Framework encrypts + persists opaque bytes per channel. |
| Email as a first-class channel | IMAP IDLE for inbound (looks event-driven to pipeline), SMTP for outbound. Non-streaming (buffer then send). Threading via In-Reply-To headers. Same Channel trait, same pipeline, same bindings — just Transport B internally. |
| ChannelStartContext bundles deps | Instead of passing (inbound_tx, lifecycle) separately, pass a context struct. Allows adding secrets, state persistence callback without changing trait signature. Channels that need credentials get them from ctx.secrets, not constructor args. |
| No native Rust plugin SDK (Phase 4+) | All tools via MCP through AgentGateway. Don't design plugin API surface before knowing what agents actually need. |
| Encrypted file secrets in Phase 0 | Dev needs secrets from day one. Vault is overkill for local dev, plaintext is unacceptable. Encrypted file is the middle ground. |
| JWT identity not SPIFFE | SPIFFE needs a SPIRE server (another container, another config surface). JWT is self-contained. IdentityProvider trait means swap later. |
| Daemon supervisor built-in | ZeroClaw lesson: agents that crash at 3am need self-healing. Systemd unit is nice but the built-in supervisor works on macOS too. |
| AgentGateway for all external tools | Don't build tool discovery, RBAC, server lifecycle. Gateway handles it. One integration point. |
| Secrets via tmpfs not env vars | `docker inspect` reveals env vars. tmpfs mounts don't survive container stop. This is the Docker secrets pattern. |
