# Phase 2 Design: Swarm Coordination

> McClawd v5 — Multi-agent swarm architecture for coordinated task execution.

**Status:** Design
**Depends on:** Phase 0 (complete), Phase 1 (sandbox + skills + daemon + web channel)
**New crate:** `mcclawd-swarm`
**Estimated scope:** ~8 new files, ~2000 lines of Rust

---

## 1. Overview

Phase 2 adds the ability for multiple agents to collaborate on a single complex task. A **swarm coordinator** (itself an LLM-powered agent) decomposes user prompts into a DAG of subtasks, assigns each to a specialist agent (from AGENTS.md), executes them with bounded concurrency inside sandboxed containers, and merges their outputs into a final result.

### Design Principles

1. **Planner is an agent** — Task decomposition uses the same Rig agent infrastructure as workers. The planner has tools for creating/linking subtasks but no domain tools.
2. **DAG, not chain** — Subtasks form a directed acyclic graph with explicit dependencies. Independent subtasks run in parallel.
3. **Shared memory, not message passing** — Workers communicate through a typed shared memory store (`Arc<DashMap>`), not point-to-point channels. This avoids deadlocks and simplifies the programming model.
4. **Fail-fast with retry** — Individual worker failures trigger retry with backoff. If a worker exhausts retries, the coordinator can re-plan the failed subtree or abort the swarm.
5. **AGENTS.md is the roster** — Agent roles, skills, and delegation rules come from the existing `AgentsConfig` parser. No new config format.
6. **Each worker gets its own sandbox** — Workers run in separate Docker containers with skill-specific tooling, sharing nothing except the DashMap.

---

## 2. Crate Structure

```
crates/mcclawd-swarm/
├── Cargo.toml
└── src/
    ├── lib.rs              # Public API: SwarmCoordinator, SwarmConfig
    ├── coordinator.rs      # Orchestrates plan → execute → merge cycle
    ├── planner.rs          # LLM-powered task decomposition agent
    ├── dag.rs              # TaskDag, DagNode, topological wave iterator
    ├── worker.rs           # WorkerAgent wrapper (sandbox + agent engine)
    ├── shared_memory.rs    # Typed shared memory (Arc<DashMap> + namespaces)
    ├── merger.rs           # Output aggregation / consensus strategies
    └── error.rs            # SwarmError types
```

### Dependencies

```toml
[dependencies]
mcclawd-core = { path = "../mcclawd-core" }
mcclawd-agent = { path = "../mcclawd-agent" }
mcclawd-tools = { path = "../mcclawd-tools" }
mcclawd-tasks = { path = "../mcclawd-tasks" }
mcclawd-channels = { path = "../mcclawd-channels" }
dashmap = "6"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
thiserror = "2"
tracing = "0.1"
petgraph = "0.7"          # DAG representation + topological sort
```

---

## 3. Core Types

### 3a. Swarm Identity

```rust
// crates/mcclawd-core/src/types.rs (extend existing)

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SwarmId(pub String);

impl SwarmId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubtaskId(pub String);

impl SubtaskId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}
```

### 3b. Swarm Configuration

```rust
// crates/mcclawd-swarm/src/lib.rs

/// Top-level swarm configuration, typically derived from workspace + CLI flags.
#[derive(Debug, Clone)]
pub struct SwarmConfig {
    /// Maximum workers running concurrently (bounded by Docker resources).
    pub max_concurrent_workers: usize,
    /// Maximum depth of re-planning on failure (prevents infinite loops).
    pub max_replan_depth: u32,
    /// Per-worker timeout (the coordinator kills a worker that exceeds this).
    pub worker_timeout: Duration,
    /// Model override for the planner agent (defaults to workspace default).
    pub planner_model: Option<String>,
    /// Whether to stream intermediate worker outputs to the user channel.
    pub stream_intermediate: bool,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            max_concurrent_workers: 4,
            max_replan_depth: 2,
            worker_timeout: Duration::from_secs(300),
            planner_model: None,
            stream_intermediate: false,
        }
    }
}
```

### 3c. Task DAG

```rust
// crates/mcclawd-swarm/src/dag.rs

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::toposort;

/// A single node in the task DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskSpec {
    pub id: SubtaskId,
    /// Which agent role handles this (matches AgentSpec.id from AGENTS.md).
    pub agent_role: String,
    /// The prompt/instruction for this subtask.
    pub prompt: String,
    /// Keys in shared memory this subtask needs as input.
    pub input_keys: Vec<String>,
    /// Key in shared memory where this subtask writes its output.
    pub output_key: String,
    /// Estimated complexity (used for scheduling heuristics).
    pub estimated_turns: Option<u32>,
}

/// The complete execution plan: a DAG of subtasks with dependency edges.
pub struct TaskDag {
    graph: DiGraph<SubtaskSpec, ()>,
    /// Map from SubtaskId to graph node index for O(1) lookup.
    index_map: HashMap<SubtaskId, NodeIndex>,
}

impl TaskDag {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index_map: HashMap::new(),
        }
    }

    /// Add a subtask node. Returns the node index.
    pub fn add_subtask(&mut self, spec: SubtaskSpec) -> NodeIndex {
        let id = spec.id.clone();
        let idx = self.graph.add_node(spec);
        self.index_map.insert(id, idx);
        idx
    }

    /// Add a dependency edge: `from` must complete before `to` can start.
    pub fn add_dependency(&mut self, from: &SubtaskId, to: &SubtaskId) -> Result<()> {
        let from_idx = self.index_map.get(from)
            .ok_or(SwarmError::UnknownSubtask(from.clone()))?;
        let to_idx = self.index_map.get(to)
            .ok_or(SwarmError::UnknownSubtask(to.clone()))?;
        self.graph.add_edge(*from_idx, *to_idx, ());
        Ok(())
    }

    /// Validate no cycles exist in the DAG.
    pub fn validate(&self) -> Result<()> {
        toposort(&self.graph, None)
            .map_err(|_| SwarmError::CyclicDependency)?;
        Ok(())
    }

    /// Return subtasks grouped into waves for parallel execution.
    /// Wave 0 has no dependencies, wave 1 depends only on wave 0, etc.
    pub fn topological_waves(&self) -> Vec<Vec<&SubtaskSpec>> {
        // Kahn's algorithm variant that groups nodes by depth level.
        // Each wave can run fully in parallel.
        let mut in_degree: HashMap<NodeIndex, usize> = HashMap::new();
        for idx in self.graph.node_indices() {
            in_degree.insert(idx, self.graph.neighbors_directed(idx, petgraph::Incoming).count());
        }

        let mut waves = vec![];
        let mut remaining: HashSet<NodeIndex> = self.graph.node_indices().collect();

        while !remaining.is_empty() {
            let wave: Vec<NodeIndex> = remaining.iter()
                .filter(|idx| in_degree[idx] == 0)
                .cloned()
                .collect();

            if wave.is_empty() {
                break; // cycle detected (should not happen after validate())
            }

            waves.push(wave.iter().map(|idx| &self.graph[*idx]).collect());

            for idx in &wave {
                remaining.remove(idx);
                for neighbor in self.graph.neighbors(*idx) {
                    if let Some(deg) = in_degree.get_mut(&neighbor) {
                        *deg -= 1;
                    }
                }
            }
        }

        waves
    }

    /// Total number of subtasks.
    pub fn len(&self) -> usize {
        self.graph.node_count()
    }
}
```

---

## 4. Swarm Coordinator

The coordinator is the top-level entry point. It owns the full lifecycle: plan, execute waves, handle failures, merge results.

```rust
// crates/mcclawd-swarm/src/coordinator.rs

pub struct SwarmCoordinator {
    config: SwarmConfig,
    planner: SwarmPlanner,
    agents_config: AgentsConfig,
    workspace: Workspace,
    shared_memory: SharedMemory,
}

/// Result of a completed swarm execution.
pub struct SwarmResult {
    pub swarm_id: SwarmId,
    pub final_output: String,
    pub subtask_results: HashMap<SubtaskId, SubtaskResult>,
    pub total_duration: Duration,
    pub total_llm_turns: u32,
}

pub struct SubtaskResult {
    pub subtask_id: SubtaskId,
    pub agent_role: String,
    pub status: SubtaskStatus,
    pub output: Option<String>,
    pub duration: Duration,
    pub llm_turns: u32,
}

#[derive(Debug, Clone)]
pub enum SubtaskStatus {
    Completed,
    Failed(String),
    Skipped { reason: String },
}

impl SwarmCoordinator {
    pub fn new(
        config: SwarmConfig,
        agents_config: AgentsConfig,
        workspace: Workspace,
        api_key: String,
    ) -> Self {
        Self {
            planner: SwarmPlanner::new(config.planner_model.clone(), api_key.clone()),
            config,
            agents_config,
            workspace,
            shared_memory: SharedMemory::new(),
        }
    }

    /// Main entry point: decompose, execute, merge.
    pub async fn run(
        &self,
        task_prompt: &str,
        output_tx: Option<broadcast::Sender<OutboundChunk>>,
    ) -> Result<SwarmResult> {
        let swarm_id = SwarmId::new();
        let start = Instant::now();

        // --- Phase 1: Planning ---
        // The planner agent decomposes the prompt into a TaskDag.
        self.emit(&output_tx, OutboundChunk::TextBlock(
            format!("[swarm:{}] Planning task decomposition...", swarm_id)
        )).await;

        let dag = self.planner.decompose(
            task_prompt,
            &self.agents_config,
            &self.shared_memory,
        ).await?;
        dag.validate()?;

        self.emit(&output_tx, OutboundChunk::TextBlock(
            format!("[swarm:{}] Plan: {} subtasks in {} waves",
                swarm_id, dag.len(), dag.topological_waves().len())
        )).await;

        // Store the original prompt in shared memory for workers to reference.
        self.shared_memory.set("_swarm_prompt", task_prompt.to_string());

        // --- Phase 2: Wave Execution ---
        let mut all_results: HashMap<SubtaskId, SubtaskResult> = HashMap::new();
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent_workers));

        for (wave_idx, wave) in dag.topological_waves().iter().enumerate() {
            self.emit(&output_tx, OutboundChunk::TextBlock(
                format!("[swarm:{}] Executing wave {}/{} ({} subtasks)",
                    swarm_id, wave_idx + 1, dag.topological_waves().len(), wave.len())
            )).await;

            let mut handles = vec![];

            for spec in wave {
                let permit = semaphore.clone().acquire_owned().await?;
                let worker = self.build_worker(spec)?;
                let mem = self.shared_memory.clone();
                let timeout = self.config.worker_timeout;
                let tx = output_tx.clone();
                let spec_clone = (*spec).clone();

                handles.push(tokio::spawn(async move {
                    let result = tokio::time::timeout(
                        timeout,
                        worker.execute(&spec_clone, &mem, tx.as_ref()),
                    ).await;

                    drop(permit); // release concurrency slot

                    match result {
                        Ok(Ok(r)) => r,
                        Ok(Err(e)) => SubtaskResult {
                            subtask_id: spec_clone.id.clone(),
                            agent_role: spec_clone.agent_role.clone(),
                            status: SubtaskStatus::Failed(e.to_string()),
                            output: None,
                            duration: Duration::ZERO,
                            llm_turns: 0,
                        },
                        Err(_) => SubtaskResult {
                            subtask_id: spec_clone.id.clone(),
                            agent_role: spec_clone.agent_role.clone(),
                            status: SubtaskStatus::Failed("Worker timeout".into()),
                            output: None,
                            duration: timeout,
                            llm_turns: 0,
                        },
                    }
                }));
            }

            // Collect wave results
            for handle in handles {
                let result = handle.await.map_err(|e| SwarmError::JoinError(e.to_string()))?;

                // Handle failures: retry or re-plan
                if let SubtaskStatus::Failed(ref err) = result.status {
                    tracing::warn!(
                        subtask = %result.subtask_id,
                        role = %result.agent_role,
                        "Subtask failed: {err}"
                    );

                    if let Some(retried) = self.retry_subtask(&result, &dag).await? {
                        all_results.insert(retried.subtask_id.clone(), retried);
                        continue;
                    }
                    // If retry also fails, check if we can re-plan
                    // (see Section 8: Failure Handling)
                }

                all_results.insert(result.subtask_id.clone(), result);
            }
        }

        // --- Phase 3: Merge ---
        let final_output = self.merge(task_prompt, &all_results, &output_tx).await?;

        Ok(SwarmResult {
            swarm_id,
            final_output,
            subtask_results: all_results,
            total_duration: start.elapsed(),
            total_llm_turns: 0, // summed from subtask results
        })
    }

    /// Build a WorkerAgent for a given subtask spec.
    fn build_worker(&self, spec: &SubtaskSpec) -> Result<WorkerAgent> {
        let agent_spec = self.agents_config.agents.iter()
            .find(|a| a.id == spec.agent_role)
            .ok_or_else(|| SwarmError::UnknownRole(spec.agent_role.clone()))?;

        Ok(WorkerAgent::new(
            agent_spec.clone(),
            self.workspace.clone(),
        ))
    }
}
```

---

## 5. Planner Agent (Task Decomposition)

The planner is itself a Rig agent with specialized tools for building the DAG. It does NOT have domain tools (no exec, no file I/O) — its only job is to produce a plan.

```rust
// crates/mcclawd-swarm/src/planner.rs

/// Planner agent: decomposes a user prompt into a TaskDag.
///
/// The planner is an LLM agent with these tools:
/// - `create_subtask(role, prompt, input_keys, output_key)` -> SubtaskId
/// - `add_dependency(from_subtask, to_subtask)` -> ()
/// - `list_roles()` -> Vec<AgentRoleInfo>
/// - `finalize_plan()` -> TaskDag
///
/// The planner's system prompt includes:
/// - Available agent roles (from AGENTS.md) with their specialties and skills
/// - Guidelines for decomposition (granularity, parallelism hints)
/// - Examples of good DAG structures
pub struct SwarmPlanner {
    model: Option<String>,
    api_key: String,
}

/// Information about an available agent role, injected into planner context.
#[derive(Debug, Clone, Serialize)]
pub struct AgentRoleInfo {
    pub id: String,
    pub specialty: Option<String>,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub delegate_when: Option<String>,
}

impl SwarmPlanner {
    pub fn new(model: Option<String>, api_key: String) -> Self {
        Self { model, api_key }
    }

    /// Decompose a user prompt into a TaskDag using the planner agent.
    pub async fn decompose(
        &self,
        prompt: &str,
        agents_config: &AgentsConfig,
        _shared_memory: &SharedMemory,
    ) -> Result<TaskDag> {
        // 1. Build role info for context
        let roles: Vec<AgentRoleInfo> = agents_config.agents.iter()
            .map(|a| AgentRoleInfo {
                id: a.id.clone(),
                specialty: a.specialty.clone(),
                tools: a.tools.clone(),
                skills: a.skills.clone(),
                delegate_when: a.delegate_when.clone(),
            })
            .collect();

        // 2. Build planner system prompt
        let system_prompt = self.build_planner_prompt(&roles);

        // 3. Create planner agent with DAG-building tools
        //    Tools are implemented as Rig tools that mutate a shared TaskDag.
        //    The planner calls them via function calling, then we extract the DAG.
        let dag_builder = Arc::new(Mutex::new(TaskDag::new()));

        // Register tools: create_subtask, add_dependency, list_roles, finalize_plan
        // (Each tool closure captures Arc<Mutex<TaskDag>> and Arc<Vec<AgentRoleInfo>>)

        // 4. Run planner agent
        //    agent.prompt(user_prompt).max_turns(10).await

        // 5. Extract and validate the built DAG
        let dag = Arc::try_unwrap(dag_builder)
            .map_err(|_| SwarmError::PlannerFailed("Could not extract DAG".into()))?
            .into_inner();

        dag.validate()?;
        Ok(dag)
    }

    fn build_planner_prompt(&self, roles: &[AgentRoleInfo]) -> String {
        format!(r#"You are a task planner for a multi-agent system.
Your job is to decompose a complex task into subtasks that can be assigned to specialist agents.

## Available Agent Roles
{role_descriptions}

## Guidelines
1. Each subtask should be self-contained and completable by a single agent.
2. Maximize parallelism: independent subtasks should have no dependency edges.
3. Use input_keys/output_key to wire data flow through shared memory.
4. The output_key of one subtask becomes the input_key of downstream subtasks.
5. Keep subtasks focused — prefer more small subtasks over fewer large ones.
6. Always include a final "merge" or "review" subtask that synthesizes results.
7. Match agent roles to subtask requirements based on their specialty and tools.

## Tools
- create_subtask(agent_role, prompt, input_keys, output_key) — create a new subtask
- add_dependency(from_subtask_id, to_subtask_id) — from must complete before to starts
- list_roles() — show available agent roles and their capabilities
- finalize_plan() — signal that the plan is complete

## Example
Task: "Research top 5 competitors and write a comparison report"

1. create_subtask("research", "Research competitor A", [], "research_a")
2. create_subtask("research", "Research competitor B", [], "research_b")
   ... (3 more research subtasks, all independent)
3. add_dependency(subtask_1, subtask_6)  // research_a -> synthesize
   ... (wire all research -> synthesize)
4. create_subtask("coding", "Write comparison report from research data",
   ["research_a", "research_b", ...], "final_report")
5. finalize_plan()
"#,
            role_descriptions = roles.iter()
                .map(|r| format!("- **{}**: {} | Tools: {:?} | Skills: {:?}",
                    r.id,
                    r.specialty.as_deref().unwrap_or("general"),
                    r.tools,
                    r.skills))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}
```

### Planner Tools (Rig Tool Implementations)

```rust
/// Tool: create_subtask
/// Called by the planner agent to add a node to the DAG.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSubtaskInput {
    pub agent_role: String,
    pub prompt: String,
    pub input_keys: Vec<String>,
    pub output_key: String,
}

/// Tool: add_dependency
#[derive(Debug, Serialize, Deserialize)]
pub struct AddDependencyInput {
    pub from_subtask_id: String,
    pub to_subtask_id: String,
}

/// Tool: finalize_plan
/// No input — signals the planner is done building the DAG.
#[derive(Debug, Serialize, Deserialize)]
pub struct FinalizePlanInput {}
```

Each tool is a `rig::tool::Tool` implementation that mutates the shared `Arc<Mutex<TaskDag>>`.

---

## 6. Worker Agent

Each worker is an isolated agent instance that executes a single subtask. Workers are built from `AgentSpec` (parsed from AGENTS.md) and run inside their own sandbox container.

```rust
// crates/mcclawd-swarm/src/worker.rs

/// A worker agent that executes a single subtask within a swarm.
pub struct WorkerAgent {
    agent_spec: AgentSpec,
    workspace: Workspace,
}

impl WorkerAgent {
    pub fn new(agent_spec: AgentSpec, workspace: Workspace) -> Self {
        Self { agent_spec, workspace }
    }

    /// Execute a subtask, reading dependencies from shared memory
    /// and writing results back.
    pub async fn execute(
        &self,
        spec: &SubtaskSpec,
        memory: &SharedMemory,
        output_tx: Option<&broadcast::Sender<OutboundChunk>>,
    ) -> Result<SubtaskResult> {
        let start = Instant::now();

        // 1. Gather dependency data from shared memory
        let context_data = self.gather_inputs(spec, memory)?;

        // 2. Build augmented prompt with dependency context
        let augmented_prompt = self.build_worker_prompt(spec, &context_data);

        // 3. Build a Rig agent using AgentEngine with this agent's config
        //    - Model from AgentSpec (or workspace default)
        //    - Tools filtered by AgentSpec.tools
        //    - Skills from AgentSpec.skills
        //    - MCP tools from AgentGateway (filtered by skill)
        let (agent, _memory_store, _mcp_bundles) = AgentEngine::build(
            &self.workspace,
            &self.resolve_api_key()?,
            &McclawdConfig::default(), // TODO: pass real config
        ).await?;

        // 4. Run the agent
        let response = agent.prompt(&augmented_prompt)
            .max_turns(spec.estimated_turns.unwrap_or(20) as u64)
            .await?;

        // 5. Write output to shared memory
        memory.set(&spec.output_key, response.clone());

        // 6. Stream result if requested
        if let Some(tx) = output_tx {
            let _ = tx.send(OutboundChunk::TextBlock(
                format!("[worker:{}:{}] {}", spec.agent_role, spec.id, &response[..100.min(response.len())])
            ));
        }

        Ok(SubtaskResult {
            subtask_id: spec.id.clone(),
            agent_role: spec.agent_role.clone(),
            status: SubtaskStatus::Completed,
            output: Some(response),
            duration: start.elapsed(),
            llm_turns: 0, // TODO: track from Rig agent
        })
    }

    /// Read all input_keys from shared memory and format as context.
    fn gather_inputs(
        &self,
        spec: &SubtaskSpec,
        memory: &SharedMemory,
    ) -> Result<HashMap<String, String>> {
        let mut inputs = HashMap::new();
        for key in &spec.input_keys {
            let value = memory.get::<String>(key)
                .ok_or_else(|| SwarmError::MissingDependency {
                    subtask: spec.id.clone(),
                    key: key.clone(),
                })?;
            inputs.insert(key.clone(), value);
        }
        Ok(inputs)
    }

    /// Build a prompt that includes the subtask instruction plus dependency data.
    fn build_worker_prompt(
        &self,
        spec: &SubtaskSpec,
        context_data: &HashMap<String, String>,
    ) -> String {
        let mut prompt = spec.prompt.clone();

        if !context_data.is_empty() {
            prompt.push_str("\n\n## Context from previous subtasks\n\n");
            for (key, value) in context_data {
                prompt.push_str(&format!("### {key}\n{value}\n\n"));
            }
        }

        prompt
    }
}
```

---

## 7. Shared Memory

The shared memory store is the communication backbone between workers. It uses `DashMap` for lock-free concurrent access with namespaced keys.

```rust
// crates/mcclawd-swarm/src/shared_memory.rs

use dashmap::DashMap;
use serde::{Serialize, de::DeserializeOwned};

/// Thread-safe shared memory for swarm workers.
/// Keys are strings; values are stored as JSON for type flexibility.
#[derive(Clone)]
pub struct SharedMemory {
    store: Arc<DashMap<String, serde_json::Value>>,
}

impl SharedMemory {
    pub fn new() -> Self {
        Self {
            store: Arc::new(DashMap::new()),
        }
    }

    /// Set a value in shared memory (serialized to JSON).
    pub fn set<T: Serialize>(&self, key: &str, value: T) {
        let json = serde_json::to_value(value).expect("serialize shared memory value");
        self.store.insert(key.to_string(), json);
    }

    /// Get a value from shared memory (deserialized from JSON).
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.store.get(key).and_then(|v| serde_json::from_value(v.value().clone()).ok())
    }

    /// Check if a key exists.
    pub fn contains(&self, key: &str) -> bool {
        self.store.contains_key(key)
    }

    /// List all keys (useful for debugging / planner introspection).
    pub fn keys(&self) -> Vec<String> {
        self.store.iter().map(|r| r.key().clone()).collect()
    }

    /// Get a snapshot of all key-value pairs (for the merger).
    pub fn snapshot(&self) -> HashMap<String, serde_json::Value> {
        self.store.iter().map(|r| (r.key().clone(), r.value().clone())).collect()
    }

    /// Namespace helper: prefix keys with a subtask ID to avoid collisions.
    pub fn namespaced(&self, namespace: &str) -> NamespacedMemory {
        NamespacedMemory {
            store: self.clone(),
            prefix: format!("{namespace}:"),
        }
    }
}

/// A namespaced view into shared memory. All keys are auto-prefixed.
pub struct NamespacedMemory {
    store: SharedMemory,
    prefix: String,
}

impl NamespacedMemory {
    pub fn set<T: Serialize>(&self, key: &str, value: T) {
        self.store.set(&format!("{}{}", self.prefix, key), value);
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.store.get(&format!("{}{}", self.prefix, key))
    }
}
```

---

## 8. Output Merger / Consensus

The merger combines subtask outputs into a final result. Multiple strategies are supported depending on the task type.

```rust
// crates/mcclawd-swarm/src/merger.rs

/// Strategy for merging subtask outputs into a final result.
#[derive(Debug, Clone)]
pub enum MergeStrategy {
    /// LLM synthesizes all outputs into a coherent response (default).
    LlmSynthesis,
    /// Concatenate outputs in DAG topological order.
    Concatenate,
    /// Use the output of the final DAG node only (pipeline pattern).
    LastNode,
    /// Majority vote — for tasks where multiple agents solve the same problem.
    MajorityVote,
    /// Custom merge prompt (user-provided).
    Custom(String),
}

pub struct OutputMerger {
    strategy: MergeStrategy,
    api_key: String,
}

impl OutputMerger {
    pub fn new(strategy: MergeStrategy, api_key: String) -> Self {
        Self { strategy, api_key }
    }

    /// Merge subtask results into a final output string.
    pub async fn merge(
        &self,
        original_prompt: &str,
        results: &HashMap<SubtaskId, SubtaskResult>,
        shared_memory: &SharedMemory,
    ) -> Result<String> {
        match &self.strategy {
            MergeStrategy::LlmSynthesis => {
                self.llm_merge(original_prompt, results).await
            }
            MergeStrategy::Concatenate => {
                Ok(self.concatenate_results(results))
            }
            MergeStrategy::LastNode => {
                self.last_node_result(results)
            }
            MergeStrategy::MajorityVote => {
                self.majority_vote(results)
            }
            MergeStrategy::Custom(prompt) => {
                self.llm_merge(prompt, results).await
            }
        }
    }

    /// Use an LLM to synthesize all worker outputs into a coherent response.
    async fn llm_merge(
        &self,
        prompt: &str,
        results: &HashMap<SubtaskId, SubtaskResult>,
    ) -> Result<String> {
        // Build a synthesis prompt:
        // "Given the original task: {prompt}
        //  And these subtask results:
        //  [subtask_1/role]: {output}
        //  [subtask_2/role]: {output}
        //  ...
        //  Synthesize a complete, coherent response."
        //
        // Run through a Rig agent with no tools (pure completion).
        todo!()
    }
}
```

---

## 9. Failure Handling

Failure handling follows a layered strategy:

### 9a. Worker-Level Retry

```
Worker fails → retry same subtask (up to 3 attempts, exponential backoff)
             → same agent role, same prompt, same dependencies
             → if all retries fail → escalate to coordinator
```

### 9b. Coordinator-Level Re-planning

```
Worker exhausted retries → coordinator evaluates:
  1. Is the subtask critical? (does anything depend on it?)
     - No dependents → mark as Skipped, continue
     - Has dependents → attempt re-plan
  2. Re-plan: ask planner to produce an alternative subtask
     (different approach, different role, simplified scope)
     - max_replan_depth limits recursion
  3. If re-plan also fails → abort the swarm
```

### 9c. Swarm-Level Abort

```
Abort condition:
  - Critical subtask failed after retries + re-plan
  - More than 50% of subtasks failed
  - Total swarm duration exceeds 3x the estimated time

On abort:
  - Cancel all running workers (CancellationToken)
  - Collect partial results
  - Return SwarmResult with partial outputs + error summary
  - User sees: "[swarm] Partially completed: 7/10 subtasks succeeded. Failed: ..."
```

```rust
// crates/mcclawd-swarm/src/error.rs

#[derive(Debug, thiserror::Error)]
pub enum SwarmError {
    #[error("Unknown agent role: {0}")]
    UnknownRole(String),

    #[error("Unknown subtask: {0}")]
    UnknownSubtask(SubtaskId),

    #[error("Cyclic dependency detected in task DAG")]
    CyclicDependency,

    #[error("Missing dependency for subtask {subtask}: key '{key}' not in shared memory")]
    MissingDependency { subtask: SubtaskId, key: String },

    #[error("Planner failed to produce a valid plan: {0}")]
    PlannerFailed(String),

    #[error("Worker join error: {0}")]
    JoinError(String),

    #[error("Swarm aborted: {reason}")]
    Aborted { reason: String, partial_results: HashMap<SubtaskId, SubtaskResult> },

    #[error("Re-plan depth exceeded ({depth}/{max})")]
    ReplanDepthExceeded { depth: u32, max: u32 },

    #[error(transparent)]
    Agent(#[from] anyhow::Error),
}
```

---

## 10. Agent Roles (AGENTS.md Integration)

Swarm roles come directly from the existing `AGENTS.md` parser (`AgentsConfig`). No new config format is needed. The `delegate_when` field drives automatic role selection.

### Default Swarm Roles

```markdown
# AGENTS.md (extended for swarm support)

## Default Skills
- memory

## Available Agents

### planner
- **Specialty:** Task decomposition and planning
- **Model:** claude-sonnet-4-5
- **Tools:** (none — planner tools are injected by SwarmPlanner)
- **Skills:** (none)
- **Delegate when:** Always used as swarm planner (not user-facing)

### coding
- **Specialty:** Code generation, debugging, refactoring
- **Model:** claude-sonnet-4-5
- **Tools:** exec, read, write, mcp:github
- **Skills:** git-workflow, code-review
- **Delegate when:** User asks for code changes or technical implementation

### research
- **Specialty:** Information gathering, summarization, analysis
- **Model:** claude-sonnet-4-5
- **Tools:** mcp:web-search, mcp:web-scraper
- **Skills:** web-scraper
- **Delegate when:** User asks for research, comparisons, or external information

### reviewer
- **Specialty:** Code review, quality assurance, testing
- **Model:** claude-sonnet-4-5
- **Tools:** read, exec
- **Skills:** code-review, testing
- **Delegate when:** After code generation, for quality checks

### writer
- **Specialty:** Documentation, reports, prose
- **Model:** claude-sonnet-4-5
- **Tools:** read, write
- **Skills:** (none)
- **Delegate when:** User asks for documentation, reports, or written content

## Delegation Rules
- Research tasks go to `research` agent
- Code tasks go to `coding` agent, then `reviewer` for QA
- Writing tasks go to `writer` agent
- Complex tasks trigger swarm with `planner` decomposition
```

---

## 11. Integration with Existing Crates

### 11a. TaskManager (mcclawd-tasks)

Extend `TaskStatus` and `TaskManager` for swarm awareness:

```rust
// Extend TaskStatus enum
pub enum TaskStatus {
    Pending,
    Building,
    Running,
    /// Swarm is executing: track wave progress.
    SwarmRunning {
        swarm_id: SwarmId,
        total_subtasks: usize,
        completed_subtasks: usize,
        current_wave: usize,
        total_waves: usize,
    },
    Restarting { attempt: u32, next_retry_secs: u64 },
    Completed,
    Failed(String),
}

// Extend ExecutionMode
pub enum ExecutionMode {
    SingleAgent,
    Swarm { config: SwarmConfig },
}
```

### 11b. AgentSupervisor (mcclawd-api)

The supervisor gains a `spawn_swarm` method alongside `spawn_agent`:

```rust
impl AgentSupervisor {
    /// Spawn a swarm for a complex task.
    pub async fn spawn_swarm(
        &self,
        task_id: TaskId,
        config: SwarmConfig,
        workspace: Workspace,
        agents_config: AgentsConfig,
        output_tx: broadcast::Sender<OutboundChunk>,
    ) -> anyhow::Result<()> {
        let coordinator = SwarmCoordinator::new(
            config,
            agents_config,
            workspace,
            self.resolve_api_key()?,
        );

        tokio::spawn(async move {
            match coordinator.run(&task_prompt, Some(output_tx.clone())).await {
                Ok(result) => {
                    let _ = output_tx.send(OutboundChunk::TextBlock(result.final_output));
                    let _ = output_tx.send(OutboundChunk::Done);
                }
                Err(e) => {
                    let _ = output_tx.send(OutboundChunk::Error(e.to_string()));
                }
            }
        });

        Ok(())
    }
}
```

### 11c. API Routes (mcclawd-api)

```rust
// POST /api/tasks — extended request body
#[derive(Deserialize)]
pub struct CreateTaskRequest {
    pub prompt: String,
    pub mode: Option<String>,         // "single" | "swarm" (default: auto-detect)
    pub swarm_config: Option<SwarmConfig>,
}

// GET /api/tasks/:id — extended response
#[derive(Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub status: TaskStatus,           // includes SwarmRunning variant
    pub swarm_progress: Option<SwarmProgress>,
}

#[derive(Serialize)]
pub struct SwarmProgress {
    pub swarm_id: String,
    pub total_subtasks: usize,
    pub completed: usize,
    pub failed: usize,
    pub current_wave: usize,
    pub subtasks: Vec<SubtaskSummary>,
}
```

### 11d. CLI (mc run)

```bash
# Explicit swarm mode
mc run --swarm "Research competitors and write a comparison report"

# With config overrides
mc run --swarm --max-workers 8 --worker-timeout 600 "Build a full-stack app"

# Auto-detect (coordinator decides if swarm is needed)
mc run "Research competitors and write a comparison report"
```

Auto-detection heuristic: If the prompt contains multiple distinct sub-goals, or if AGENTS.md delegation rules match multiple agents, the coordinator automatically uses swarm mode.

### 11e. WebSocket Streaming

Swarm progress streams through the existing WebSocket infrastructure. Each `OutboundChunk` is prefixed with the swarm/subtask context:

```
[swarm:abc123] Planning task decomposition...
[swarm:abc123] Plan: 6 subtasks in 3 waves
[swarm:abc123] Executing wave 1/3 (3 subtasks)
[worker:research:sub1] Researching competitor A...
[worker:research:sub2] Researching competitor B...
[worker:research:sub3] Researching competitor C...
[swarm:abc123] Executing wave 2/3 (2 subtasks)
[worker:coding:sub4] Analyzing research data...
[worker:writer:sub5] Drafting comparison section...
[swarm:abc123] Executing wave 3/3 (1 subtask)
[worker:writer:sub6] Synthesizing final report...
[swarm:abc123] Complete: 6/6 subtasks, 45.2s total
```

---

## 12. Swarm Patterns (Common DAG Shapes)

### 12a. Fan-Out/Fan-In (Research)
```
                    ┌─ research_1 ─┐
                    ├─ research_2 ─┤
plan ──────────────►├─ research_3 ─├──► synthesize ──► report
                    ├─ research_4 ─┤
                    └─ research_5 ─┘
```

### 12b. Pipeline (Code + Review)
```
plan ──► design ──► implement ──► test ──► review ──► final
```

### 12c. Map-Reduce (Data Processing)
```
         ┌─ process_chunk_1 ─┐
split ──►├─ process_chunk_2 ─├──► reduce ──► output
         └─ process_chunk_3 ─┘
```

### 12d. Diamond (Multiple Dependencies)
```
         ┌─ frontend ──┐
plan ───►│              ├──► integrate ──► test
         └─ backend  ──┘
```

---

## 13. Implementation Plan

| # | Task | Crate | Files | Depends On |
|---|------|-------|-------|------------|
| 1 | Add `SwarmId`, `SubtaskId` to core types | mcclawd-core | types.rs | — |
| 2 | Create mcclawd-swarm crate skeleton | mcclawd-swarm | lib.rs, error.rs, Cargo.toml | 1 |
| 3 | Implement `TaskDag` with petgraph | mcclawd-swarm | dag.rs + tests | 2 |
| 4 | Implement `SharedMemory` | mcclawd-swarm | shared_memory.rs + tests | 2 |
| 5 | Implement `SwarmPlanner` with Rig tools | mcclawd-swarm | planner.rs | 3, 4 |
| 6 | Implement `WorkerAgent` | mcclawd-swarm | worker.rs | 4 |
| 7 | Implement `OutputMerger` | mcclawd-swarm | merger.rs | 6 |
| 8 | Implement `SwarmCoordinator` | mcclawd-swarm | coordinator.rs | 5, 6, 7 |
| 9 | Extend `TaskStatus` for swarm states | mcclawd-tasks | manager.rs | 1 |
| 10 | Add `spawn_swarm` to AgentSupervisor | mcclawd-api | agent_supervisor.rs | 8, 9 |
| 11 | Add `--swarm` flag to `mc run` | mcclawd-api | commands/run.rs | 10 |
| 12 | Extend API routes for swarm progress | mcclawd-api | routes.rs | 10 |
| 13 | Integration tests | mcclawd-swarm | tests/ | all |

**Estimated total:** ~2000 lines of Rust, 8 new files, 4 modified files.

---

## 14. Open Questions

1. **Sandbox per worker vs. shared sandbox?** Current design: each worker gets its own sandbox container. Simpler isolation but higher overhead. Alternative: workers share a sandbox with namespaced filesystem access. Decision: start with per-worker, optimize later if container startup is a bottleneck.

2. **Planner model choice?** The planner needs strong reasoning but no tool use beyond DAG construction. A smaller/faster model might suffice. Decision: default to workspace model, allow override via `SwarmConfig.planner_model`.

3. **Shared memory persistence?** Currently in-memory only (lost if daemon restarts mid-swarm). Phase 3+ could persist to SQLite or Redis. Decision: in-memory for Phase 2, add persistence trait later.

4. **Auto-detect swarm vs. single agent?** Should `mc run` automatically choose swarm mode? Decision: add heuristic in Phase 2 but default to explicit `--swarm` flag. Auto-detect can be refined in Phase 3.

5. **Worker-to-worker direct communication?** Current design uses shared memory only (no direct channels between workers). This is simpler but prevents real-time collaboration. Decision: shared memory is sufficient for Phase 2. Direct channels are a Phase 3+ concern.

---

## 15. Success Criteria

1. `mc run --swarm "Research X and write a report"` produces a multi-agent execution with visible progress.
2. Fan-out/fan-in pattern works: N parallel research agents feed into 1 synthesis agent.
3. Pipeline pattern works: design -> implement -> test -> review chain.
4. Worker failure triggers retry, then re-plan, then graceful degradation.
5. WebSocket streams per-worker progress updates to the UI.
6. Swarm uses AGENTS.md roles — adding a new role requires only editing AGENTS.md.
7. Shared memory correctly passes data between dependent subtasks.
8. `GET /api/tasks/:id` returns swarm progress (waves, subtask statuses).
