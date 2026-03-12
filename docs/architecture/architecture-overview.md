# McClawd Architecture Overview

> Docker-first. OpenClaw compatible. DLP on everything. Swarm-native.
> Abstract WASM/Firecracker later. Pluggable memory. Self-improving skills.

---

## What We Cherry-Pick and Why

### From OpenClaw (ecosystem compatibility)
- **SKILL.md format** — 5,700 skills on ClawHub. Our native skill format.
- **Workspace files** — SOUL.md, AGENTS.md, USER.md (+ IDENTITY.md, TOOLS.md, HEARTBEAT.md)
- **JSON5 config** — openclaw.json / .mcp.json import path
- **ClawHub registry** — skill search, download, versioning, dependency resolution
- **mcporter concept** — skills declare install steps, runtime builds containers automatically

### From McClawd v5 (what we already built)
- **Rig agent loop** — 20+ LLM providers, built-in tool calling, no manual ReAct
- **MCP via AgentGateway** — skills expose MCP servers, AgentGateway routes tool calls
- **HookPipeline DLP** — 109 patterns (cloud keys, PII, HIPAA, injection), audit trail
- **GuardedTool<T>** — wraps every tool with DLP before/after
- **Swarm DAG** — petgraph-based TaskDag with topological wave scheduling
- **SecretStore** — AES-256-GCM-SIV encrypted, never touches LLM context
- **Channel architecture** — 5 transport patterns behind uniform trait

### From IronClaw (security patterns — build, don't fork)
- **iron-verify** — static analysis on SKILL.md install steps before running anything
- **Capability-based permissions** — skills declare what they need (http, fs, exec, secrets)
- **Credential injection at execution boundary** — secrets go to container, never to LLM
- **Leak detection** — scan outbound HTTP for data exfiltration

### From Pi/pi_agent_rust (efficiency patterns)
- **Progressive skill disclosure** — 97-char summaries in prompt, full context on demand
- **JSONL sessions** — crash recovery, tree branching, compaction
- **QuickJS for OpenClaw TS extensions** — run 224 verified extensions without Node.js (Phase 2+)

### What we DON'T take
| Skipped | From | Why |
|---|---|---|
| Custom ReAct loop | Pi, IronClaw | Rig handles it better with 20+ providers |
| NEAR AI default | IronClaw | We use Rig multi-provider |
| PostgreSQL-only storage | IronClaw | Pluggable backends instead |
| eval() in Worker | OpenBrowserClaw | Security risk |
| No DLP | OpenBrowserClaw | DLP is mandatory |
| TypeScript runtime | OpenClaw | We're Rust-native |
| Permissive security | OpenClaw | DLP on everything |
| Pi's TUI | Pi | We have CLI + web UI |
| No MCP support | Pi | MCP-first is better for isolation |

---

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              USER                                           │
│                    CLI / Web UI / Telegram / etc                             │
└──────────────────────────────┬──────────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         mc binary (host)                                     │
│                                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │   Channel    │  │   TaskMgr    │  │  SecretStore │  │  Config/CLI   │  │
│  │  (inbound/   │  │  (lifecycle  │  │  (AES-256-   │  │  (clap +      │  │
│  │   outbound)  │  │   + sched)   │  │   GCM-SIV)   │  │   mcclawd.toml│  │
│  └──────┬───────┘  └──────┬───────┘  └──────────────┘  └───────────────┘  │
│         │                  │                                                │
│         ▼                  ▼                                                │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    Prompt Sanitizer                                  │   │
│  │              (injection detection on all input)                      │   │
│  └──────────────────────────┬──────────────────────────────────────────┘   │
│                              │                                              │
│                    ┌─────────┴─────────┐                                    │
│                    │  Single task?      │                                    │
│                    │  or Swarm?         │                                    │
│                    └────┬─────────┬─────┘                                   │
│                         │         │                                          │
│            ┌────────────┘         └────────────┐                            │
│            ▼                                   ▼                            │
│  ┌──────────────────┐              ┌──────────────────────────────────┐    │
│  │  AgentEngine     │              │  SwarmPlanner                     │    │
│  │  (single agent)  │              │  (LLM decomposes prompt into DAG)│    │
│  │                  │              │  Tools: create_subtask,           │    │
│  │  = swarm of 1    │              │  add_dependency, finalize_plan    │    │
│  └────────┬─────────┘              └───────────────┬──────────────────┘    │
│           │                                        │                        │
│           │                                        ▼                        │
│           │                        ┌──────────────────────────────────┐    │
│           │                        │  SwarmCoordinator                 │    │
│           │                        │  (wave-based parallel execution)  │    │
│           │                        │                                   │    │
│           │                        │  Wave 1: [A, B] ──► parallel     │    │
│           │                        │  Wave 2: [C]    ──► after A,B    │    │
│           │                        │  Wave 3: [D]    ──► after C      │    │
│           │                        └───────┬──────────────────────────┘    │
│           │                                │                                │
│           │          ┌─────────────────────┼─────────────────────┐          │
│           ▼          ▼                     ▼                     ▼          │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                       WorkerAgent(s)                                  │  │
│  │                                                                      │  │
│  │  Each worker:                                                        │  │
│  │  1. Reads input_keys from SharedMemory                               │  │
│  │  2. Resolves skills for its role (from AGENTS.md)                    │  │
│  │  3. Builds Rig agent with role-specific tools                        │  │
│  │  4. Executes via Rig agent loop                                      │  │
│  │  5. Writes output_key to SharedMemory                                │  │
│  │                                                                      │  │
│  │  Every tool call wrapped in GuardedTool:                             │  │
│  │  ┌─────────────────────────────────────────────────────────────┐    │  │
│  │  │  HookPipeline                                               │    │  │
│  │  │  before: DLP scan args → SecretScanner → UserHooks → Audit │    │  │
│  │  │  ── tool executes (MCP call via AgentGateway) ──            │    │  │
│  │  │  after:  DLP scan result → TaintTrace → UserHooks → Audit  │    │  │
│  │  └─────────────────────────────────────────────────────────────┘    │  │
│  └──────────────────────────┬───────────────────────────────────────────┘  │
│                              │                                              │
│                              ▼                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │  GuardedSharedMemory                                                  │  │
│  │  DLP on every write │ DashMap (now) │ PostgreSQL (when needed)        │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                              │                                              │
│                              ▼                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │  OutputMerger                                                         │  │
│  │  Concatenate │ LastNode │ MajorityVote │ LlmSynthesis │ Custom       │  │
│  └──────────────────────────┬───────────────────────────────────────────┘  │
│                              │                                              │
│                              ▼                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │  Channel DLP (outbound scan) ──► User receives safe output            │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
                               │
             rmcp (MCP protocol over HTTP)
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Docker Network (isolated)                                 │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │  AgentGateway                                                         │  │
│  │  (routes MCP tool calls to correct container)                         │  │
│  └──────┬──────────────────┬──────────────────┬─────────────────────────┘  │
│         │                  │                  │                              │
│         ▼                  ▼                  ▼                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                      │
│  │ MCP Server   │  │ MCP Server   │  │ MCP Server   │  ... (per skill)     │
│  │ langextract  │  │ filesystem   │  │ web-search   │                      │
│  │              │  │              │  │              │                      │
│  │ supergateway │  │ supergateway │  │ supergateway │                      │
│  │ (stdio→HTTP) │  │ (stdio→HTTP) │  │ (stdio→HTTP) │                      │
│  └──────────────┘  └──────────────┘  └──────────────┘                      │
│                                                                             │
│  Each container built by McPorter from SKILL.md install steps               │
│  Cached by SHA256(base_image + sorted_install_steps)                        │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Data Flow: Skill Install to Execution

```
1. INSTALL
   mc skills install langextract
   │
   ├──► ClawHub API: search, resolve version, download SKILL.md
   ├──► iron-verify: static analysis on install steps (shell injection? root? unsafe URLs?)
   ├──► Save to .mcclawd/skills/langextract/SKILL.md
   └──► Done. No code change. No recompile.

2. RUN
   mc run "extract contract.pdf and summarize"
   │
   ├──► ToolResolver: reads SKILL.md, resolves deps, computes image hash
   │    Output: ResolvedToolSet { skills, servers, tools, install_steps, hash }
   │
   ├──► McPorter: builds Docker image if not cached, starts MCP containers
   │    ┌─────────────────────────────────────────────────────┐
   │    │ FROM mcclawd-base:latest                            │
   │    │ RUN pip install langextract==1.2.0  (sanitized!)    │
   │    │ → image tag: mcclawd-task:<sha256>                  │
   │    └─────────────────────────────────────────────────────┘
   │    Starts: mcp-langextract container, mcp-filesystem container
   │    Ensures: AgentGateway running and connected to both
   │
   ├──► ContextBuilder: assembles system prompt
   │    SOUL.md + USER.md + AGENTS.md + skill summaries (progressive disclosure)
   │
   ├──► AgentEngine: builds Rig agent
   │    .preamble(system_prompt)
   │    .tool(GuardedTool::new(memory_store, hook_pipeline))
   │    .tool(GuardedTool::new(memory_recall, hook_pipeline))
   │    .rmcp_tools(mcp_tools, mcp_peer)   ← tools from AgentGateway
   │    .build()
   │
   └──► Agent loop (Rig manages):
        User: "extract contract.pdf and summarize"
        │
        ├─ Agent thinks → calls langextract.extract_text
        │  ├─ GuardedTool: DLP scan args (check for secrets/PII)
        │  ├─ rmcp → AgentGateway → mcp-langextract container
        │  ├─ Container runs extraction
        │  ├─ Result returns via MCP
        │  └─ GuardedTool: DLP scan result (redact any PII found)
        │
        ├─ Agent thinks → uses extracted text to write summary
        │
        └─ Final response → Channel DLP scan → User
```

---

## Data Flow: Swarm Execution

```
mc run --swarm "Analyze all contracts in ~/Documents and write risk report with legal refs"
│
├──► Prompt Sanitizer: check for injection
│
├──► SwarmPlanner (LLM agent with planning tools):
│    Sees: available roles from AGENTS.md, their skills
│    Creates TaskDag:
│
│    ┌─ Wave 1 ─────────────────────────────────────────────┐
│    │  parse-pdf-1 [coder, langextract]                     │
│    │  parse-pdf-2 [coder, langextract]     ← parallel      │
│    │  parse-pdf-3 [coder, langextract]                     │
│    └───────────────────────────────────────────────────────┘
│                          │ all write to SharedMemory
│    ┌─ Wave 2 ─────────────────────────────────────────────┐
│    │  analyze-contracts [analyst, filesystem]               │
│    │  Reads: parse:pdf1, parse:pdf2, parse:pdf3            │
│    │  Writes: analysis:contracts                           │
│    └───────────────────────────────────────────────────────┘
│                          │
│    ┌─ Wave 3 ─────────────────────────────────────────────┐
│    │  research-legal [researcher, web-search]              │
│    │  Reads: analysis:contracts (knows what to search for) │
│    │  Writes: research:legal-refs                          │
│    └───────────────────────────────────────────────────────┘
│                          │
│    ┌─ Wave 4 ─────────────────────────────────────────────┐
│    │  write-report [analyst]                               │
│    │  Reads: analysis:contracts, research:legal-refs       │
│    │  Writes: report:final                                 │
│    └───────────────────────────────────────────────────────┘
│
├──► SwarmCoordinator: executes waves
│    Each wave: spawn workers in parallel (tokio::spawn)
│    Each worker: builds its own Rig agent with role-specific skills
│    All tool calls: GuardedTool → DLP scan → MCP → DLP scan
│    All memory writes: GuardedSharedMemory → DLP scan before store
│
├──► OutputMerger: combines final results
│    Strategy chosen by planner (default: LlmSynthesis for complex tasks)
│
└──► Channel DLP → User receives report
```

---

## Memory Model

Two layers. That's it.

```
┌─────────────────────────────────────────────────────────────────┐
│  GuardedSharedMemory                                             │
│  (DLP scan on every write, transparent read)                     │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  SharedMemoryBackend trait                                  │ │
│  │                                                             │ │
│  │  async fn set(&self, key: &str, value: Value);             │ │
│  │  async fn get(&self, key: &str) -> Option<Value>;          │ │
│  │  fn keys(&self) -> Vec<String>;                            │ │
│  │  fn snapshot(&self) -> HashMap<String, Value>;             │ │
│  └────────────────────────────────────────────────────────────┘ │
│           │                    │                                  │
│           ▼                    ▼                                  │
│  ┌────────────────┐  ┌────────────────┐                         │
│  │  DashMap        │  │  PostgreSQL     │                         │
│  │  (default)      │  │  (persistent)   │                         │
│  │                 │  │                 │                         │
│  │  In-memory      │  │  JSONB storage  │                         │
│  │  Fast, simple   │  │  Crash recovery │                         │
│  │  Lost on exit   │  │  Cross-session  │                         │
│  └────────────────┘  └────────────────┘                         │
└─────────────────────────────────────────────────────────────────┘

DashMap:    what we have now. Works for single-process swarms.
PostgreSQL: add when we need persistence. Uses existing database_url config.

Key naming convention for swarms:
  "{step}:{subtask_id}"  →  "parse:pdf1", "analysis:contracts", "report:final"
```

---

## Self-Improving Skills (JSONL ↔ SharedMemory)

Two storage systems that feed each other:

- **SharedMemory** — live working state during execution (DashMap or PG)
- **JSONL session logs** — append-only record of every execution, persisted to disk

They link bidirectionally:

```
┌─ JSONL → SharedMemory (session start) ──────────────────────────┐
│                                                                  │
│  On task start, SessionLoader scans past JSONL logs for this     │
│  skill and loads accumulated learnings into SharedMemory:        │
│                                                                  │
│  session_2024_03_10.jsonl:                                       │
│    {"tool":"langextract.extract_text","status":"fail",           │
│     "error":"scanned PDF, no text layer","duration_ms":1200}     │
│    {"tool":"langextract.extract_structured","status":"ok",       │
│     "note":"OCR mode worked on scanned PDF","duration_ms":3400}  │
│                                                                  │
│  → SharedMemory.set("skill:langextract:hints",                   │
│      "For scanned PDFs use extract_structured with OCR mode.     │
│       extract_text fails on documents without text layers.")     │
│                                                                  │
│  ContextBuilder reads these hints and injects them into the      │
│  agent's system prompt. Agent executes better on first try.      │
└──────────────────────────────────────────────────────────────────┘

┌─ SharedMemory → JSONL (during execution) ────────────────────────┐
│                                                                   │
│  Every tool call result is appended to the session JSONL:         │
│                                                                   │
│  Agent calls langextract.extract_text                             │
│  → GuardedTool logs to JSONL: tool, args, result, duration, ok/fail│
│  → If agent retries with different approach, that's logged too    │
│  → Swarm worker outputs logged with subtask_id and wave           │
│                                                                   │
│  End of session: SessionCompactor summarizes patterns:            │
│  "langextract: 5 calls, 4 success, 1 fail (scanned PDF).        │
│   Best approach: extract_structured for unknown doc types."       │
│  → Appended as summary entry in JSONL                             │
│  → Available for next session's SharedMemory hydration            │
└───────────────────────────────────────────────────────────────────┘

The loop:
  Past JSONL → hydrate SharedMemory → agent uses hints →
  executes → logs to JSONL → next session reads those logs → ...

Skills themselves never change. The agent gets smarter about
USING them through accumulated execution history.
```

JSONL files live at `.mcclawd/sessions/{session_id}.jsonl`. One file per task/swarm.
Compaction runs at session end to keep file sizes manageable.

---

## DLP Scan Points

Every boundary is scanned. No exceptions.

```
                     ┌─── 1. Prompt Sanitizer ──────────────────────┐
                     │  User input → injection detection             │
                     └──────────────────────────────────────────────┘
                                        │
                                        ▼
                     ┌─── 2. GuardedTool (before) ──────────────────┐
                     │  Tool args → DLP 109 patterns + entropy      │
                     │  BLOCK if secrets found, REDACT if PII       │
                     └──────────────────────────────────────────────┘
                                        │
                                        ▼ (tool executes in container)
                                        │
                     ┌─── 3. GuardedTool (after) ───────────────────┐
                     │  Tool result → DLP scan + taint tracking     │
                     │  REDACT any PII before agent sees it          │
                     └──────────────────────────────────────────────┘
                                        │
                                        ▼
                     ┌─── 4. SharedMemory DLP ──────────────────────┐
                     │  Every write DLP-scanned before storing       │
                     │  Workers can't pass raw PII to each other     │
                     └──────────────────────────────────────────────┘
                                        │
                                        ▼
                     ┌─── 5. Channel DLP (outbound) ────────────────┐
                     │  Final response → DLP scan before user sees   │
                     │  Last line of defense                         │
                     └──────────────────────────────────────────────┘
```

---

## Crate Map

```
Cargo.toml (workspace)
├── crates/
│   ├── mcclawd-core        # Config, secrets, DLP (109 patterns), hooks, SKILL.md parser,
│   │                        # ClawHub client, ToolResolver, identity (JWT), iron-verify
│   │
│   ├── mcclawd-agent       # ContextBuilder (system prompt assembly), AgentEngine (Rig builder),
│   │                        # WorkspaceLoader, AGENTS.md parser, progressive disclosure
│   │
│   ├── mcclawd-tools       # GuardedTool<T> wrapper, MemoryStore/Recall builtins,
│   │                        # NavigateTo/CreateTask system tools
│   │
│   ├── mcclawd-swarm       # TaskDag (petgraph), SwarmPlanner (LLM plans DAG),
│   │                        # SwarmCoordinator (wave execution), WorkerAgent,
│   │                        # GuardedSharedMemory, OutputMerger (5 strategies)
│   │
│   ├── mcclawd-channels    # Channel trait, InboundPipeline, CLI/Web/Telegram adapters
│   │
│   ├── mcclawd-tasks       # TaskManager (state machine), TaskScheduler (cron),
│   │                        # JSONL session persistence (Phase 1+)
│   │
│   ├── mcclawd-runner      # ContainerRuntime trait, DockerRuntime (now),
│   │                        # IronBox/WASM/e2b (later). SandboxConfig, restart logic
│   │
│   └── mcclawd-api         # mc binary (clap CLI), Axum API server, McPorter
│                            # (Docker image builder + MCP container orchestrator)
│
└── ui/                      # React 19 + Vite + Tailwind frontend
```

### ContainerRuntime trait (abstraction point)

```rust
/// Docker now. IronBox/WASM/e2b later. McPorter doesn't care which.
pub trait ContainerRuntime: Send + Sync {
    async fn build(&self, base: &str, install_steps: &[String], hash: &str) -> Result<String>;
    async fn start(&self, image_id: &str, config: &SandboxConfig) -> Result<ContainerHandle>;
    async fn stop(&self, handle: &ContainerHandle) -> Result<()>;
    async fn health(&self, handle: &ContainerHandle) -> Result<bool>;
}

// Phase 0-1: DockerRuntime only
// Phase 2+:  IronBoxRuntime (Firecracker), WasmRuntime, E2bCloudRuntime
```

---

## What's Built vs What's Next

| Component | Status | Source |
|---|---|---|
| Rig agent loop + tool calling | Built | McClawd v5 |
| MCP via AgentGateway + rmcp | Built | McClawd v5 |
| HookPipeline + 109 DLP patterns | Built | McClawd v5 |
| GuardedTool<T> wrapper | Built | McClawd v5 |
| SKILL.md parser | Built | McClawd v5 / OpenClaw |
| ClawHub client + installer + cache | Built | McClawd v5 / OpenClaw |
| ToolResolver (skill→MCP mapping) | Built | McClawd v5 |
| TaskDag + topological waves | Built | McClawd v5 |
| SharedMemory (DashMap) | Built | McClawd v5 |
| SwarmPlanner (3 tools defined) | Built, unwired | McClawd v5 |
| WorkerAgent | Built, placeholder | McClawd v5 |
| McPorter (Docker image builder) | Built, needs hardening | McClawd v5 |
| SecretStore (AES-256-GCM-SIV) | Built | McClawd v5 |
| Prompt sanitizer | Built, unwired | McClawd v5 |
| ContainerRuntime trait | Built (Docker only) | McClawd v5 |
| **GuardedSharedMemory** | **Next** | New (DLP on memory writes) |
| **Wire SwarmPlanner to LLM** | **Next** | McClawd v5 wiring |
| **Wire WorkerAgent to Rig** | **Next** | McClawd v5 wiring |
| **Progressive disclosure** | **Next** | Pi pattern |
| **iron-verify** | **Next** | IronClaw pattern |
| **Channel DLP** | **Next** | New |
| **Shell injection fix** | **Next** | Security fix |
| PostgreSQL memory backend | When needed | Persistence for SharedMemory |
| IronBox/Firecracker runtime | Phase 2+ | IronBox |
| WASM sandbox | Phase 2+ | IronClaw pattern |
