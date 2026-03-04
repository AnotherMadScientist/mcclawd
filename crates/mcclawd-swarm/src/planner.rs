//! Swarm Planner — LLM-driven task decomposition into a DAG.
//!
//! Three Rig tools let a planner agent build a `TaskDag` interactively:
//!
//! - `create_subtask` — add a node to the DAG
//! - `add_dependency` — add an edge (prerequisite relationship)
//! - `finalize_plan` — validate the DAG and return topological waves
//!
//! The `SwarmPlanner` struct orchestrates the agent call. For testing,
//! use `decompose_with_dag()` to bypass the LLM.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;

use crate::dag::{SubtaskNode, TaskDag};
use crate::error::SwarmError;

// ----------------------------------------------------------------
// Error
// ----------------------------------------------------------------

/// Error type for planner tools (implements `std::error::Error`).
#[derive(Debug, thiserror::Error)]
#[error("Planner tool error: {0}")]
pub struct PlannerToolError(String);

// ----------------------------------------------------------------
// Shared state
// ----------------------------------------------------------------

/// Shared state for all planner tools — they all mutate the same DAG.
pub type PlannerState = Arc<Mutex<TaskDag>>;

// ----------------------------------------------------------------
// Tool input types (Part 1)
// ----------------------------------------------------------------

/// Arguments for `create_subtask`.
#[derive(Debug, Deserialize)]
pub struct CreateSubtaskArgs {
    /// The agent role (e.g. "researcher", "coder", "reviewer").
    pub agent_role: String,
    /// The prompt/instruction for this subtask.
    pub prompt: String,
    /// Keys this subtask reads from shared memory (empty if root task).
    #[serde(default)]
    pub input_keys: Vec<String>,
    /// Key this subtask writes its output to in shared memory.
    pub output_key: String,
}

/// Arguments for `add_dependency`.
#[derive(Debug, Deserialize)]
pub struct AddDependencyArgs {
    /// The subtask that must complete first.
    pub from_subtask_id: String,
    /// The subtask that depends on `from_subtask_id`.
    pub to_subtask_id: String,
}

/// Arguments for `finalize_plan` (empty — signals the planner is done).
#[derive(Debug, Deserialize)]
pub struct FinalizePlanArgs {}

// ----------------------------------------------------------------
// CreateSubtaskTool (Part 2)
// ----------------------------------------------------------------

/// Monotonic counter for generating unique subtask IDs.
static SUBTASK_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Rig tool that creates a subtask node in the shared DAG.
#[derive(Serialize, Deserialize, Clone)]
pub struct CreateSubtaskTool {
    #[serde(skip)]
    pub state: PlannerState,
}

impl CreateSubtaskTool {
    pub fn new(state: PlannerState) -> Self {
        Self { state }
    }
}

impl Tool for CreateSubtaskTool {
    const NAME: &'static str = "create_subtask";
    type Error = PlannerToolError;
    type Args = CreateSubtaskArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "create_subtask".to_string(),
            description: "Create a new subtask in the execution plan. Returns the generated subtask ID.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_role": {
                        "type": "string",
                        "description": "The agent role to execute this subtask (e.g. researcher, coder, reviewer)"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "The instruction/prompt for this subtask"
                    },
                    "input_keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Keys this subtask reads from shared memory (empty if root task)"
                    },
                    "output_key": {
                        "type": "string",
                        "description": "Key this subtask writes its output to in shared memory"
                    }
                },
                "required": ["agent_role", "prompt", "output_key"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let counter = SUBTASK_COUNTER.fetch_add(1, Ordering::Relaxed);
        let id = format!("{}-{}", args.agent_role, counter);

        let node = SubtaskNode {
            id: id.clone(),
            prompt: args.prompt,
            agent_role: args.agent_role,
            input_keys: args.input_keys,
            output_key: args.output_key,
        };

        let mut dag = self.state.lock().await;
        dag.add_subtask(node);

        Ok(id)
    }
}

// ----------------------------------------------------------------
// AddDependencyTool
// ----------------------------------------------------------------

/// Rig tool that adds a dependency edge between two subtasks.
#[derive(Serialize, Deserialize, Clone)]
pub struct AddDependencyTool {
    #[serde(skip)]
    pub state: PlannerState,
}

impl AddDependencyTool {
    pub fn new(state: PlannerState) -> Self {
        Self { state }
    }
}

impl Tool for AddDependencyTool {
    const NAME: &'static str = "add_dependency";
    type Error = PlannerToolError;
    type Args = AddDependencyArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "add_dependency".to_string(),
            description: "Add a dependency edge: from_subtask must complete before to_subtask can start.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "from_subtask_id": {
                        "type": "string",
                        "description": "ID of the subtask that must complete first"
                    },
                    "to_subtask_id": {
                        "type": "string",
                        "description": "ID of the subtask that depends on from_subtask_id"
                    }
                },
                "required": ["from_subtask_id", "to_subtask_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut dag = self.state.lock().await;
        dag.add_dependency(&args.from_subtask_id, &args.to_subtask_id)
            .map_err(|e| PlannerToolError(e.to_string()))?;
        Ok("ok".to_string())
    }
}

// ----------------------------------------------------------------
// FinalizePlanTool
// ----------------------------------------------------------------

/// Rig tool that validates the DAG and returns topological execution waves.
#[derive(Serialize, Deserialize, Clone)]
pub struct FinalizePlanTool {
    #[serde(skip)]
    pub state: PlannerState,
}

impl FinalizePlanTool {
    pub fn new(state: PlannerState) -> Self {
        Self { state }
    }
}

impl Tool for FinalizePlanTool {
    const NAME: &'static str = "finalize_plan";
    type Error = PlannerToolError;
    type Args = FinalizePlanArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "finalize_plan".to_string(),
            description: "Validate the execution plan and return the topological waves (groups of subtasks that can run in parallel).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let dag = self.state.lock().await;
        dag.validate()
            .map_err(|e| PlannerToolError(e.to_string()))?;
        let waves = dag
            .topological_waves()
            .map_err(|e| PlannerToolError(e.to_string()))?;
        serde_json::to_string(&waves).map_err(|e| PlannerToolError(e.to_string()))
    }
}

// ----------------------------------------------------------------
// SwarmPlanner (Part 3)
// ----------------------------------------------------------------

/// Information about an available agent role.
#[derive(Debug, Clone, Serialize)]
pub struct AgentRoleInfo {
    /// Unique role identifier (e.g. "researcher", "coder").
    pub id: String,
    /// Human-readable description of the agent's specialty.
    pub specialty: Option<String>,
    /// MCP tools available to this role.
    pub tools: Vec<String>,
    /// Skills installed for this role.
    pub skills: Vec<String>,
}

/// Decomposes a high-level prompt into a `TaskDag` using an LLM planner agent.
///
/// The planner agent is given the three planner tools (`create_subtask`,
/// `add_dependency`, `finalize_plan`) and asked to break the prompt into
/// subtasks with dependencies. The result is a validated `TaskDag` ready
/// for the `SwarmCoordinator` to execute.
pub struct SwarmPlanner {
    /// Model identifier (e.g. "claude-sonnet-4-20250514").
    pub model: Option<String>,
    /// API key for the LLM provider.
    pub api_key: String,
}

impl SwarmPlanner {
    pub fn new(model: Option<String>, api_key: String) -> Self {
        Self { model, api_key }
    }

    /// Decompose a prompt into a `TaskDag` using the planner agent.
    ///
    /// In production, this runs an LLM agent with the planner tools.
    /// For testing, use `decompose_with_dag()` to provide a pre-built DAG.
    pub async fn decompose(
        &self,
        _prompt: &str,
        _roles: &[AgentRoleInfo],
    ) -> crate::error::Result<TaskDag> {
        // TODO: Wire up Rig agent with planner tools:
        //   1. Build a Rig agent with create_subtask, add_dependency, finalize_plan tools
        //   2. Provide the agent with the list of available roles + the prompt
        //   3. Let the agent decompose the prompt by calling tools
        //   4. Return the resulting TaskDag
        Err(SwarmError::PlanningFailed(
            "LLM planner not yet wired — use decompose_with_dag() for testing".into(),
        ))
    }

    /// Test helper: manually provide a pre-built DAG instead of LLM decomposition.
    pub fn decompose_with_dag(dag: TaskDag) -> crate::error::Result<TaskDag> {
        dag.validate()?;
        Ok(dag)
    }
}

// ----------------------------------------------------------------
// Tests
// ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Part 1: Deserialization tests ---

    #[test]
    fn create_subtask_args_deserialize() {
        let json = r#"{"agent_role":"researcher","prompt":"Research topic","input_keys":[],"output_key":"research_out"}"#;
        let args: CreateSubtaskArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.agent_role, "researcher");
        assert_eq!(args.output_key, "research_out");
    }

    #[test]
    fn create_subtask_args_deserialize_default_input_keys() {
        let json = r#"{"agent_role":"coder","prompt":"Write code","output_key":"code_out"}"#;
        let args: CreateSubtaskArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.agent_role, "coder");
        assert!(args.input_keys.is_empty());
    }

    #[test]
    fn add_dependency_args_deserialize() {
        let json = r#"{"from_subtask_id":"a","to_subtask_id":"b"}"#;
        let args: AddDependencyArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.from_subtask_id, "a");
        assert_eq!(args.to_subtask_id, "b");
    }

    #[test]
    fn finalize_plan_args_deserialize() {
        let json = r#"{}"#;
        let _args: FinalizePlanArgs = serde_json::from_str(json).unwrap();
    }

    // --- Part 2: Tool integration tests ---

    #[tokio::test]
    async fn create_subtask_tool_adds_to_dag() {
        let state: PlannerState = Arc::new(Mutex::new(TaskDag::new()));
        let tool = CreateSubtaskTool::new(state.clone());
        let result = tool
            .call(CreateSubtaskArgs {
                agent_role: "researcher".into(),
                prompt: "Research AI".into(),
                input_keys: vec![],
                output_key: "research_out".into(),
            })
            .await
            .unwrap();
        // Result should contain the generated subtask ID
        assert!(result.contains("researcher"));
        // DAG should have the subtask
        let dag = state.lock().await;
        assert!(dag.subtask(&result).is_some());
    }

    #[tokio::test]
    async fn add_dependency_tool() {
        let state: PlannerState = Arc::new(Mutex::new(TaskDag::new()));
        // First create two subtasks
        let create = CreateSubtaskTool::new(state.clone());
        let id_a = create
            .call(CreateSubtaskArgs {
                agent_role: "researcher".into(),
                prompt: "A".into(),
                input_keys: vec![],
                output_key: "a_out".into(),
            })
            .await
            .unwrap();
        let id_b = create
            .call(CreateSubtaskArgs {
                agent_role: "coder".into(),
                prompt: "B".into(),
                input_keys: vec!["a_out".into()],
                output_key: "b_out".into(),
            })
            .await
            .unwrap();
        // Add dependency
        let dep_tool = AddDependencyTool::new(state.clone());
        let result = dep_tool
            .call(AddDependencyArgs {
                from_subtask_id: id_a,
                to_subtask_id: id_b,
            })
            .await
            .unwrap();
        assert_eq!(result, "ok");
    }

    #[tokio::test]
    async fn add_dependency_tool_unknown_subtask() {
        let state: PlannerState = Arc::new(Mutex::new(TaskDag::new()));
        let dep_tool = AddDependencyTool::new(state.clone());
        let result = dep_tool
            .call(AddDependencyArgs {
                from_subtask_id: "nonexistent".into(),
                to_subtask_id: "also_nonexistent".into(),
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn finalize_plan_validates_and_returns_waves() {
        let state: PlannerState = Arc::new(Mutex::new(TaskDag::new()));
        let create = CreateSubtaskTool::new(state.clone());
        let id_a = create
            .call(CreateSubtaskArgs {
                agent_role: "researcher".into(),
                prompt: "A".into(),
                input_keys: vec![],
                output_key: "a".into(),
            })
            .await
            .unwrap();
        let id_b = create
            .call(CreateSubtaskArgs {
                agent_role: "coder".into(),
                prompt: "B".into(),
                input_keys: vec!["a".into()],
                output_key: "b".into(),
            })
            .await
            .unwrap();
        let dep = AddDependencyTool::new(state.clone());
        dep.call(AddDependencyArgs {
            from_subtask_id: id_a.clone(),
            to_subtask_id: id_b.clone(),
        })
        .await
        .unwrap();
        let finalize = FinalizePlanTool::new(state.clone());
        let waves_json = finalize.call(FinalizePlanArgs {}).await.unwrap();
        let waves: Vec<Vec<String>> = serde_json::from_str(&waves_json).unwrap();
        assert_eq!(waves.len(), 2);
        assert!(waves[0].contains(&id_a));
        assert!(waves[1].contains(&id_b));
    }

    #[tokio::test]
    async fn finalize_plan_detects_cycle() {
        let state: PlannerState = Arc::new(Mutex::new(TaskDag::new()));
        // Manually insert nodes with a cycle
        {
            let mut dag = state.lock().await;
            dag.add_subtask(SubtaskNode {
                id: "x".into(),
                prompt: "".into(),
                agent_role: "".into(),
                input_keys: vec![],
                output_key: "x_out".into(),
            });
            dag.add_subtask(SubtaskNode {
                id: "y".into(),
                prompt: "".into(),
                agent_role: "".into(),
                input_keys: vec![],
                output_key: "y_out".into(),
            });
            dag.add_dependency("x", "y").unwrap();
            dag.add_dependency("y", "x").unwrap();
        }
        let finalize = FinalizePlanTool::new(state.clone());
        let result = finalize.call(FinalizePlanArgs {}).await;
        assert!(result.is_err());
    }

    // --- Part 3: SwarmPlanner tests ---

    #[tokio::test]
    async fn swarm_planner_decompose_returns_error() {
        let planner = SwarmPlanner::new(None, "test-key".into());
        let result = planner.decompose("do something", &[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn swarm_planner_decompose_with_dag() {
        let mut dag = TaskDag::new();
        dag.add_subtask(SubtaskNode {
            id: "a".into(),
            prompt: "research".into(),
            agent_role: "researcher".into(),
            input_keys: vec![],
            output_key: "a_out".into(),
        });
        dag.add_subtask(SubtaskNode {
            id: "b".into(),
            prompt: "code".into(),
            agent_role: "coder".into(),
            input_keys: vec!["a_out".into()],
            output_key: "b_out".into(),
        });
        dag.add_dependency("a", "b").unwrap();
        let result = SwarmPlanner::decompose_with_dag(dag);
        assert!(result.is_ok());
        let validated_dag = result.unwrap();
        assert!(validated_dag.subtask("a").is_some());
        assert!(validated_dag.subtask("b").is_some());
    }

    #[tokio::test]
    async fn swarm_planner_decompose_with_dag_rejects_cycle() {
        let mut dag = TaskDag::new();
        dag.add_subtask(SubtaskNode {
            id: "a".into(),
            prompt: "".into(),
            agent_role: "".into(),
            input_keys: vec![],
            output_key: "".into(),
        });
        dag.add_subtask(SubtaskNode {
            id: "b".into(),
            prompt: "".into(),
            agent_role: "".into(),
            input_keys: vec![],
            output_key: "".into(),
        });
        dag.add_dependency("a", "b").unwrap();
        dag.add_dependency("b", "a").unwrap();
        let result = SwarmPlanner::decompose_with_dag(dag);
        assert!(result.is_err());
    }
}
