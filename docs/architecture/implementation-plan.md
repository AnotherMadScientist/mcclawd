# Implementation Plan: Unified Architecture

> Phased plan to implement the unified architecture from `unified-architecture.md`.
> Each task is sized, ordered by dependency, and tagged with the crate it touches.

## Phase 1A: Core Wiring (Immediate — Make What Exists Work Together)

### 1. Wire SwarmPlanner to LLM via Rig agent
**Crate**: `mcclawd-swarm` (planner.rs)
**What**: Replace the `todo!()` in `SwarmPlanner::decompose()` with a real Rig agent that uses
the three planner tools (create_subtask, add_dependency, finalize_plan).
**Why**: The tools exist, the DAG exists, the coordinator exists — they just aren't connected.
**Depends on**: Nothing
**Size**: S (the tools are already implemented, just need to build the agent)

```rust
// In SwarmPlanner::decompose():
let state: PlannerState = Arc::new(Mutex::new(TaskDag::new()));
let client = anthropic::Client::new(&self.api_key)?;
let agent = client
    .agent(self.model.as_deref().unwrap_or("claude-haiku-4-5-20251001"))
    .preamble(&planner_system_prompt(roles))
    .tool(CreateSubtaskTool::new(state.clone()))
    .tool(AddDependencyTool::new(state.clone()))
    .tool(FinalizePlanTool::new(state.clone()))
    .default_max_turns(10)
    .build();

agent.prompt(prompt).await?;
let dag = Arc::try_unwrap(state).unwrap().into_inner();
dag.validate()?;
Ok(dag)
```

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
**Why**: Swarm workers pass data through SharedMemory. Without DLP here, a tool could
return PII that gets stored unscanned and read by another worker into its LLM context.
**Depends on**: Nothing (can be done in parallel with #1/#2)
**Size**: S

### 4. Wire prompt sanitizer into ContextBuilder
**Crate**: `mcclawd-agent` (context.rs)
**What**: Call `sanitizer.rs` on the assembled system prompt before it's passed to the agent.
The sanitizer already exists — it just isn't called.
**Why**: Without this, prompt injection markers in workspace files could affect agent behavior.
**Depends on**: Nothing
**Size**: XS

### 5. Channel-level DLP on outbound messages
**Crate**: `mcclawd-channels` (traits.rs or pipeline.rs)
**What**: Add a DLP scan in the outbound path before messages reach the user.
**Why**: Defense-in-depth. Even if all tool-level DLP works, the LLM could hallucinate
realistic-looking PII in its response.
**Depends on**: Nothing
**Size**: S

## Phase 1B: OpenClaw Compatibility Gaps

### 6. Gap 1 — Add IDENTITY.md, TOOLS.md, HEARTBEAT.md
**Crate**: `mcclawd-agent` (workspace.rs, context.rs)
**What**: Add parsers for the 3 missing workspace files. Wire into ContextBuilder priority order.
**Depends on**: Nothing
**Size**: S

### 7. Gap 5 — Complete ClawHub versioning
**Crate**: `mcclawd-core` (clawhub/installer.rs), `mcclawd-api` (commands/)
**What**: Add `--version` flag to `mc skills install`, wire `mc skills update --check`.
**Depends on**: Nothing
**Size**: S

### 8. Gap 6 — Progressive skill disclosure
**Crate**: `mcclawd-agent` (context.rs)
**What**: Replace full skill context loading with summary-first approach.
Load only name + description (~97 chars per skill) into system prompt.
Inject full context dynamically when user message matches a skill.
**Depends on**: Nothing
**Size**: M (need SkillRouter matching logic)

## Phase 1C: Security Hardening (from audit)

### 9. Fix shell injection in McPorter Dockerfile generation
**Crate**: `mcclawd-api` (server/mcp_porter.rs)
**What**: Sanitize install_steps before interpolating into Dockerfile RUN commands.
Reject steps containing shell metacharacters (`;`, `&&`, `|`, `` ` ``, `$()`).
**Depends on**: Nothing
**Size**: XS (critical security fix)

### 10. Remove host execution fallback
**Crate**: `mcclawd-api` (server/mcp_porter.rs)
**What**: When Docker is unavailable, return an error instead of falling back to host execution.
**Depends on**: Nothing
**Size**: XS (critical security fix)

### 11. Add resource limits to SandboxConfig
**Crate**: `mcclawd-runner` or `mcclawd-api`
**What**: Enforce memory_limit, cpu_limit, pids_limit on Docker containers.
**Depends on**: Nothing
**Size**: S

## Phase 2: Deeper Integration

### 12. LlmSynthesis merger — wire to real LLM
**Crate**: `mcclawd-swarm` (merger.rs)
**What**: Replace the concatenation placeholder in `MergeStrategy::LlmSynthesis` with
an actual Rig agent call that synthesizes subtask outputs into a coherent final response.
**Depends on**: #1, #2 (need working swarm first)
**Size**: S

### 13. JSONL session persistence
**Crate**: `mcclawd-tasks` (new: session.rs)
**What**: Persist task conversations as JSONL files with tree branching support.
Replace in-memory TaskManager with crash-recoverable persistence.
**Depends on**: Nothing (can start anytime)
**Size**: M

### 14. QuickJS extension runtime (Pi compatibility)
**Crate**: `mcclawd-tools` (new: quickjs_runtime.rs)
**What**: Embed QuickJS to run OpenClaw/Pi TypeScript extensions in-process.
Wrap as Rig Tool behind GuardedTool for DLP coverage.
**Depends on**: Nothing technically, but lower priority
**Size**: L

### 15. User-defined hooks (Gap 2)
**Crate**: `mcclawd-core` (hooks/user_hook.rs)
**What**: Wire UserHookConfig triggers (before_tool_call, after_tool_call, on_error)
to shell commands or HTTP calls.
**Depends on**: Nothing
**Size**: M

### 16. Swarm UI — real-time wave progress
**Crate**: `mcclawd-api` (server/), `ui/`
**What**: SSE endpoint streaming swarm wave progress. Frontend renders DAG visualization
with per-worker status (pending/running/completed/failed).
**Depends on**: #1, #2 (need working swarm)
**Size**: L

## Phase 3: Execution Tiers & Scale

### 17. WASM execution tier
**Crate**: `mcclawd-tools` (new: wasm_runtime.rs)
**What**: Wasmtime integration for skills that provide .wasm artifacts.
**Depends on**: Architecture validation from Phase 1-2
**Size**: L

### 18. PostgreSQL scratchboard
**Crate**: `mcclawd-swarm` (new: pg_memory.rs), `mcclawd-core` (persistence/)
**What**: SharedMemoryBackend::Postgres for cross-process and long-running swarms.
**Depends on**: #3 (GuardedSharedMemory trait abstraction)
**Size**: M

### 19. Cross-tier shared memory (WebSocket)
**Crate**: `mcclawd-api` (server/), `ui/`
**What**: WebSocket pub/sub for browser ↔ server scratchboard sync.
**Depends on**: #18 (PostgreSQL backend)
**Size**: L

---

## Dependency Graph

```
Phase 1A (parallel start):
  #1 SwarmPlanner wiring ──► #2 Worker wiring ──► #12 LlmSynthesis merger
  #3 GuardedSharedMemory    (independent)          ──► #18 PG scratchboard
  #4 Prompt sanitizer       (independent)
  #5 Channel DLP            (independent)

Phase 1B (parallel with 1A):
  #6 Workspace files        (independent)
  #7 ClawHub versioning     (independent)
  #8 Progressive disclosure (independent)

Phase 1C (parallel with 1A/1B):
  #9  Shell injection fix   (independent, critical)
  #10 Remove host fallback  (independent, critical)
  #11 Resource limits       (independent)

Phase 2 (after 1A):
  #12 LlmSynthesis         (after #1, #2)
  #13 JSONL sessions        (independent)
  #14 QuickJS runtime       (independent)
  #15 User hooks            (independent)
  #16 Swarm UI              (after #1, #2)

Phase 3 (after Phase 2):
  #17 WASM tier             (independent)
  #18 PG scratchboard       (after #3)
  #19 Cross-tier sync       (after #18)
```

## Recommended Execution Order (What to Build First)

**Sprint 1** (highest impact, many can be parallel):
- #9, #10 (security — XS, do immediately)
- #1 (SwarmPlanner wiring — S, unblocks #2 and #12)
- #3 (GuardedSharedMemory — S, unblocks swarm DLP)
- #4 (Prompt sanitizer wiring — XS)
- #8 (Progressive disclosure — M, biggest context quality improvement)

**Sprint 2**:
- #2 (Worker wiring — M, makes swarm actually work)
- #5 (Channel DLP — S)
- #6 (Workspace files — S)
- #11 (Resource limits — S)

**Sprint 3**:
- #12 (LlmSynthesis — S)
- #7 (ClawHub versioning — S)
- #13 (JSONL sessions — M)

**Sprint 4+**:
- #14, #15, #16 (QuickJS, user hooks, swarm UI)
- #17, #18, #19 (WASM, PG scratchboard, cross-tier)
