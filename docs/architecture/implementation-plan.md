# Implementation Plan: Unified Architecture

> Structured for **parallel agent execution** — each workstream is independently assignable
> to a Claude Code agent with no file conflicts between concurrent agents.
>
> OpenClaw-native. AgentGateway-first. McClawd's 3-tier scanner (not iron-verify).
> All code runs in containers. DLP on everything.

---

## Status Legend

| Status | Meaning |
|--------|---------|
| DONE | Working code, tested |
| STUB | File/struct exists, placeholder logic (todo!() or mock) |
| TODO | No code exists yet |

---

## Current Status Overview

| # | Task | Status | Notes |
|---|------|--------|-------|
| 0 | ContainerRuntime trait | TODO | Docker logic lives in sandbox/container.rs, no trait yet |
| 1 | SwarmPlanner → LLM | STUB | Tools built, decompose() returns error |
| 2 | WorkerAgent → LLM | STUB | execute() echoes prompt, no real agent |
| 3 | GuardedSharedMemory | STUB | SharedMemory exists, no DLP integration |
| 4 | Prompt sanitizer | DONE | sanitizer.rs built and integrated |
| 5 | Channel-level DLP | STUB | DLP patterns exist (109), not wired to outbound channel path |
| 6 | Shell injection fix | DONE | McPorter uses proper escaping |
| 7 | Remove host fallback | DONE | All execution containerized |
| 8 | ~~iron-verify~~ | DONE | McClawd 3-tier scanner is more comprehensive |
| 9 | Resource limits | DONE | memory, cpu, pids_limit, no-new-privileges enforced |
| 10 | Progressive disclosure | DONE | Skill budget + priority ordering + truncation |
| 11 | Workspace files | DONE | All 6 files (SOUL/AGENTS/USER/IDENTITY/TOOLS/HEARTBEAT) |
| 12 | ClawHub versioning | DONE | Version pinning, .installed.json tracking |
| 13 | Firecracker backend | TODO | No code |
| 14 | Remote execution | TODO | No code |
| 15 | WASM sandbox | TODO | No code |
| 16 | LlmSynthesis merger | STUB | Falls back to concatenation, no LLM call |
| 17 | JSONL sessions | DONE | SessionStore trait + persistence |
| 18 | User-defined hooks | DONE | UserHookConfig with shell/HTTP, triggers, patterns |
| 19 | Swarm UI | STUB | WebSocket routes exist, no SSE, no DAG viz |
| 20 | ~~Browser tier~~ | REMOVED | AgentGateway covers all cases |
| 21 | PG scratchboard | DONE | pg_store integrated with swarm persistence |
| 22 | ~~Cross-tier sync~~ | REMOVED | Depended on browser tier |
| 23 | QuickJS/WASM extensions | TODO | No code |

**Remaining work: 4 STUB items + 4 TODO items = 8 tasks across 6 workstreams.**

---

## Workstream Architecture

```
                    ┌─────────────────────────────────┐
                    │     WAVE 1 (all parallel)        │
                    │                                  │
                    │  WS-A: ContainerRuntime trait     │
                    │  WS-B: Swarm LLM wiring          │
                    │  WS-C: DLP enforcement           │
                    │                                  │
                    └──────┬───────┬───────┬───────────┘
                           │       │       │
                    ┌──────▼───────▼───────▼───────────┐
                    │     WAVE 2 (all parallel)        │
                    │                                  │
                    │  WS-D: Firecracker backend       │
                    │  WS-E: WASM sandbox              │
                    │  WS-F: Swarm UI                  │
                    │                                  │
                    └──────────────────────────────────┘
```

Each workstream is a **self-contained unit of work** assignable to one agent.
No two workstreams touch the same files.

---

## Wave 1: Foundation (all 3 workstreams run in parallel)

### Workstream A: ContainerRuntime Trait Abstraction

**Agent scope**: Extract Docker logic behind a trait so Firecracker/WASM plug in later.
**Status**: TODO
**Size**: M (1–2 days)
**Depends on**: Nothing
**Blocks**: WS-D (Firecracker), WS-E (WASM)

#### Files OWNED by this agent (exclusive write access)

| File | Action |
|------|--------|
| `crates/mcclawd-runner/src/runtime.rs` | CREATE — `ContainerRuntime` trait definition |
| `crates/mcclawd-runner/src/docker.rs` | CREATE — Docker impl of `ContainerRuntime` |

#### Files READ-ONLY (do not modify, use as reference)

| File | Why |
|------|-----|
| `crates/mcclawd-api/src/sandbox/container.rs` | Current Docker logic to extract from |
| `crates/mcclawd-core/src/config.rs` | `SandboxConfig` struct definition |

#### Deliverables

1. Define `ContainerRuntime` trait in `runtime.rs`:
   ```rust
   #[async_trait]
   pub trait ContainerRuntime: Send + Sync {
       async fn build(&self, base: &str, steps: &[String], hash: &str) -> Result<String>;
       async fn start(&self, image_id: &str, config: &SandboxConfig) -> Result<ContainerHandle>;
       async fn stop(&self, handle: &ContainerHandle) -> Result<()>;
       async fn health(&self, handle: &ContainerHandle) -> Result<bool>;
   }
   ```
2. Implement `DockerRuntime` in `docker.rs` by extracting logic from `sandbox/container.rs`
3. Unit tests for `DockerRuntime` (mock Docker API or test containers)
4. Update `mcclawd-runner/Cargo.toml` if new dependencies needed

#### Acceptance criteria
- `cargo test -p mcclawd-runner` passes
- Existing `sandbox/container.rs` behavior unchanged (refactor, not rewrite)
- `ContainerHandle` type is runtime-agnostic (no Docker-specific fields exposed)

---

### Workstream B: Swarm LLM Wiring

**Agent scope**: Connect SwarmPlanner and WorkerAgent to real Rig LLM agents.
**Status**: STUB (tools exist, LLM calls missing)
**Size**: M (2–3 days)
**Depends on**: Nothing
**Blocks**: WS-F (Swarm UI)

#### Files OWNED by this agent (exclusive write access)

| File | Action |
|------|--------|
| `crates/mcclawd-swarm/src/planner.rs` | MODIFY — wire `decompose()` to Rig agent |
| `crates/mcclawd-swarm/src/worker.rs` | MODIFY — wire `execute()` to Rig agent |
| `crates/mcclawd-swarm/src/merger.rs` | MODIFY — wire `LlmSynthesis` to Rig agent |

#### Files READ-ONLY

| File | Why |
|------|-----|
| `crates/mcclawd-agent/src/engine.rs` | Rig agent builder patterns |
| `crates/mcclawd-agent/src/context.rs` | Context assembly for system prompts |
| `crates/mcclawd-swarm/src/coordinator.rs` | Orchestrator that calls planner/workers |
| `crates/mcclawd-swarm/src/tools/` | Existing planner tools (create_subtask, etc.) |

#### Deliverables

**Task B1: SwarmPlanner → LLM** (item #1)
1. Replace error return in `SwarmPlanner::decompose()` with Rig agent call
2. Agent uses three existing tools: `CreateSubtaskTool`, `AddDependencyTool`, `FinalizePlanTool`
3. System prompt instructs LLM to decompose user goal into DAG of subtasks
4. Tests: unit test with mock LLM, integration test with real decomposition

**Task B2: WorkerAgent → LLM** (item #2)
1. Replace placeholder `execute()` with `execute_live()` that builds a Rig agent per subtask
2. Each worker agent gets the subtask's role skills from AGENTS.md
3. Worker connects to MCP tools via AgentGateway for tool execution
4. Tests: unit test with mock LLM, verify tool calls are dispatched

**Task B3: LlmSynthesis merger** (item #16)
1. Wire `MergeStrategy::LlmSynthesis` in `merger.rs` to actual Rig agent call
2. Agent receives all subtask outputs and produces unified response
3. Tests: unit test with mock LLM

#### Acceptance criteria
- `cargo test -p mcclawd-swarm` passes
- `decompose()` produces a valid DAG from a natural language prompt
- Workers execute subtasks via real LLM calls (with MCP tool access)
- LlmSynthesis produces merged output (not just concatenation)

---

### Workstream C: DLP Enforcement

**Agent scope**: Wire existing DLP patterns into SharedMemory and outbound channels.
**Status**: STUB (patterns exist, enforcement gaps)
**Size**: S (1 day)
**Depends on**: Nothing
**Blocks**: Nothing

#### Files OWNED by this agent (exclusive write access)

| File | Action |
|------|--------|
| `crates/mcclawd-swarm/src/shared_memory.rs` | MODIFY — wrap with HookPipeline DLP |
| `crates/mcclawd-channels/src/traits.rs` | MODIFY — add DLP scan to outbound path |
| `crates/mcclawd-core/src/hooks/secret_tokenizer.rs` | CREATE — `{SECRET_NAME}` tokenization at host boundaries |

#### Files READ-ONLY

| File | Why |
|------|-----|
| `crates/mcclawd-core/src/hooks/dlp.rs` | DLP patterns and HookPipeline to integrate |
| `crates/mcclawd-core/src/hooks/mod.rs` | Hook types and pipeline API |
| `crates/mcclawd-core/src/secrets/mod.rs` | SecretBackend trait for resolve() |

#### Deliverables

**Task C1: GuardedSharedMemory** (item #3)
1. Wrap `SharedMemory::set()` with `HookPipeline` DLP scan
2. Reject or redact writes that contain secrets/PII
3. Tests: verify DLP blocks AWS key written to shared memory

**Task C2: Channel outbound DLP** (item #5)
1. Add DLP scan in the outbound message path in `Channel` trait or pipeline
2. Scan before messages reach users, not after
3. Tests: verify outbound message with credit card number is redacted

**Task C3: Unified redaction tokenization at trust boundaries**

All sensitive data — secrets, PII, DLP matches — is replaced with **typed tokens
carrying a human-glanceable suffix** before crossing into LLM context. A per-task
**RedactionVault** maps tokens back to original values for resolution at execution
boundaries.

**Token format**: `{TYPE:LABEL:…SUFFIX}`

| Type | Example token | What it replaces |
|------|--------------|-----------------|
| SECRET | `{SECRET:ANTHROPIC_API_KEY:…3kF9}` | API key from SecretBackend |
| SECRET | `{SECRET:GITHUB_TOKEN:…xQ2m}` | Rotated key (different suffix) |
| PII | `{PII:CREDIT_CARD:…4242}` | Credit card number |
| PII | `{PII:PHONE:…7890}` | Phone number |
| PII | `{PII:EMAIL:…@acme.com}` | Email address (domain as suffix) |
| PII | `{PII:SSN:…6789}` | Social security number |
| DLP | `{DLP:AWS_ACCESS_KEY:…WZYX}` | Detected by DLP pattern match |
| DLP | `{DLP:PRIVATE_KEY:…a7b2}` | SHA256 last 4 of key |

**Suffix rules** (last N chars of the original value, never enough to reconstruct):
- Numbers (card, phone, SSN): last 4 digits
- API keys/tokens: last 4 alphanumeric characters
- Email: `…@domain.tld`
- Private keys: last 4 hex chars of SHA256 hash
- Generic strings: last 4 characters

**Why suffixes matter**: A human reviewing the audit trail or LLM output can
immediately tell *which* card or *which* key version is involved without seeing
the raw value. The LLM can reference "the card ending in 4242" naturally.

**RedactionVault** (per-task, in-memory):
```rust
/// Per-task vault mapping redaction tokens → original values.
/// Created when a task starts, dropped when the task ends.
/// Never serialized. Never persisted. Never enters LLM context.
pub struct RedactionVault {
    /// token → original value (e.g. "{PII:CREDIT_CARD:…4242}" → "4111111111114242")
    entries: DashMap<String, RedactionEntry>,
}

pub struct RedactionEntry {
    pub original: String,       // raw value (zeroized on drop)
    pub redaction_type: RedactionType,  // Secret | Pii | Dlp
    pub label: String,          // "CREDIT_CARD", "ANTHROPIC_API_KEY"
    pub suffix: String,         // "4242", "3kF9"
    pub created_at: DateTime<Utc>,
}
```

**Dataflow with tokenization**:

```
User prompt: "charge card 4111111111114242"
       │
       ▼
┌─ RedactionTokenizer ──────────────────────────────────┐
│  1. DLP patterns detect credit card → match           │
│  2. Generate suffix: last 4 = "4242"                  │
│  3. Generate token: "{PII:CREDIT_CARD:…4242}"         │
│  4. Store in RedactionVault: token → "4111...4242"    │
│  5. Replace in text: "charge card {PII:CREDIT_CARD:…4242}" │
└───────────────────────────────────────────────────────┘
       │
       ▼
  LLM sees: "charge card {PII:CREDIT_CARD:…4242}"
  LLM responds: "I'll charge the card ending in 4242"
       │
       ▼  tool_call("stripe_charge", {card: "{PII:CREDIT_CARD:…4242}"})
       │
┌─ Execution boundary (host → MCP/container) ──────────┐
│  RedactionVault.resolve("{PII:CREDIT_CARD:…4242}")    │
│  → "4111111111114242"                                 │
│  Actual API call uses real card number                 │
└───────────────────────────────────────────────────────┘
```

**Handling key rotation / duplicates**:
- Same label, different value → different suffix → different token
- `{SECRET:ANTHROPIC_API_KEY:…3kF9}` (production) vs `{SECRET:ANTHROPIC_API_KEY:…mN7x}` (staging)
- Collision handling: if two values produce same suffix, append 2-char hash: `…4242:a3`

**Implementation**:
- `RedactionVault` in `crates/mcclawd-core/src/hooks/redaction_vault.rs` (CREATE)
  - `DashMap<String, RedactionEntry>` with `zeroize` on drop for `original` field
  - `register(redaction_type, label, original) -> String` — generates token, stores mapping
  - `resolve(token: &str) -> Option<String>` — looks up original value
  - `resolve_all(text: &str) -> String` — replaces all `{TYPE:LABEL:…SUFFIX}` tokens in text
  - `tokenize_all(text: &str, patterns: &[DlpPattern], secrets: &[(name, value)]) -> String`
    — scans text, registers all matches, returns tokenized text
- `RedactionTokenizer` as a `SecurityHook` in `crates/mcclawd-core/src/hooks/secret_tokenizer.rs` (CREATE)
  - Wraps `RedactionVault` + `DlpPattern` list
  - `before_tool_call()`: tokenizes tool arguments via vault
  - `after_tool_call()`: tokenizes tool results via vault
  - Runs **before** DlpHook in the pipeline (so DLP sees clean tokenized text)
- Wire into `HookPipeline` as first hook (before DLP, secret scanner, audit)
- Wire `RedactionVault` into `AppState` as `Arc<DashMap<TaskId, RedactionVault>>`
- Wire `resolve_all()` into execution boundaries:
  - `ContainerRuntime::start()` for tmpfs secret injection at `/run/secrets/{KEY}`
  - MCP tool call dispatch for argument substitution
  - `ContextBuilder` for pre-LLM prompt tokenization (secrets from SecretBackend)

**Audit integration**:
- `RedactionTokenizer` pushes `PendingFinding` to `SecurityContext` for each tokenization
  - `finding_type: "redaction_applied"`
  - `tag: "redaction:PII:CREDIT_CARD"` (or `"redaction:SECRET:ANTHROPIC_API_KEY"`)
  - `redacted_preview: "{PII:CREDIT_CARD:…4242}"` (the token itself, safe to store)
- These flow through the existing `AuditHook → PgAuditSink → security_events + dlp_findings` pipeline
- UI shows: "Redacted CREDIT_CARD (…4242) in tool args for stripe_charge"

Files OWNED (in addition to C1/C2 files above):

| File | Action |
|------|--------|
| `crates/mcclawd-core/src/hooks/redaction_vault.rs` | CREATE — token↔value vault |
| `crates/mcclawd-core/src/hooks/secret_tokenizer.rs` | CREATE — SecurityHook wrapper |

Tests:
- `vault.register(Pii, "CREDIT_CARD", "4111111111114242") == "{PII:CREDIT_CARD:…4242}"`
- `vault.resolve("{PII:CREDIT_CARD:…4242}") == Some("4111111111114242")`
- `vault.register(Secret, "API_KEY", "sk-abc123") == "{SECRET:API_KEY:…c123}"`
- Same label + different value → different token: `…c123` vs `…d456`
- Suffix collision: `…4242:a3` disambiguates two cards ending in 4242
- `resolve_all()` handles multiple tokens in one string
- `tokenize_all()` finds and replaces all DLP matches + known secrets in text
- `RedactionEntry.original` is zeroized on drop (no secret residue in memory)
- E2E: LLM prompt contains only tokens, container /run/secrets has real values
- E2E: audit trail shows `redaction_applied` findings with safe token previews

#### Acceptance criteria
- `cargo test -p mcclawd-swarm` passes (shared memory DLP)
- `cargo test -p mcclawd-channels` passes (channel DLP)
- `cargo test -p mcclawd-core` passes (redaction vault + tokenizer)
- No PII/secrets can flow through SharedMemory or outbound channels unscanned
- LLM context never contains raw sensitive values — only `{TYPE:LABEL:…SUFFIX}` tokens
- Tokens carry enough suffix for human identification but not value reconstruction
- Key rotation produces distinct tokens (different suffixes)
- Secret values are resolved only at host→container and host→MCP boundaries
- All tokenization events flow through existing audit pipeline as `redaction_applied` findings

---

## Wave 2: Advanced Runtimes + UI (all 3 workstreams run in parallel)

> **Prerequisite**: Wave 1 Workstream A (ContainerRuntime trait) must be complete.
> Wave 1 Workstreams B and C are NOT prerequisites for WS-D or WS-E.

### Workstream D: Firecracker Backend

**Agent scope**: Implement Firecracker microVM backend for `ContainerRuntime`.
**Status**: TODO
**Size**: L (3–5 days)
**Depends on**: WS-A (ContainerRuntime trait)
**Blocks**: Remote execution (future work)

#### Files OWNED by this agent (exclusive write access)

| File | Action |
|------|--------|
| `crates/mcclawd-runner/src/firecracker.rs` | CREATE — Firecracker `ContainerRuntime` impl |
| `crates/mcclawd-runner/Cargo.toml` | MODIFY — add Firecracker API dependencies |

#### Files READ-ONLY

| File | Why |
|------|-----|
| `crates/mcclawd-runner/src/runtime.rs` | ContainerRuntime trait to implement |
| `crates/mcclawd-runner/src/docker.rs` | Reference implementation |
| `crates/mcclawd-core/src/config.rs` | SandboxConfig fields |

#### Deliverables

1. `FirecrackerRuntime` implementing `ContainerRuntime`:
   - `build()`: Create ext4 rootfs from install steps (not Dockerfile)
   - `start()`: Boot microVM via Firecracker HTTP API with minimal kernel
   - Network: TAP interface or virtio-vsock to reach AgentGateway
   - Jailer integration for seccomp + cgroup enforcement
2. Config: `runtime = "firecracker"` in SandboxConfig
3. Tests: unit tests with mock Firecracker API
4. Feature-gated: `#[cfg(feature = "firecracker")]`

#### Acceptance criteria
- `cargo test -p mcclawd-runner --features firecracker` passes
- microVM boots in <200ms
- AgentGateway reachable from inside microVM

---

### Workstream E: WASM Sandbox

**Agent scope**: Implement Wasmtime-based sandbox for .wasm skills.
**Status**: TODO
**Size**: L (3–5 days)
**Depends on**: WS-A (ContainerRuntime trait)
**Blocks**: QuickJS extensions (future work)

#### Files OWNED by this agent (exclusive write access)

| File | Action |
|------|--------|
| `crates/mcclawd-runner/src/wasm.rs` | CREATE — WASM `ContainerRuntime` impl |
| `crates/mcclawd-tools/src/wasm_tool.rs` | CREATE — WASM tool wrapper |
| `crates/mcclawd-runner/Cargo.toml` | MODIFY — add wasmtime dependency |

#### Files READ-ONLY

| File | Why |
|------|-----|
| `crates/mcclawd-runner/src/runtime.rs` | ContainerRuntime trait to implement |
| `crates/mcclawd-core/src/secrets.rs` | SecretStore for credential injection |

#### Deliverables

1. `WasmRuntime` implementing `ContainerRuntime`:
   - Zero-access-by-default capability model (IronClaw-inspired)
   - Explicit opt-in for: http, fs, exec, secrets
   - Credential injection at execution boundary via SecretStore
   - Leak detection on outbound HTTP requests
2. `WasmTool` adapter that wraps a .wasm skill as a Rig tool
3. Tests: unit tests with sample .wasm skill
4. Feature-gated: `#[cfg(feature = "wasm")]`

#### Acceptance criteria
- `cargo test -p mcclawd-runner --features wasm` passes
- .wasm skill executes with only explicitly granted capabilities
- Credentials never visible inside WASM memory (injected at boundary)

---

### Workstream F: Swarm UI

**Agent scope**: Real-time swarm progress visualization.
**Status**: STUB (WebSocket routes exist, no SSE or DAG viz)
**Size**: L (3–5 days)
**Depends on**: WS-B (Swarm LLM wiring — need real swarm data)
**Blocks**: Nothing

#### Files OWNED by this agent (exclusive write access)

| File | Action |
|------|--------|
| `crates/mcclawd-api/src/server/swarm_sse.rs` | CREATE — SSE endpoint for swarm progress |
| `ui/packages/app/src/pages/SwarmPage.tsx` | CREATE or MODIFY — DAG visualization |
| `ui/packages/app/src/components/SwarmDAG.tsx` | CREATE — DAG graph component |

#### Files READ-ONLY

| File | Why |
|------|-----|
| `crates/mcclawd-api/src/server/swarms.rs` | Existing swarm API routes |
| `crates/mcclawd-api/src/server/ws.rs` | Existing WebSocket patterns |
| `crates/mcclawd-swarm/src/coordinator.rs` | Swarm events to stream |

#### Deliverables

1. SSE endpoint: `GET /api/swarms/:id/events` streaming wave progress
2. Event types: `wave_started`, `task_started`, `task_completed`, `task_failed`, `wave_completed`
3. React component: DAG visualization showing tasks as nodes, dependencies as edges
4. Real-time status updates (color-coded: pending/running/done/failed)

#### Acceptance criteria
- SSE endpoint streams events as swarm executes
- UI renders DAG and updates in real-time
- `cargo test -p mcclawd-api` passes
- `cd ui && pnpm test` passes

---

## Future Work (not yet scheduled)

These items are complete or deferred. No agent assignment needed now.

### Completed (no action needed)
- ~~#4 Prompt sanitizer~~ — DONE
- ~~#6 Shell injection fix~~ — DONE
- ~~#7 Remove host fallback~~ — DONE
- ~~#8 iron-verify~~ — DONE (McClawd 3-tier scanner)
- ~~#9 Resource limits~~ — DONE
- ~~#10 Progressive disclosure~~ — DONE
- ~~#11 Workspace files~~ — DONE
- ~~#12 ClawHub versioning~~ — DONE
- ~~#17 JSONL sessions~~ — DONE
- ~~#18 User-defined hooks~~ — DONE
- ~~#20 Browser tier~~ — REMOVED
- ~~#21 PG scratchboard~~ — DONE
- ~~#22 Cross-tier sync~~ — REMOVED

### Wave 3 (after Wave 2, not yet planned for agents)
- **#14 Remote execution** — `mc-remote` daemon, WireGuard/SSH tunnel (depends on WS-D)
- **#23 QuickJS/WASM extensions** — Run Pi TS extensions via QuickJS-in-WASM (depends on WS-E)

---

## Dependency Graph (remaining work only)

```
WAVE 1 (parallel — no dependencies between workstreams):
  WS-A: ContainerRuntime trait ──┬──► WS-D Firecracker (Wave 2)
                                 └──► WS-E WASM sandbox  (Wave 2)
  WS-B: Swarm LLM wiring ───────────► WS-F Swarm UI     (Wave 2)
  WS-C: DLP enforcement              (no downstream)

WAVE 2 (parallel — no dependencies between workstreams):
  WS-D: Firecracker backend ────────► #14 Remote execution (Wave 3)
  WS-E: WASM sandbox ──────────────► #23 QuickJS/WASM    (Wave 3)
  WS-F: Swarm UI                     (no downstream)
```

## Agent Assignment Summary

| Wave | Workstream | Agent ID | Size | Status |
|------|-----------|----------|------|--------|
| 1 | A: ContainerRuntime trait | `agent-ws-a` | M | TODO |
| 1 | B: Swarm LLM wiring | `agent-ws-b` | M | STUB→BUILT |
| 1 | C: DLP enforcement | `agent-ws-c` | S | STUB→BUILT |
| 2 | D: Firecracker backend | `agent-ws-d` | L | TODO |
| 2 | E: WASM sandbox | `agent-ws-e` | L | TODO |
| 2 | F: Swarm UI | `agent-ws-f` | L | STUB→BUILT |

**Wave 1**: 3 agents in parallel, ~2 days, zero file conflicts.
**Wave 2**: 3 agents in parallel, ~4 days, zero file conflicts.
**Total**: 6 agents, 2 waves, ~6 days wall-clock.

---

## File Ownership Matrix (conflict prevention)

Every file modified in this plan is owned by exactly one workstream.
No two agents write to the same file.

```
crates/mcclawd-runner/
  src/runtime.rs          → WS-A (CREATE)
  src/docker.rs           → WS-A (CREATE)
  src/firecracker.rs      → WS-D (CREATE)
  src/wasm.rs             → WS-E (CREATE)
  Cargo.toml              → WS-A (Wave 1), then WS-D or WS-E (Wave 2, sequential)

crates/mcclawd-swarm/
  src/planner.rs          → WS-B
  src/worker.rs           → WS-B
  src/merger.rs           → WS-B
  src/shared_memory.rs    → WS-C

crates/mcclawd-channels/
  src/traits.rs           → WS-C

crates/mcclawd-core/
  src/hooks/redaction_vault.rs    → WS-C (CREATE)
  src/hooks/secret_tokenizer.rs   → WS-C (CREATE)

crates/mcclawd-api/
  src/server/swarm_sse.rs → WS-F (CREATE)

crates/mcclawd-tools/
  src/wasm_tool.rs        → WS-E (CREATE)

ui/packages/app/
  src/pages/SwarmPage.tsx → WS-F
  src/components/SwarmDAG.tsx → WS-F (CREATE)
```

---

## What We Cherry-Pick vs Build

| Component | Source | Status |
|---|---|---|
| DLP patterns (109) | McClawd | DONE |
| HookPipeline | McClawd | DONE |
| Swarm DAG + waves | McClawd | DONE (structure), wire to LLM (WS-B) |
| MCP via AgentGateway | McClawd | DONE |
| SKILL.md parser | McClawd | DONE |
| ClawHub client | McClawd | DONE |
| 3-tier security scanner | McClawd | DONE (replaces iron-verify) |
| Progressive disclosure | Pi-inspired | DONE |
| JSONL sessions | Pi-inspired | DONE |
| User-defined hooks | OpenClaw | DONE |
| ContainerRuntime trait | New | WS-A |
| Firecracker runtime | IronClaw concept + AWS | WS-D |
| WASM sandbox model | IronClaw-inspired | WS-E |
| Capability permissions | IronClaw-inspired | WS-E |
| Credential injection | IronClaw-inspired | WS-E (SecretStore already close) |
| QuickJS extensions | pi_agent_rust | Wave 3 (evaluate) |
