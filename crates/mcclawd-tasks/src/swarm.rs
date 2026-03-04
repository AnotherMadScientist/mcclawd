//! Phase 2 swarm core types.
//!
//! These types define the data structures for coordinated multi-agent swarms:
//! task decomposition, subtask tracking, configuration, and results.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// SwarmId
// ---------------------------------------------------------------------------

/// Unique identifier for a swarm execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SwarmId(pub String);

impl SwarmId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for SwarmId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// SubtaskSpec
// ---------------------------------------------------------------------------

/// Specification for a single subtask within a swarm DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskSpec {
    /// Unique subtask identifier (e.g. "research-1", "code-2").
    pub id: String,
    /// The prompt / instruction for this subtask's agent.
    pub prompt: String,
    /// Agent role from AGENTS.md (e.g. "researcher", "coder").
    pub role: String,
    /// IDs of subtasks that must complete before this one starts.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Shared-memory keys this subtask reads from.
    #[serde(default)]
    pub input_keys: Vec<String>,
    /// Shared-memory key this subtask writes its output to.
    pub output_key: String,
}

// ---------------------------------------------------------------------------
// SubtaskStatus
// ---------------------------------------------------------------------------

/// Runtime status of an individual subtask.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubtaskStatus {
    /// Waiting for dependencies to complete.
    Pending,
    /// Currently executing.
    Running,
    /// Finished successfully with a result value.
    Complete(Value),
    /// Failed with an error message.
    Failed(String),
}

impl SubtaskStatus {
    /// Returns `true` if the subtask has reached a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, SubtaskStatus::Complete(_) | SubtaskStatus::Failed(_))
    }
}

// ---------------------------------------------------------------------------
// SwarmConfig
// ---------------------------------------------------------------------------

/// Configuration knobs for a swarm execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmConfig {
    /// Maximum number of subtasks that may run concurrently.
    pub max_concurrent: usize,
    /// Maximum retries per subtask before marking it failed.
    pub max_retries: u32,
    /// Maximum depth of re-planning when subtasks fail.
    pub max_replan_depth: u32,
    /// Overall timeout for the entire swarm execution.
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            max_retries: 2,
            max_replan_depth: 1,
            timeout: Duration::from_secs(600), // 10 minutes
        }
    }
}

/// Serde helper for [`Duration`] using human-readable strings like `"10m"`.
mod humantime_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

// ---------------------------------------------------------------------------
// SwarmStatus
// ---------------------------------------------------------------------------

/// High-level lifecycle status of a swarm.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SwarmStatus {
    /// LLM planner is decomposing the task into subtasks.
    Planning,
    /// Subtasks are being executed by worker agents.
    Running,
    /// All subtasks done; merger is aggregating outputs.
    Merging,
    /// Swarm finished successfully.
    Complete,
    /// Swarm failed (with reason).
    Failed(String),
}

impl SwarmStatus {
    /// Returns `true` if the swarm has reached a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, SwarmStatus::Complete | SwarmStatus::Failed(_))
    }
}

// ---------------------------------------------------------------------------
// SwarmResult
// ---------------------------------------------------------------------------

/// Final output of a completed swarm execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmResult {
    /// The swarm that produced this result.
    pub swarm_id: SwarmId,
    /// Merged final output string.
    pub final_output: String,
    /// Per-subtask results keyed by subtask id.
    pub subtask_results: HashMap<String, SubtaskStatus>,
    /// Wall-clock duration of the entire swarm run.
    #[serde(with = "humantime_serde")]
    pub duration: Duration,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_swarm_config_values() {
        let cfg = SwarmConfig::default();
        assert_eq!(cfg.max_concurrent, 4);
        assert_eq!(cfg.max_retries, 2);
        assert_eq!(cfg.max_replan_depth, 1);
        assert_eq!(cfg.timeout, Duration::from_secs(600));
    }

    #[test]
    fn subtask_spec_serialization_roundtrip() {
        let spec = SubtaskSpec {
            id: "research-1".into(),
            prompt: "Research Rust async patterns".into(),
            role: "researcher".into(),
            dependencies: vec![],
            input_keys: vec![],
            output_key: "research_output".into(),
        };

        let json = serde_json::to_string(&spec).expect("serialize");
        let back: SubtaskSpec = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.id, "research-1");
        assert_eq!(back.role, "researcher");
        assert_eq!(back.output_key, "research_output");
        assert!(back.dependencies.is_empty());
    }

    #[test]
    fn swarm_status_transitions() {
        // Planning is not terminal
        let status = SwarmStatus::Planning;
        assert!(!status.is_terminal());

        // Running is not terminal
        let status = SwarmStatus::Running;
        assert!(!status.is_terminal());

        // Merging is not terminal
        let status = SwarmStatus::Merging;
        assert!(!status.is_terminal());

        // Complete is terminal
        let status = SwarmStatus::Complete;
        assert!(status.is_terminal());

        // Failed is terminal
        let status = SwarmStatus::Failed("timeout".into());
        assert!(status.is_terminal());
    }

    #[test]
    fn subtask_status_terminal_check() {
        assert!(!SubtaskStatus::Pending.is_terminal());
        assert!(!SubtaskStatus::Running.is_terminal());
        assert!(SubtaskStatus::Complete(Value::Null).is_terminal());
        assert!(SubtaskStatus::Failed("err".into()).is_terminal());
    }

    #[test]
    fn swarm_id_display() {
        let id = SwarmId("test-123".into());
        assert_eq!(format!("{id}"), "test-123");
    }

    #[test]
    fn swarm_result_serialization_roundtrip() {
        let result = SwarmResult {
            swarm_id: SwarmId("s-1".into()),
            final_output: "Done".into(),
            subtask_results: HashMap::from([
                ("t1".into(), SubtaskStatus::Complete(Value::String("ok".into()))),
                ("t2".into(), SubtaskStatus::Failed("timeout".into())),
            ]),
            duration: Duration::from_secs(42),
        };

        let json = serde_json::to_string(&result).expect("serialize");
        let back: SwarmResult = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.swarm_id.0, "s-1");
        assert_eq!(back.final_output, "Done");
        assert_eq!(back.subtask_results.len(), 2);
        assert_eq!(back.duration, Duration::from_secs(42));
    }
}
