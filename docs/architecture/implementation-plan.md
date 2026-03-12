# Implementation Plan: Unified Architecture

> Phased plan to implement the unified architecture from `unified-architecture.md`.
> OpenClaw-native. AgentGateway-first. McClawd's 3-tier scanner (not iron-verify).
> All code runs in containers. DLP on everything.

## Phase 0: Container Runtime Abstraction (Foundation)

### 0. ContainerRuntime trait + Docker backend
**Crate**: `mcclawd-runner` (new: runtime.rs, docker.rs)
**What**: Extract current Docker logic behind a `ContainerRuntime` trait. This is the
foundation that Firecracker and WASM backends plug into later.
**Why**: McPorter currently hardcodes Docker. The trait abstraction lets us swap
Firecracker in without touching McPorter, AgentEngine, or SwarmCoordinator.
**Depends on**: Nothing
**Size**: M

```rust
pub trait ContainerRuntime: Send + Sync {
    async fn build(&self, base: &str, steps: &[String], hash: &str) -> Result<String>;
    async fn start(&self, image_id: &str, config: &SandboxConfig) -> Result<ContainerHandle>;
    async fn stop(&self, handle: &ContainerHandle) -> Result<()>;
    async fn health(&self, handle: &ContainerHandle) -> Result<bool>;
}
```

## Phase 1A: Core Wiring (Make What Exists Work Together)

### 1. Wire SwarmPlanner to LLM via Rig agent
**Crate**: `mcclawd-swarm` (planner.rs)
**What**: Replace the `todo!()` in `SwarmPlanner::decompose()` with a real Rig agent that uses
the three planner tools (create_subtask, add_dependency, finalize_plan).
**Why**: The tools exist, the DAG exists, the coordinator exists — they just aren't connected.
**Depends on**: Nothing
**Size**: S

### 2. Wire WorkerAgent to real Rig agents
**Crate**: `mcclawd-swarm` (worker.rs) + `mcclawd-agent` (engine.rs)
**What**: Replace placeholder `execute()` with `execute_live()` that builds a Rig agent
per subtask, using the role's skills from AGENTS.md.
**Why**: Workers currently echo prompts. They need to actually call LLMs and MCP tools.
**Depends on**: #1 (planner produces DAG that workers execute)
**Size**: M

### 3. GuardedSharedMemory — DLP on shared memory writes
**Crate**: `mcclawd-swarm` (shared_memory.rs)
**What**: Wrap `SharedMemory` with `HookPipeline` so every `set()` is DLP-scanned.
**Why**: Swarm workers pass data through SharedMemory. Without DLP here, PII flows
between workers unscanned.
**Depends on**: Nothing
**Size**: S

### 4. Wire prompt sanitizer into ContextBuilder
**Crate**: `mcclawd-agent` (context.rs)
**What**: Call `sanitizer.rs` on the assembled system prompt before passing to agent.
**Depends on**: Nothing
**Size**: XS

### 5. Channel-level DLP on outbound messages
**Crate**: `mcclawd-channels` (traits.rs or pipeline.rs)
**What**: DLP scan in outbound path before messages reach users.
**Depends on**: Nothing
**Size**: S

## Phase 1B: Security Hardening + OpenClaw Gaps

### 6. Fix shell injection in McPorter
**Crate**: `mcclawd-api` (server/mcp_porter.rs)
**What**: Sanitize install_steps against shell metacharacters. Critical security fix.
**Depends on**: Nothing
**Size**: XS

### 7. Remove host execution fallback
**Crate**: `mcclawd-api` (server/mcp_porter.rs)
**What**: Error when Docker/Firecracker unavailable instead of running on host.
**Depends on**: Nothing
**Size**: XS

### ~~8. iron-verify~~ (DONE — McClawd's 3-tier scanner is more comprehensive)
McClawd already has a 3-tier security scanner in `scanner.rs`:
- Tier 1: Security sidecar (POST /scan/skill)
- Tier 2: snyk-agent-scan (uvx, 120s timeout)
- Tier 3: Built-in 28-pattern static analysis (E-codes/W-codes)
No need to build iron-verify separately.

### 9. Resource limits on containers
**Crate**: `mcclawd-runner`
**What**: Enforce memory_limit, cpu_limit, pids_limit on all containers.
**Depends on**: #0 (ContainerRuntime trait)
**Size**: S

### 10. Progressive skill disclosure (from Pi)
**Crate**: `mcclawd-agent` (context.rs)
**What**: Load only name + description (~97 chars per skill) into system prompt.
Inject full SKILL.md context dynamically when user message matches.
**Why**: Cuts initial prompt from ~15K to ~3K tokens. Pi's best architectural insight.
**Depends on**: Nothing
**Size**: M

### 11. Gap 1 — Add IDENTITY.md, TOOLS.md, HEARTBEAT.md
**Crate**: `mcclawd-agent` (workspace.rs, context.rs)
**What**: Parsers for 3 missing workspace files. Wire into ContextBuilder.
**Depends on**: Nothing
**Size**: S

### 12. Gap 5 — Complete ClawHub versioning
**Crate**: `mcclawd-core` (clawhub/installer.rs), `mcclawd-api` (commands/)
**What**: `--version` flag on install, `mc skills update --check`.
**Depends on**: Nothing
**Size**: S

## Phase 2: Firecracker + WASM + Deeper Integration

### 13. Firecracker ContainerRuntime backend
**Crate**: `mcclawd-runner` (new: firecracker.rs)
**What**: Implement `ContainerRuntime` for Firecracker microVMs:
- Build ext4 rootfs from install steps (instead of Dockerfile)
- Boot microVM with minimal kernel via Firecracker API
- Connect to AgentGateway via TAP network or virtio-vsock
- Jailer integration for seccomp + cgroup enforcement
**Why**: Hardware-level isolation. No shared kernel. No container escape. 168ms boot.
**Depends on**: #0 (ContainerRuntime trait)
**Size**: L

### 14. Remote execution support
**Crate**: `mcclawd-api` (new: remote.rs), `mcclawd-runner`
**What**: `mc-remote` daemon that accepts MCP requests from local `mc` binary.
Route Tier 2 tool calls to remote AgentGateway over WireGuard/SSH tunnel.
Config: `remote_gateway = "wg://10.0.0.2:3000"`.
**Why**: Run heavy skills on OVH KS-1 (32GB RAM, KVM), lightweight on laptop.
**Depends on**: #13 (Firecracker on remote), #0 (ContainerRuntime)
**Size**: L

### 15. WASM sandbox tier (from IronClaw)
**Crate**: `mcclawd-runner` (new: wasm.rs), `mcclawd-tools` (new: wasm_tool.rs)
**What**: Wasmtime runtime implementing `ContainerRuntime` for .wasm skills.
IronClaw-style capability model: zero access by default, explicit opt-in for
http/fs/exec/secrets. Credential injection at execution boundary.
Leak detection on outbound HTTP requests.
**Depends on**: #0 (ContainerRuntime trait)
**Size**: L

### 16. LlmSynthesis merger
**Crate**: `mcclawd-swarm` (merger.rs)
**What**: Wire `MergeStrategy::LlmSynthesis` to actual Rig agent call.
**Depends on**: #1, #2
**Size**: S

### 17. JSONL session persistence (from Pi)
**Crate**: `mcclawd-tasks` (new: session.rs)
**What**: Persist conversations as JSONL with tree branching and compaction.
**Depends on**: Nothing
**Size**: M

### 18. User-defined hooks (OpenClaw Gap 2)
**Crate**: `mcclawd-core` (hooks/user_hook.rs)
**What**: Wire triggers (before_tool_call, after_tool_call, on_error) to
shell commands or HTTP calls per UserHookConfig.
**Depends on**: Nothing
**Size**: M

### 19. Swarm UI — real-time wave progress
**Crate**: `mcclawd-api` (server/), `ui/`
**What**: SSE endpoint streaming swarm progress. DAG visualization in frontend.
**Depends on**: #1, #2
**Size**: L

## Phase 3: Browser Tier + Scale

### ~~20. Browser execution tier~~ (REMOVED)
AgentGateway + Docker/Firecracker covers all use cases. No need for browser-native execution.

### 21. PostgreSQL scratchboard
**Crate**: `mcclawd-swarm` (new: pg_memory.rs)
**What**: `SharedMemoryBackend::Postgres` for cross-process swarms.
**Depends on**: #3 (GuardedSharedMemory abstraction)
**Size**: M

### ~~22. Cross-tier shared memory~~ (REMOVED — depended on browser tier)

### 23. QuickJS/WASM extension runtime for Pi TS extensions
**Crate**: `mcclawd-tools` (new: quickjs_wasm.rs)
**What**: Run 224 verified Pi/OpenClaw TypeScript extensions via QuickJS
compiled to WASM, inside the Tier 1 WASM sandbox.
**Depends on**: #15 (WASM tier)
**Size**: L

---

## Dependency Graph

```
Phase 0:
  #0 ContainerRuntime trait ──┬──► #9  Resource limits
                              ├──► #13 Firecracker backend
                              ├──► #14 Remote execution
                              └──► #15 WASM sandbox

Phase 1A (parallel):
  #1 SwarmPlanner ──► #2 Workers ──► #16 LlmSynthesis, #19 Swarm UI
  #3 GuardedSharedMemory ──► #21 PG scratchboard
  #4 Prompt sanitizer
  #5 Channel DLP

Phase 1B (parallel with 1A):
  #6  Shell injection fix
  #7  Remove host fallback
  #8  ~~iron-verify~~ (DONE — McClawd 3-tier scanner)
  #9  Resource limits (after #0)
  #10 Progressive disclosure
  #11 Workspace files
  #12 ClawHub versioning

Phase 2 (after 1A/1B):
  #13 Firecracker ──► #14 Remote execution
  #15 WASM sandbox
  #16 LlmSynthesis
  #17 JSONL sessions
  #18 User hooks
  #19 Swarm UI

Phase 3 (after Phase 2):
  #21 PG scratchboard
  #23 QuickJS/WASM extensions (after #15)
```

## Recommended Sprint Order

**Sprint 1** (foundation + critical security):
- #0 ContainerRuntime trait (M — unlocks everything)
- #6, #7 (security fixes — XS each)
- #1 SwarmPlanner wiring (S — unblocks swarm)
- #4 Prompt sanitizer (XS)

**Sprint 2** (swarm + DLP + Pi patterns):
- #2 Worker wiring (M — makes swarm work)
- #3 GuardedSharedMemory (S)
- #5 Channel DLP (S)
- #10 Progressive disclosure (M — biggest context quality win)
- ~~#8 iron-verify~~ (DONE — McClawd 3-tier scanner already built)

**Sprint 3** (OpenClaw compat + persistence):
- #9 Resource limits (S)
- #11 Workspace files (S)
- #12 ClawHub versioning (S)
- #16 LlmSynthesis merger (S)
- #17 JSONL sessions (M)

**Sprint 4** (Firecracker + WASM — the big leap):
- #13 Firecracker backend (L)
- #15 WASM sandbox (L)
- #18 User hooks (M)

**Sprint 5** (remote + UI):
- #14 Remote execution (L)
- #19 Swarm UI (L)

**Sprint 6+** (persistence + extensions):
- #21 PG scratchboard (M)
- #23 QuickJS/WASM extensions (L)

---

## What We Cherry-Pick vs Build

| Component | Source | Cherry-pick or Build? |
|---|---|---|
| DLP patterns (109) | McClawd | Already built |
| HookPipeline | McClawd | Already built |
| Swarm DAG + waves | McClawd | Already built, wire to LLM |
| MCP via AgentGateway | McClawd | Already built |
| SKILL.md parser | McClawd | Already built |
| ClawHub client | McClawd | Already built |
| Capability permissions | IronClaw | Build (inspired by, not forked) |
| 3-tier security scanner | McClawd | Already built (replaces iron-verify) |
| WASM sandbox model | IronClaw | Build with Wasmtime |
| Credential injection | IronClaw | Build (McClawd SecretStore already close) |
| Firecracker runtime | IronClaw concept + AWS Firecracker | Build on raw Firecracker API |
| Progressive disclosure | Pi | Build (algorithm, not library) |
| JSONL sessions | Pi | Build (file format, not library) |
| QuickJS extensions | pi_agent_rust | Evaluate as dependency for Phase 3 |
