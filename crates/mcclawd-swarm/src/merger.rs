use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{Result, SwarmError};

/// Strategy for merging subtask outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeStrategy {
    /// LLM synthesizes all outputs (default — placeholder for now).
    LlmSynthesis,
    /// Concatenate outputs in topological order.
    Concatenate,
    /// Use only the last node's output (pipeline pattern).
    LastNode,
    /// Majority vote (for redundant subtasks).
    MajorityVote,
    /// Custom merge prompt.
    Custom(String),
}

impl Default for MergeStrategy {
    fn default() -> Self {
        Self::Concatenate // Default to concatenate for testing; LlmSynthesis needs API key
    }
}

/// Result of a single subtask execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskResult {
    pub subtask_id: String,
    pub agent_role: String,
    pub output: Option<String>,
    pub status: SubtaskStatus,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubtaskStatus {
    Completed,
    Failed(String),
    Skipped { reason: String },
}

pub struct OutputMerger {
    strategy: MergeStrategy,
}

impl OutputMerger {
    pub fn new(strategy: MergeStrategy) -> Self {
        Self { strategy }
    }

    /// Merge subtask results into a final output string.
    /// `ordered_ids` provides topological order for concatenation.
    pub async fn merge(
        &self,
        _original_prompt: &str,
        results: &HashMap<String, SubtaskResult>,
        ordered_ids: &[String],
    ) -> Result<String> {
        match &self.strategy {
            MergeStrategy::Concatenate => {
                let parts: Vec<String> = ordered_ids
                    .iter()
                    .filter_map(|id| results.get(id))
                    .filter_map(|r| r.output.clone())
                    .collect();
                if parts.is_empty() {
                    return Err(SwarmError::MergeFailed("No outputs to merge".into()));
                }
                Ok(parts.join("\n\n---\n\n"))
            }
            MergeStrategy::LastNode => {
                let last_id = ordered_ids
                    .last()
                    .ok_or_else(|| SwarmError::MergeFailed("No nodes".into()))?;
                results
                    .get(last_id)
                    .and_then(|r| r.output.clone())
                    .ok_or_else(|| SwarmError::MergeFailed("Last node has no output".into()))
            }
            MergeStrategy::MajorityVote => {
                let mut votes: HashMap<String, usize> = HashMap::new();
                for r in results.values() {
                    if let Some(ref output) = r.output {
                        *votes.entry(output.clone()).or_default() += 1;
                    }
                }
                votes
                    .into_iter()
                    .max_by_key(|(_, count)| *count)
                    .map(|(output, _)| output)
                    .ok_or_else(|| SwarmError::MergeFailed("No votes".into()))
            }
            MergeStrategy::LlmSynthesis => {
                // Placeholder — would call LLM to synthesize
                // For now, fall back to concatenate
                let parts: Vec<String> = ordered_ids
                    .iter()
                    .filter_map(|id| results.get(id))
                    .filter_map(|r| r.output.clone())
                    .collect();
                Ok(format!(
                    "[LLM Synthesis Placeholder]\n\n{}",
                    parts.join("\n\n")
                ))
            }
            MergeStrategy::Custom(prompt) => {
                // Placeholder — would use custom prompt with LLM
                Ok(format!("[Custom merge: {}]", prompt))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_results() -> (HashMap<String, SubtaskResult>, Vec<String>) {
        let mut results = HashMap::new();
        results.insert(
            "research".into(),
            SubtaskResult {
                subtask_id: "research".into(),
                agent_role: "researcher".into(),
                output: Some("Research findings here".into()),
                status: SubtaskStatus::Completed,
                duration_ms: 1000,
            },
        );
        results.insert(
            "code".into(),
            SubtaskResult {
                subtask_id: "code".into(),
                agent_role: "coder".into(),
                output: Some("Code implementation here".into()),
                status: SubtaskStatus::Completed,
                duration_ms: 2000,
            },
        );
        (results, vec!["research".into(), "code".into()])
    }

    #[tokio::test]
    async fn concatenate_in_order() {
        let (results, order) = make_results();
        let merger = OutputMerger::new(MergeStrategy::Concatenate);
        let output = merger.merge("task", &results, &order).await.unwrap();
        assert!(output.contains("Research findings"));
        assert!(output.contains("Code implementation"));
        // Research should come before Code
        let r_pos = output.find("Research").unwrap();
        let c_pos = output.find("Code").unwrap();
        assert!(r_pos < c_pos);
    }

    #[tokio::test]
    async fn last_node() {
        let (results, order) = make_results();
        let merger = OutputMerger::new(MergeStrategy::LastNode);
        let output = merger.merge("task", &results, &order).await.unwrap();
        assert_eq!(output, "Code implementation here");
    }

    #[tokio::test]
    async fn majority_vote() {
        let mut results = HashMap::new();
        results.insert(
            "a".into(),
            SubtaskResult {
                subtask_id: "a".into(),
                agent_role: "".into(),
                output: Some("yes".into()),
                status: SubtaskStatus::Completed,
                duration_ms: 0,
            },
        );
        results.insert(
            "b".into(),
            SubtaskResult {
                subtask_id: "b".into(),
                agent_role: "".into(),
                output: Some("yes".into()),
                status: SubtaskStatus::Completed,
                duration_ms: 0,
            },
        );
        results.insert(
            "c".into(),
            SubtaskResult {
                subtask_id: "c".into(),
                agent_role: "".into(),
                output: Some("no".into()),
                status: SubtaskStatus::Completed,
                duration_ms: 0,
            },
        );
        let merger = OutputMerger::new(MergeStrategy::MajorityVote);
        let output = merger
            .merge(
                "task",
                &results,
                &["a", "b", "c"].map(String::from).to_vec(),
            )
            .await
            .unwrap();
        assert_eq!(output, "yes");
    }

    #[tokio::test]
    async fn merge_empty_fails() {
        let merger = OutputMerger::new(MergeStrategy::Concatenate);
        let result = merger.merge("task", &HashMap::new(), &[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn merge_strategy_default() {
        let strategy = MergeStrategy::default();
        assert!(matches!(strategy, MergeStrategy::Concatenate));
    }
}
