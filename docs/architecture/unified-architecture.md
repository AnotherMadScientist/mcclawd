# McClawd Unified Architecture

> Combines McClawd v5, OpenClaw ecosystem compatibility, browser-hybrid execution tiers, full DLP, and swarm orchestration into a single extensible platform.

## Design Principles

1. **Zero-code skill consumption** — Any ClawHub SKILL.md installs and runs without recompilation
2. **McPorter as the universal skill runtime** — Skills declare what they need; McPorter builds the container
3. **DLP at every boundary** — HookPipeline wraps every tool call, shared memory write, and outbound message
4. **OpenClaw-first** — Workspace files, SKILL.md format, JSON5 config, ClawHub registry are the native interface
5. **Swarm-native** — Single agent and swarm are the same code path (swarm of 1 = single agent)
6. **Execution tiers** — Same skill can run in-process, WASM, or Docker depending on what it needs

---

## 1. Crate Map

```
Cargo.toml (workspace)
├── crates/
│   ├── mcclawd-core          # Types, config, secrets, hooks, DLP, identity, skill parser, clawhub client
│   ├── mcclawd-agent         # Workspace loader, context builder, Rig agent builder, AGENTS.md parser
│   ├── mcclawd-tools         # GuardedTool wrapper, builtin tools (memory), system tools
│   ├── mcclawd-swarm         # TaskDag, SwarmPlanner, SwarmCoordinator, SharedMemory, Worker, Merger
│   ├── mcclawd-channels      # Channel trait, InboundPipeline, CLI/Web adapters
│   ├── mcclawd-channel-*     # Per-channel crates (telegram, discord, slack, whatsapp, email)
│   ├── mcclawd-tasks         # TaskManager, TaskScheduler, TaskRecord state machine
│   ├── mcclawd-runner        # Container lifecycle, sandbox config, restart logic
│   └── mcclawd-api           # `mc` binary, Axum API server, McPorter, routes
└── ui/                        # React 19 + Vite + Tailwind frontend
```

### What each crate owns

| Crate | Owns | Does NOT own |
|---|---|---|
| **mcclawd-core** | SKILL.md parser, ClawHub client/installer/cache/dep-resolver, ToolResolver, HookPipeline (DLP + audit + secret scanner + user hooks + taint trace), SecretStore, JWT identity, McpConfig, OpenClaw compat (JSON5 parser + migration) | Agent construction, MCP connections, container lifecycle |
| **mcclawd-agent** | ContextBuilder (assembles system prompt from workspace + skills), AgentEngine (configures Rig agent with tools), WorkspaceLoader, AGENTS.md parser, MCP connection management | Swarm coordination, task lifecycle, DLP patterns |
| **mcclawd-tools** | GuardedTool<T> (wraps any Rig Tool with HookPipeline before/after), MemoryStore/MemoryRecall builtins, NavigateTo/CreateTask system tools | Tool discovery, skill installation |
| **mcclawd-swarm** | TaskDag (petgraph), SwarmPlanner (LLM decomposes prompt into DAG via Rig tools), SwarmCoordinator (wave-based parallel execution), SharedMemory (Arc<DashMap>), WorkerAgent, OutputMerger (5 strategies) | Container management, DLP (delegated to GuardedTool) |
| **mcclawd-tasks** | TaskManager (in-memory task CRUD + state machine), TaskScheduler (cron-based recurring tasks), TaskRecord with skill/tool metadata | Agent execution, swarm orchestration |
| **mcclawd-runner** | Docker container lifecycle, SandboxConfig, restart with backoff | Agent construction, MCP protocol |
| **mcclawd-api** | `mc` CLI (clap), Axum HTTP server, McPorter (Docker image builder + MCP container orchestrator), all API routes, SSE streaming | Core types, agent logic |

---

## 2. The Skill Lifecycle (Zero-Code Extensibility)

This is the central design: a skill goes from ClawHub to running agent without any Rust code changes.

### 2.1 SKILL.md Format (OpenClaw Standard)

```markdown
---
name: langextract
version: 1.2.0
author: openclaw-community
description: Document extraction and analysis
---

## Description
Extracts text, tables, and structure from PDF, DOCX, XLSX, PPTX, and image files.

## MCP Tools
- langextract

## Install
pip install langextract==1.2.0

## Dependencies
- filesystem

## Context
You have access to document extraction tools. Use langextract.extract_text for
plain text extraction, langextract.extract_tables for tabular data, and
langextract.extract_structured for full document structure with headings.

## Instructions
- Always extract_structured first to understand document layout
- For large documents (>50 pages), process in chunks
- Report extraction confidence scores when available

## Examples
Extract a PDF: `langextract.extract_text({"path": "/workspace/doc.pdf"})`
Extract tables: `langextract.extract_tables({"path": "/workspace/report.xlsx"})`
```

### 2.2 Installation Flow

```
mc skills install langextract
        │
        ▼
┌─ ClawHub Client ──────────────────────────────────────────────┐
│  1. Search registry: GET /api/skills/langextract              │
│  2. Resolve version: latest compatible (semver)               │
│  3. Download SKILL.md to .mcclawd/skills/langextract/         │
│  4. Parse: SkillParser extracts all sections                  │
│  5. Resolve deps: DepResolver topological sort                │
│     └─ "filesystem" dep → already installed? skip : install   │
│  6. Record: InstalledSkillInfo with version, source, hash     │
│  7. Cache: update local catalog                               │
└───────────────────────────────────────────────────────────────┘
        │
        ▼ (No code changes. No recompile. Just a markdown file on disk.)
```

### 2.3 Runtime Activation

When a task runs, the skill activates through three systems:

```
Task: "Extract and analyze contract.pdf"
        │
        ▼
┌─ ToolResolver ────────────────────────────────────────────────┐
│  Input: selected_skills = ["langextract"]                     │
│                                                               │
│  1. Load SKILL.md from .mcclawd/skills/langextract/           │
│  2. Parse MCP Tools section → ["langextract"]                 │
│  3. Resolve deps → ["filesystem", "langextract"] (topo order) │
│  4. Match to MCP servers in config:                           │
│     langextract → mcp-langextract container                   │
│     filesystem  → mcp-filesystem container                    │
│  5. Collect install steps: ["pip install langextract==1.2.0"] │
│  6. Compute image_hash = SHA256(base + sorted install steps)  │
│  7. Aggregate skill context for system prompt                 │
│                                                               │
│  Output: ResolvedToolSet {                                    │
│    skills, required_servers, allowed_tools,                   │
│    install_steps, skill_context, image_hash                   │
│  }                                                            │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌─ McPorter ────────────────────────────────────────────────────┐
│  Input: ResolvedToolSet                                       │
│                                                               │
│  1. Check image cache: hash exists? → skip build              │
│  2. Build Docker image if needed:                             │
│     FROM mcclawd-base:latest                                  │
│     RUN pip install langextract==1.2.0  ← from install steps  │
│     (sanitized against shell injection)                       │
│  3. Ensure Docker network exists                              │
│  4. Start MCP server containers (langextract, filesystem)     │
│  5. Ensure AgentGateway running + on network                  │
│  6. Return AgentEnvironment with gateway URL + allowed tools  │
│                                                               │
│  Output: AgentEnvironment {                                   │
│    image_tag, network, gateway_url,                           │
│    allowed_tools, skill_context, model                        │
│  }                                                            │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌─ ContextBuilder ──────────────────────────────────────────────┐
│  Assembles system prompt in priority order:                   │
│                                                               │
│  1. SOUL.md        — agent identity/personality               │
│  2. IDENTITY.md    — persona identity (if present)            │
│  3. USER.md        — user preferences                         │
│  4. AGENTS.md      — role assignments + delegation rules      │
│  5. TOOLS.md       — tool usage preferences (if present)      │
│  6. Skill contexts — from ResolvedToolSet.skill_context       │
│     └─ Filtered by AGENTS.md assignments                      │
│     └─ Token budget: 50,000 chars (configurable)              │
│  7. Capabilities   — builtin tools description                │
│                                                               │
│  Output: system_prompt: String                                │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌─ AgentEngine ─────────────────────────────────────────────────┐
│  Builds Rig agent:                                            │
│                                                               │
│  client.agent(model)                                          │
│    .preamble(&system_prompt)                                  │
│    .tool(GuardedTool::new(memory_store, pipeline))            │
│    .tool(GuardedTool::new(memory_recall, pipeline))           │
│    .rmcp_tools(mcp_bundle.tools, mcp_bundle.peer)            │
│    .default_max_turns(max_turns)                              │
│    .build()                                                   │
│                                                               │
│  Every tool call goes through GuardedTool which invokes       │
│  HookPipeline.before_tool_call() and after_tool_call()        │
└───────────────────────────────────────────────────────────────┘
```

### 2.4 Why This Is Zero-Code

| What changes | What doesn't |
|---|---|
| A new SKILL.md file appears in `.mcclawd/skills/` | No Rust code |
| McPorter builds a new Docker image (cached) | No recompile |
| ContextBuilder includes the skill's context in the prompt | No config schema changes |
| ToolResolver maps skill's MCP tool prefixes to servers | No agent logic changes |
| AgentGateway discovers new MCP server's tools automatically | No tool registration code |

The **SKILL.md is the API contract**. Everything else is derived from it.

---

## 3. Execution Tiers

Skills run at different levels depending on their runtime requirements:

```
┌─────────────────────────────────────────────────────────────┐
│  Tier 0: In-Process (Rust)                                  │
│  ─────────────────────────                                  │
│  Tools: memory_store, memory_recall, navigate_to,           │
│         create_task                                         │
│  Runtime: Native Rust, compiled into mc binary              │
│  Latency: Nanoseconds                                       │
│  DLP: GuardedTool wraps each with HookPipeline              │
│  Sandbox: None (trusted code, part of mc binary)            │
│                                                             │
│  When to use: Builtins only. Not for user skills.           │
└─────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────┐
│  Tier 1: MCP over Docker (Primary skill runtime)            │
│  ───────────────────────────────────────────────            │
│  Tools: Any ClawHub skill (langextract, web-search,         │
│         filesystem, git, code-analysis, etc.)               │
│  Runtime: Docker container with supergateway (stdio→HTTP)   │
│  Latency: Milliseconds (HTTP to AgentGateway)               │
│  DLP: GuardedTool scans args before call, results after     │
│  Sandbox: Full Docker isolation (cgroups, namespaces,       │
│           resource limits, read-only rootfs)                │
│                                                             │
│  How it works:                                              │
│  mc ──rmcp──► AgentGateway ──HTTP──► supergateway ──stdio──►│
│                                       MCP server process    │
│                                                             │
│  When to use: Default for all external skills.              │
└─────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────┐
│  Tier 2: WASM MCP Servlet (Future — lightweight skills)     │
│  ──────────────────────────────────────────────────         │
│  Tools: Skills that can compile to WASM (Rust, Go, TS)      │
│  Runtime: Wasmtime/wasm32 in-process or mcp.run hosted      │
│  Latency: Microseconds (in-process), milliseconds (hosted)  │
│  DLP: Same HookPipeline, same GuardedTool                   │
│  Sandbox: WASM capability model (no ambient filesystem/net) │
│                                                             │
│  Advantages: No Docker overhead, starts instantly,          │
│  portable to browser tier                                   │
│                                                             │
│  When to use: When SKILL.md declares `runtime: wasm` and    │
│  provides a .wasm artifact.                                 │
└─────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────┐
│  Tier 3: Browser (Future — offline/edge execution)          │
│  ─────────────────────────────────────────────              │
│  Tools: JS-native tools, WASM-compiled tools                │
│  Runtime: Web Workers + OPFS + IndexedDB                    │
│  DLP: Same 109 DLP patterns compiled to JS regex            │
│  Sandbox: Browser origin sandbox                            │
│                                                             │
│  Limitations: No Docker, no native binaries, CORS limits    │
│  Use case: Offline mode, edge deployment, zero-server       │
└─────────────────────────────────────────────────────────────┘
```

### Tier Selection Logic

McPorter selects the tier automatically based on SKILL.md:

```rust
// Pseudocode — in McPorter.select_tier()
fn select_tier(skill: &LoadedSkill, config: &McclawdConfig) -> ExecutionTier {
    // Tier 0: Builtin tools (hardcoded list)
    if BUILTIN_TOOLS.contains(&skill.name) {
        return Tier::InProcess;
    }

    // Tier 2: WASM artifact available and WASM runtime enabled
    if skill.has_wasm_artifact() && config.execution.wasm_enabled {
        return Tier::Wasm;
    }

    // Tier 1: Default — Docker container
    Tier::Docker
}
```

---

## 4. DLP Architecture (Full Pipeline)

### 4.1 The HookPipeline

Every tool call passes through the `HookPipeline`, which chains security hooks:

```
Agent decides to call tool
        │
        ▼
┌─ GuardedTool<T> ──────────────────────────────────────────────┐
│                                                                │
│  ┌─ HookPipeline.before_tool_call(tool_name, args) ─────────┐│
│  │                                                           ││
│  │  1. DlpHook         — scan args for PII/secrets/keys     ││
│  │     Action: BLOCK (reject call) or REDACT (mask values)   ││
│  │                                                           ││
│  │  2. SecretScannerHook — Shannon entropy on tokens         ││
│  │     Action: WARN (log) if entropy > 4.5 bits/char         ││
│  │                                                           ││
│  │  3. UserHook(s)      — user-defined shell/HTTP hooks      ││
│  │     Trigger: before_tool_call                             ││
│  │     Action: BLOCK or ALLOW                                ││
│  │                                                           ││
│  │  4. AuditHook        — log pre-call event                 ││
│  │     Sinks: Tracing, File (JSONL), PostgreSQL              ││
│  │                                                           ││
│  │  Result: SecurityContext.was_blocked?                      ││
│  │    true  → return error to agent, tool NOT called         ││
│  │    false → proceed                                        ││
│  └───────────────────────────────────────────────────────────┘│
│                                                                │
│  ── Tool T executes (MCP call, memory op, etc.) ──            │
│                                                                │
│  ┌─ HookPipeline.after_tool_call(tool_name, result) ────────┐│
│  │                                                           ││
│  │  1. DlpHook         — scan result for PII/secrets         ││
│  │     Action: REDACT values before agent sees them          ││
│  │                                                           ││
│  │  2. SecretScannerHook — entropy check on result           ││
│  │     Action: WARN                                          ││
│  │                                                           ││
│  │  3. TaintTraceHook  — track data provenance               ││
│  │     Tags result with source tool for downstream tracing   ││
│  │                                                           ││
│  │  4. UserHook(s)      — user-defined post-call hooks       ││
│  │                                                           ││
│  │  5. AuditHook        — log post-call event with findings  ││
│  └───────────────────────────────────────────────────────────┘│
│                                                                │
│  Return (possibly redacted) result to agent                    │
└────────────────────────────────────────────────────────────────┘
```

### 4.2 DLP Pattern Categories (109 patterns)

| Category | Examples | Action |
|---|---|---|
| Cloud provider keys | AWS `AKIA...`, Azure subscription, GCP `AIza...` | Block |
| AI/ML keys | `sk-ant-...`, `sk-...` (OpenAI), HuggingFace `hf_...` | Block |
| SaaS keys | GitHub `ghp_...`, Slack `xoxb-...`, Stripe `sk_live_...` | Block |
| Package tokens | npm, PyPI, NuGet, Cargo, Hex | Block |
| Crypto keys | Private keys (RSA, EC, Ed25519), seed phrases | Block |
| Auth tokens | JWT, Bearer, Basic auth, session cookies | Block |
| PII | SSN, credit cards, IBAN, phone numbers, email | Redact |
| HIPAA | MRN, DEA numbers, NPI | Redact |
| Injection | SQL injection, command injection, prompt injection markers | Block |
| Encoding bypass | Base64-encoded secrets, hex-encoded patterns, homoglyphs | Warn |

### 4.3 DLP Scan Points in a Typical Workflow

```
User Input ──────────────────── [Prompt Sanitizer] ──► Agent receives clean input
                                 Scans for injection
                                 markers, jailbreak
                                 attempts

Agent calls tool ────────────── [before_tool_call] ──► Tool executes or BLOCKED
                                 DLP scans arguments
                                 outbound to tool

Tool returns result ─────────── [after_tool_call] ───► Agent receives (redacted) result
                                 DLP scans result
                                 coming back in

Agent writes SharedMemory ───── [DLP on set()] ─────► SharedMemory stores redacted value
                                 NEW: wrap SharedMemory
                                 writes with DLP scan

Agent sends final response ──── [Channel DLP] ──────► User receives safe output
                                 NEW: scan outbound
                                 channel messages
```

### 4.4 DLP on SharedMemory (New — Critical for Swarms)

Currently `SharedMemory.set()` stores raw values. For swarms where workers write
findings that other workers (and the LLM) read, this is a DLP gap.

```rust
// New: GuardedSharedMemory wraps SharedMemory with DLP
pub struct GuardedSharedMemory {
    inner: SharedMemory,
    pipeline: Arc<HookPipeline>,
}

impl GuardedSharedMemory {
    pub async fn set<T: Serialize>(&self, key: &str, value: T) {
        let json = serde_json::to_value(&value).unwrap();
        let json_str = json.to_string();

        // Run DLP before storing
        let ctx = SecurityContext::new_for_task("shared_memory");
        let decision = self.pipeline
            .after_tool_call("shared_memory.set", &json_str, &ctx)
            .await;

        if decision.was_blocked {
            tracing::warn!(key, "SharedMemory write blocked by DLP");
            return;
        }

        // Store the (possibly redacted) value
        let redacted: serde_json::Value = serde_json::from_str(
            &decision.redacted_text.unwrap_or(json_str)
        ).unwrap_or(json);
        self.inner.set(key, redacted);
    }

    // get() is safe — data was already DLP-scanned on write
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.inner.get(key)
    }
}
```

---

## 5. Swarm Architecture

### 5.1 Components

```
┌─────────────────────────────────────────────────────────────────┐
│                     SwarmPlanner                                 │
│                                                                 │
│  LLM agent with 3 tools:                                       │
│  • create_subtask(role, prompt, input_keys, output_key)         │
│  • add_dependency(from_id, to_id)                               │
│  • finalize_plan() → validates DAG, returns waves               │
│                                                                 │
│  Input: user prompt + available AgentRoleInfo[]                 │
│  Output: validated TaskDag                                      │
│                                                                 │
│  The planner sees:                                              │
│  - Available roles from AGENTS.md (with skills + tool lists)    │
│  - SharedMemory key naming conventions                          │
│  - Max concurrency from SwarmConfig                             │
└──────────────────────┬──────────────────────────────────────────┘
                       │ TaskDag
                       ▼
┌─────────────────────────────────────────────────────────────────┐
│                   SwarmCoordinator                               │
│                                                                 │
│  Wave-based parallel execution:                                 │
│                                                                 │
│  for wave in dag.topological_waves():                           │
│      futures = []                                               │
│      for subtask in wave:                                       │
│          sem.acquire()    // limit to max_concurrent_workers     │
│          futures.push(spawn(worker.execute(subtask, memory)))   │
│      join_all(futures)                                          │
│      // Check for failures → replan if max_replan_depth > 0     │
│                                                                 │
│  Final: OutputMerger.merge(results) → SwarmResult               │
└──────────────────────┬──────────────────────────────────────────┘
                       │
          ┌────────────┼────────────┐
          ▼            ▼            ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│  WorkerAgent │ │  WorkerAgent │ │  WorkerAgent │
│  role: coder │ │  role: rsrch │ │  role: anlys │
│              │ │              │ │              │
│  1. Read     │ │  1. Read     │ │  1. Read     │
│  input_keys  │ │  input_keys  │ │  input_keys  │
│  from shared │ │  from shared │ │  from shared │
│  memory      │ │  memory      │ │  memory      │
│              │ │              │ │              │
│  2. Build    │ │  2. Build    │ │  2. Build    │
│  Rig agent   │ │  Rig agent   │ │  Rig agent   │
│  with role's │ │  with role's │ │  with role's │
│  skills+tools│ │  skills+tools│ │  skills+tools│
│              │ │              │ │              │
│  3. Execute  │ │  3. Execute  │ │  3. Execute  │
│  (Rig loop)  │ │  (Rig loop)  │ │  (Rig loop)  │
│              │ │              │ │              │
│  4. Write    │ │  4. Write    │ │  4. Write    │
│  output_key  │ │  output_key  │ │  output_key  │
│  to shared   │ │  to shared   │ │  to shared   │
│  memory      │ │  memory      │ │  memory      │
└──────────────┘ └──────────────┘ └──────────────┘
        │               │               │
        └───────────────┼───────────────┘
                        ▼
              ┌──────────────────┐
              │  SharedMemory    │
              │  (GuardedShared  │
              │   Memory)        │
              │                  │
              │  Arc<DashMap>    │
              │  DLP on writes   │
              └──────────────────┘
```

### 5.2 AGENTS.md Drives Swarm Configuration

```markdown
# Agents

## researcher
- Skills: web-search, fetch-url
- Profile: Research
- Description: Finds and summarizes information from the web

## coder
- Skills: filesystem, langextract, code-analysis
- Profile: Coding
- Description: Reads files, extracts content, writes code

## analyst
- Skills: filesystem
- Profile: Minimal
- Description: Analyzes findings and writes reports

## Delegation
- Default: analyst
- Complex tasks: Use researcher + coder in parallel, then analyst to merge
- Research-heavy: Multiple researcher workers with different search angles
```

The `AgentsParser` extracts `AgentSpec` entries. The `SwarmPlanner` receives these
as `AgentRoleInfo[]` and uses them to assign subtasks to appropriate roles.

### 5.3 Worker → Real Rig Agent (Wiring the Placeholder)

Currently `WorkerAgent.execute()` is a placeholder. The production implementation:

```rust
impl WorkerAgent {
    pub async fn execute_live(
        &self,
        subtask: &SubtaskNode,
        shared_memory: &GuardedSharedMemory,
        config: &McclawdConfig,
        api_key: &str,
        pipeline: Arc<HookPipeline>,
    ) -> SubtaskResult {
        let start = Instant::now();

        // 1. Gather inputs from shared memory
        let inputs: HashMap<String, String> = subtask.input_keys.iter()
            .filter_map(|key| {
                shared_memory.get::<String>(key).map(|v| (key.clone(), v))
            })
            .collect();

        // 2. Look up role in AGENTS.md → get skill list
        let role_skills = config.agent_roles.get(&subtask.agent_role)
            .map(|r| r.skills.clone())
            .unwrap_or_default();

        // 3. Resolve tools for this role's skills
        let tool_set = ToolResolver::resolve(&role_skills, config).unwrap();

        // 4. McPorter ensures containers are running
        let env = McPorter::prepare_task_environment(&tool_set, config).await.unwrap();

        // 5. Build worker-specific prompt with inputs
        let worker_prompt = format!(
            "{}\n\nInputs from previous steps:\n{}",
            subtask.prompt,
            inputs.iter()
                .map(|(k, v)| format!("- {k}: {v}"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        // 6. Build Rig agent with role's skills
        let workspace = WorkspaceLoader::load_default();
        let (agent, _mem, _bundles) = AgentEngine::build_with_skill_filter(
            workspace, api_key, 10, config,
            Some(pipeline), Some(role_skills), &env.model,
        ).await.unwrap();

        // 7. Execute via Rig agent loop
        match tokio::time::timeout(self.timeout, agent.prompt(&worker_prompt)).await {
            Ok(Ok(response)) => {
                shared_memory.set(&subtask.output_key, &response).await;
                SubtaskResult {
                    subtask_id: subtask.id.clone(),
                    agent_role: subtask.agent_role.clone(),
                    output: Some(response),
                    status: SubtaskStatus::Completed,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            Ok(Err(e)) => SubtaskResult {
                subtask_id: subtask.id.clone(),
                agent_role: subtask.agent_role.clone(),
                output: None,
                status: SubtaskStatus::Failed(e.to_string()),
                duration_ms: start.elapsed().as_millis() as u64,
            },
            Err(_) => SubtaskResult {
                subtask_id: subtask.id.clone(),
                agent_role: subtask.agent_role.clone(),
                output: None,
                status: SubtaskStatus::Failed("timeout".into()),
                duration_ms: start.elapsed().as_millis() as u64,
            },
        }
    }
}
```

### 5.4 Merge Strategies

| Strategy | When to use | Implementation |
|---|---|---|
| **Concatenate** | Sequential pipeline (parse → analyze → report) | Join outputs with `---` separator |
| **LastNode** | Pipeline where final node is the deliverable | Return only sink node's output |
| **LlmSynthesis** | Complex multi-perspective analysis | LLM agent merges all outputs with synthesis prompt |
| **MajorityVote** | Redundant workers for reliability | Most common output wins |
| **Custom(prompt)** | Domain-specific merging | User-provided merge prompt sent to LLM |

### 5.5 Swarm + Single Agent Unification

A single agent task is a swarm of 1:

```rust
// mc run "analyze contract.pdf" → creates a 1-node DAG
let mut dag = TaskDag::new();
dag.add_subtask(SubtaskNode {
    id: "main".into(),
    prompt: user_prompt.into(),
    agent_role: "default".into(),  // from AGENTS.md default role
    input_keys: vec![],
    output_key: "result".into(),
});

// Same SwarmCoordinator code path, 1 wave, 1 worker
let coordinator = SwarmCoordinator::new(SwarmConfig::default());
let result = coordinator.execute(&user_prompt, &dag).await?;
```

This means all the DLP, shared memory, and merge infrastructure is always active,
even for simple tasks. Zero special-casing.

---

## 6. OpenClaw Compatibility Layer

### 6.1 What OpenClaw Defines

| Concept | OpenClaw Format | McClawd Implementation |
|---|---|---|
| Workspace config | `openclaw.json` (JSON5) | `openclaw_config.rs` parser → migrates to `mcclawd.toml` |
| MCP servers | `.mcp.json` (JSON5) | Parsed by `OpenClawMcpConfig` → maps to `McpConfig` |
| Skills | `SKILL.md` (ClawHub format) | `skill_parser.rs` — full parser with all sections |
| Workspace files | SOUL.md, AGENTS.md, USER.md, IDENTITY.md, TOOLS.md, HEARTBEAT.md | WorkspaceLoader loads 3/6 (Gap 1: add remaining 3) |
| Skill registry | ClawHub API | `clawhub/client.rs` — full client with search, download, cache |
| Skill deps | `## Dependencies` in SKILL.md | `dep_resolver.rs` — topological sort with cycle detection |
| Skill versioning | semver in SKILL.md frontmatter | `installer.rs` — version tracking, pinning, update checks |

### 6.2 Import Path

```bash
# Import existing OpenClaw workspace
mc import openclaw /path/to/openclaw-project

# What happens:
# 1. Parse openclaw.json (JSON5) → extract channels, model, skills
# 2. Parse .mcp.json (JSON5) → extract MCP server configs
# 3. Generate mcclawd.toml with equivalent config
# 4. Copy workspace files (SOUL.md, AGENTS.md, etc.)
# 5. Install referenced ClawHub skills
```

### 6.3 Remaining Gaps (from compat plan)

| Gap | Status | Priority | What's needed |
|---|---|---|---|
| Gap 1: Missing workspace files | IDENTITY.md, TOOLS.md, HEARTBEAT.md | P0 | Add parsers + context injection |
| Gap 2: User-defined hooks | Config format designed, UserHook trait exists | P1 | Wire trigger dispatch in HookPipeline |
| Gap 3: JSON5 config support | **COMPLETE** | - | Already uses `json5` crate |
| Gap 4: Skill dependency resolution | **COMPLETE** | - | DepResolver + ToolResolver integrated |
| Gap 5: ClawHub skill versioning | Partially complete | P0 | Wire `--version` CLI flag, update checks |
| Gap 6: Skill context injection | Partially complete | P0 | Token budget enforcement in ContextBuilder |

---

## 7. McPorter Deep Dive

McPorter is the bridge between SKILL.md declarations and running containers.

### 7.1 Responsibilities

```
SKILL.md ──► ToolResolver ──► McPorter ──► Running containers
                                  │
                                  ├─ Docker image build (cached by hash)
                                  ├─ Docker network management
                                  ├─ MCP server container lifecycle
                                  ├─ AgentGateway container lifecycle
                                  ├─ Tool filtering (allowed_tools set)
                                  └─ Skill context aggregation
```

### 7.2 Image Caching

McPorter computes a deterministic hash from:
- Base image tag
- Sorted, deduplicated install steps from all active skills

```
SHA256("mcclawd-base:latest" + "pip install langextract==1.2.0\npip install web-search==0.5.0")
  → "a1b2c3d4..."

Docker image: mcclawd-task:a1b2c3d4
```

Same skill combination = same hash = cached image = instant start.

### 7.3 Security Hardening (from audit)

| Issue | Fix | Status |
|---|---|---|
| Shell injection in Dockerfile RUN | Escape/validate install steps | Planned |
| Host execution fallback | Remove fallback, require Docker | Planned |
| Secrets in env vars | Use Docker secrets or tmpfs mount | Planned |
| No resource limits | SandboxConfig with memory/CPU/PID limits | Planned |
| No image digest pinning | Pin base images by SHA256 digest | Planned |

---

## 8. Data Flow: End-to-End Example

**User: "Analyze all PDFs in ~/Documents/contracts/ and write a risk report with legal references"**

```
                    User prompt
                        │
                        ▼
              ┌──────────────────┐
              │  Prompt Sanitizer │ ← Scan for injection
              └────────┬─────────┘
                       │
                       ▼
              ┌──────────────────┐
              │  SwarmPlanner     │ ← LLM decomposes into DAG
              │  (system agent)   │
              └────────┬─────────┘
                       │
                       ▼ TaskDag:
    ┌──────────────────────────────────────────────────────┐
    │  Wave 1: [parse-pdf-1, parse-pdf-2, ..., parse-pdf-N]│ ← parallel
    │  Wave 2: [analyze-contracts, analyze-amendments]      │ ← parallel
    │  Wave 3: [research-legal-refs]                        │ ← web search
    │  Wave 4: [write-report]                               │ ← synthesis
    └──────────────────────────────────────────────────────┘
                       │
                       ▼
              ┌──────────────────┐
              │ SwarmCoordinator  │
              └────────┬─────────┘
                       │
    ╔══════════════════╧══════════════════════════════════╗
    ║  Wave 1: Parse (max_concurrent_workers = 4)         ║
    ║                                                     ║
    ║  Worker[coder]          Worker[coder]               ║
    ║  Skills: [langextract]  Skills: [langextract]       ║
    ║  ┌──────────────┐       ┌──────────────┐            ║
    ║  │ MCP call:     │       │ MCP call:     │           ║
    ║  │ langextract.  │       │ langextract.  │           ║
    ║  │ extract_text  │       │ extract_text  │           ║
    ║  └──────┬───────┘       └──────┬───────┘            ║
    ║         │ [DLP scan]           │ [DLP scan]          ║
    ║         ▼                      ▼                     ║
    ║  SharedMemory:           SharedMemory:               ║
    ║  "parse:pdf1" = "..."    "parse:pdf2" = "..."        ║
    ║  (redacted)              (redacted)                  ║
    ╚═════════════════════════════════════════════════════╝
                       │
    ╔══════════════════╧══════════════════════════════════╗
    ║  Wave 2: Analyze                                    ║
    ║                                                     ║
    ║  Worker[analyst]                                    ║
    ║  Skills: [filesystem]                               ║
    ║  Reads: parse:pdf1, parse:pdf2, ... (already clean) ║
    ║  Writes: "analysis:contracts" (DLP-scanned)         ║
    ╚═════════════════════════════════════════════════════╝
                       │
    ╔══════════════════╧══════════════════════════════════╗
    ║  Wave 3: Research                                   ║
    ║                                                     ║
    ║  Worker[researcher]                                 ║
    ║  Skills: [web-search]                               ║
    ║  ┌──────────────────────────────────┐               ║
    ║  │ MCP call: web_search(query)      │               ║
    ║  │ [DLP: scan query for leaked PII] │               ║
    ║  │ If blocked → agent rephrases     │               ║
    ║  └──────────────────────────────────┘               ║
    ║  Writes: "research:legal-refs" (DLP-scanned)        ║
    ╚═════════════════════════════════════════════════════╝
                       │
    ╔══════════════════╧══════════════════════════════════╗
    ║  Wave 4: Merge                                      ║
    ║                                                     ║
    ║  Worker[analyst]                                    ║
    ║  Reads: analysis:contracts, research:legal-refs     ║
    ║  Writes: "report:final"                             ║
    ║                                                     ║
    ║  OutputMerger(LlmSynthesis) → final report          ║
    ╚═════════════════════════════════════════════════════╝
                       │
                       ▼
              ┌──────────────────┐
              │  Channel.send()   │ ← Deliver to user
              │  [Channel DLP]    │ ← Final scan
              └──────────────────┘
```

---

## 9. Shared Memory Hierarchy

### Level 1: In-Process (Current — Phase 0-2)

```rust
// Arc<DashMap<String, serde_json::Value>>
// All workers in same tokio runtime share this
let mem = GuardedSharedMemory::new(pipeline);
mem.set("key", value).await;  // DLP-scanned
let v: T = mem.get("key");    // instant
```

**Scope**: Single `mc` process. All swarm workers share it.
**Persistence**: None — lost on process exit.
**Concurrency**: Lock-free via DashMap.

### Level 2: Persistent Scratchboard (Phase 2+)

For cross-process scenarios (multiple `mc` instances, long-running swarms):

```sql
CREATE TABLE scratchboard (
    swarm_id    UUID NOT NULL,
    key         TEXT NOT NULL,
    value       JSONB NOT NULL,
    dlp_scanned BOOLEAN DEFAULT true,
    created_by  TEXT,       -- worker agent ID
    created_at  TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (swarm_id, key)
);
```

Accessed via `SharedMemory` trait with a PostgreSQL backend:

```rust
pub trait SharedMemoryBackend: Send + Sync {
    async fn set(&self, key: &str, value: serde_json::Value);
    async fn get(&self, key: &str) -> Option<serde_json::Value>;
    fn contains(&self, key: &str) -> bool;
    fn keys(&self) -> Vec<String>;
    fn snapshot(&self) -> HashMap<String, serde_json::Value>;
}

// Implementations:
// - DashMapBackend (current, in-process)
// - PostgresBackend (Phase 2+, persistent)
// - RedisBackend (Phase 3+, distributed)
```

### Level 3: Cross-Tier Sync (Phase 3+)

Browser ↔ Server via WebSocket:

```
Browser Worker                          Axum Server
     │                                       │
     │  WS: {"op":"set","key":"k","val":"v"} │
     │  ─────────────────────────────────►   │
     │                        DLP scan → store│
     │  WS: {"op":"notify","key":"k"}        │
     │  ◄─────────────────────────────────   │
```

---

## 10. Configuration

### mcclawd.toml (Native Config)

```toml
[agent]
model = "claude-haiku-4-5-20251001"
max_turns = 20

[skills]
managed_dir = ".mcclawd/skills"

[skills.pinned_versions]
langextract = "1.2.0"
web-search = ">=0.5.0"

[execution]
wasm_enabled = false           # Phase 2+
default_tier = "docker"

[sandbox]
memory_limit = "512m"
cpu_limit = "1.0"
pids_limit = 100
read_only_rootfs = true
network_mode = "internal"      # no external access by default

[security]
dlp_enabled = true
audit_sink = "file"            # "file" | "postgres" | "tracing"
audit_path = ".mcclawd/audit.jsonl"
prompt_sanitizer = true

[mcp]
gateway_url = "http://localhost:3000"

[mcp.servers.langextract]
transport = "streamable-http"
url = "http://mcp-langextract:3001"

[mcp.servers.filesystem]
transport = "streamable-http"
url = "http://mcp-filesystem:3002"

[mcp.servers.web-search]
transport = "streamable-http"
url = "http://mcp-web-search:3003"
```

### openclaw.json (Import-Compatible)

```json5
{
  // OpenClaw format — mc import openclaw converts this
  model: "claude-haiku-4-5-20251001",
  skills: ["langextract", "web-search", "filesystem"],
  channels: {
    telegram: { token: "env:TELEGRAM_TOKEN" },
    discord: { token: "env:DISCORD_TOKEN" },
  },
  mcp: ".mcp.json",
}
```

---

## 11. Security Model Summary

### Defense in Depth

```
Layer 1: Prompt Sanitizer ── blocks injection in user input
Layer 2: GuardedTool ─────── DLP + audit on every tool call
Layer 3: SharedMemory DLP ── scans inter-worker data
Layer 4: Container Sandbox ─ Docker isolation for skill code
Layer 5: Channel DLP ─────── scans outbound messages to users
Layer 6: Audit Trail ─────── every action logged (JSONL/PG)
Layer 7: Secret Store ────── AES-256-GCM-SIV, never in prompts
```

### What Never Touches the LLM

- Raw secret values (encrypted at rest, injected into tool env only)
- Unredacted PII (credit cards, SSN — replaced with `[REDACTED-*]`)
- API keys/tokens (blocked by DLP before entering agent context)
- High-entropy strings (warned, optionally blocked)

---

## 12. Implementation Plan

### Phase 1A: Wire Swarm Workers to Real Rig Agents (Current Priority)

1. **Wire `WorkerAgent.execute_live()`** — Replace placeholder with actual Rig agent construction per subtask
2. **Wire `SwarmPlanner.decompose()`** — Connect LLM planner agent with create_subtask/add_dependency/finalize_plan tools
3. **Add `GuardedSharedMemory`** — Wrap SharedMemory.set() with HookPipeline DLP scan
4. **Wire prompt sanitizer** — Call `sanitizer.rs` in ContextBuilder before agent prompt assembly
5. **Add channel-level DLP** — Scan outbound messages in Channel.send_chunk()

### Phase 1B: OpenClaw Gaps

6. **Gap 1: Add IDENTITY.md, TOOLS.md, HEARTBEAT.md** — Parser + context injection
7. **Gap 5: Complete ClawHub versioning** — `--version` flag, update check command
8. **Gap 6: Token budget enforcement** — Truncate skill context in ContextBuilder

### Phase 1C: Security Hardening

9. **Fix shell injection in McPorter Dockerfile generation** — Sanitize install steps
10. **Remove host execution fallback** — Require Docker for MCP tools
11. **Add resource limits to SandboxConfig** — Memory, CPU, PIDs
12. **Pin base images by digest** — Deterministic builds

### Phase 2: Persistent Infrastructure

13. **PostgreSQL scratchboard** — SharedMemoryBackend::Postgres
14. **User-defined hooks (Gap 2)** — Shell/HTTP hooks on tool call triggers
15. **LlmSynthesis merger** — Wire OutputMerger to actual LLM call
16. **Swarm UI** — Real-time wave progress in frontend

### Phase 3: Execution Tiers

17. **WASM tier** — Wasmtime integration for WASM-compiled skills
18. **Browser tier** — DLP patterns compiled to JS, Web Worker swarm
19. **Cross-tier shared memory** — WebSocket scratchboard sync

---

## 13. Pi / pi_agent_rust Integration

### 13.1 What Pi Brings to the Table

[Pi](https://github.com/badlogic/pi-mono) is the agent runtime powering OpenClaw. It's a minimalist
coding agent harness with a clear philosophy: the agent adapts to your workflow, not the other way
around. [pi_agent_rust](https://github.com/Dicklesworthstone/pi_agent_rust) is a from-scratch Rust
port by Jeffrey Emanuel that achieves dramatically better performance.

| Capability | Pi (TypeScript) | pi_agent_rust | McClawd (current) |
|---|---|---|---|
| **Agent loop** | Custom ReAct | Custom ReAct | Rig (delegated) |
| **Built-in tools** | 7 (read/write/edit/bash/grep/find/ls) | 7 (same) | 2 builtin + MCP |
| **Extension runtime** | Node/Bun | Embedded QuickJS (no Node needed) | N/A (MCP only) |
| **MCP support** | None (uses mcporter bridge) | None | Native (rmcp 0.13) |
| **Session persistence** | JSONL with tree branching | JSONL with tree branching | In-memory TaskManager |
| **Skill loading** | Progressive disclosure (97 chars initially) | Same | Full context at build time |
| **Performance (1M tokens)** | Baseline | 4.95x faster, 12x lower memory | N/A (Rig-managed) |
| **Extension security** | Capability-gated (tool/exec/http/session/ui) | Same + two-stage exec guards | HookPipeline DLP |

### 13.2 What McClawd Should Adopt from Pi

**1. Progressive Skill Disclosure (High Value, Low Effort)**

Pi's most important architectural insight: don't dump all skill context into the system prompt.
Instead, load ~97 chars per skill (name + description) initially. When the user's request matches
a skill, inject the full SKILL.md content dynamically.

This directly addresses McClawd's Gap 6 (token budget). Currently `ContextBuilder` loads all
active skill context at agent build time. With progressive disclosure:

```
Current (McClawd):
  System prompt = SOUL.md + USER.md + AGENTS.md + [FULL langextract context]
                  + [FULL web-search context] + [FULL filesystem context]
  = 15,000+ tokens before the user says anything

Progressive (Pi pattern):
  System prompt = SOUL.md + USER.md + AGENTS.md + skill summaries:
    "- langextract (v1.2.0): Document extraction for PDF, DOCX, XLSX"
    "- web-search (v0.5.0): Web search and URL fetching"
    "- filesystem (v2.1.0): File and directory operations"
  = 3,000 tokens

  When user says "extract this PDF" → inject full langextract context mid-conversation
```

Implementation: Add a `SkillRouter` that matches user messages against skill descriptions
and injects full context via Rig's dynamic context mechanism.

**2. Embedded QuickJS for Pi Extension Compatibility (Medium Value, Medium Effort)**

pi_agent_rust embeds QuickJS to run Pi/OpenClaw TypeScript extensions without requiring
Node.js or Bun. This means McClawd could run the entire OpenClaw extension ecosystem
(224 verified extensions) in-process.

This creates a new execution tier between Tier 0 (native Rust) and Tier 1 (Docker MCP):

```
Tier 0:   Native Rust builtins (memory_store, memory_recall)
Tier 0.5: QuickJS extensions (Pi/OpenClaw TS extensions) ← NEW
Tier 1:   Docker MCP containers (langextract, etc.)
Tier 2:   WASM MCP servlets
Tier 3:   Browser
```

Benefits:
- Zero cold-start for lightweight extensions
- No Docker overhead for simple tools
- Full OpenClaw extension compatibility
- Capability-gated (pi_agent_rust's security model)

**3. JSONL Session Persistence with Tree Branching (Medium Value, Low Effort)**

Pi's session model stores conversations as JSONL files with support for:
- Full conversation history
- Tree branching (explore multiple paths)
- Compaction (trim old context while preserving key decisions)

McClawd's `TaskManager` is currently in-memory. Adopting JSONL sessions would give:
- Crash recovery (resume tasks after restart)
- Session history for debugging
- Branch-and-merge for swarm workers (each worker is a branch)

**4. Pi's 7 Built-in Tools as Tier 0 (High Value for Single-Agent)**

Pi's read/write/edit/bash/grep/find/ls tools run in-process with zero overhead. McClawd
currently routes even filesystem operations through Docker MCP containers. For development
workflows where Docker overhead matters:

```
Option A: Keep current architecture (all tools via MCP)
  + Uniform security model (every tool goes through AgentGateway)
  + Consistent sandboxing
  - 10-50ms overhead per tool call (HTTP round-trip)

Option B: Hybrid — Pi-style builtins for dev mode, MCP for production
  + Near-zero latency for basic file operations
  + Better developer experience
  - Two code paths to maintain
  - Builtins bypass container sandbox

Recommendation: Option A for production, Option B as "dev mode" flag
```

### 13.3 Integration Strategy

Rather than replacing Rig with Pi's agent loop, McClawd should **consume pi_agent_rust
as a library** for specific capabilities:

```
┌─────────────────────────────────────────────────────────┐
│  McClawd Architecture (unchanged)                       │
│                                                         │
│  Rig agent loop → GuardedTool → HookPipeline → MCP     │
│       │                                                 │
│       │  NEW: Additional tool sources                   │
│       │                                                 │
│       ├─► QuickJS Extension Runtime (from pi_agent_rust)│
│       │   Runs: OpenClaw TS extensions in-process       │
│       │   Gated by: capability permissions              │
│       │   Wrapped by: GuardedTool (same DLP)            │
│       │                                                 │
│       ├─► Pi Builtin Tools (optional, dev mode)         │
│       │   read/write/edit/bash/grep/find/ls             │
│       │   Faster than MCP for local development         │
│       │   Wrapped by: GuardedTool (same DLP)            │
│       │                                                 │
│       └─► JSONL Session Backend (from pi_agent_rust)    │
│           Replaces: in-memory TaskManager               │
│           Adds: crash recovery, branching, compaction   │
└─────────────────────────────────────────────────────────┘
```

### 13.4 What McClawd Does NOT Adopt from Pi

| Pi Feature | Why NOT |
|---|---|
| Pi's agent loop (custom ReAct) | Rig already handles this with 20+ provider support |
| Pi's TUI (pi-tui) | McClawd has its own CLI + web UI |
| No MCP support | McClawd's MCP-first design is superior for skill isolation |
| Pi's Slack bot | McClawd has full channel architecture (5 transport patterns) |

### 13.5 OpenClaw Extension Compatibility Matrix

With QuickJS integration, McClawd would support:

| Extension Type | Runs in McClawd? | How |
|---|---|---|
| Pi TS extensions (*.ts) | Yes | QuickJS embedded runtime |
| Pi native-rust extensions (*.native.json) | Yes | Native Rust loader |
| OpenClaw skills (SKILL.md) | Yes | Already supported (skill_parser.rs) |
| OpenClaw MCP skills | Yes | Already supported (McPorter + rmcp) |
| Claude Code slash commands | No | Different extension model |
| Codex plugins | No | Different extension model |

### 13.6 Implementation Priority

| Task | Effort | Value | Phase |
|---|---|---|---|
| Progressive skill disclosure | 2 days | Very High | 1A (immediate) |
| JSONL session persistence | 3 days | High | 1B |
| QuickJS extension runtime | 1 week | High | 2 |
| Pi builtin tools (dev mode) | 3 days | Medium | 2 |
| pi_agent_rust as dependency | 2 days | Medium | 2 |

### 13.7 Dependency Decision

Two approaches:

**A. Add `pi_agent_rust` as a Cargo dependency**
```toml
[workspace.dependencies]
pi-agent-core = { git = "https://github.com/Dicklesworthstone/pi_agent_rust" }
```
- Pro: Get QuickJS runtime, session persistence, all 7 tools immediately
- Pro: Proven compatible with 224 OpenClaw extensions
- Con: Large dependency, may conflict with Rig's agent loop
- Con: pi_agent_rust may have its own opinions about agent lifecycle

**B. Cherry-pick specific components**
- Extract QuickJS binding code for extension runtime
- Port JSONL session format (it's just a file format)
- Implement progressive disclosure independently (it's an algorithm, not a library)
- Pro: No dependency conflicts, take only what's needed
- Con: More work, may drift from upstream

**Recommendation: Start with B (cherry-pick), evaluate A once pi_agent_rust stabilizes its
library API.** The most valuable pieces (progressive disclosure, JSONL sessions) are patterns,
not libraries. The QuickJS runtime is the main candidate for direct dependency.
