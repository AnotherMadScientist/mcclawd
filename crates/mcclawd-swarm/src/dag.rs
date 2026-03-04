use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;

use crate::error::{Result, SwarmError};

/// A single subtask node in the execution DAG.
#[derive(Debug, Clone)]
pub struct SubtaskNode {
    pub id: String,
    pub prompt: String,
    pub agent_role: String,
    pub input_keys: Vec<String>,
    pub output_key: String,
}

/// Directed acyclic graph of subtasks with dependency edges.
///
/// Edges point from prerequisite to dependent: an edge from A to B means
/// "A must complete before B can start."
pub struct TaskDag {
    graph: DiGraph<SubtaskNode, ()>,
    index_map: HashMap<String, NodeIndex>,
}

impl TaskDag {
    /// Create an empty DAG.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index_map: HashMap::new(),
        }
    }

    /// Add a subtask node and return its graph index.
    pub fn add_subtask(&mut self, node: SubtaskNode) -> NodeIndex {
        let id = node.id.clone();
        let idx = self.graph.add_node(node);
        self.index_map.insert(id, idx);
        idx
    }

    /// Add a dependency edge: `from` must complete before `to` can start.
    pub fn add_dependency(&mut self, from: &str, to: &str) -> Result<()> {
        let from_idx = self
            .index_map
            .get(from)
            .copied()
            .ok_or_else(|| SwarmError::PlanningFailed(format!("unknown subtask: {from}")))?;
        let to_idx = self
            .index_map
            .get(to)
            .copied()
            .ok_or_else(|| SwarmError::PlanningFailed(format!("unknown subtask: {to}")))?;
        self.graph.add_edge(from_idx, to_idx, ());
        Ok(())
    }

    /// Validate that the graph is acyclic.
    pub fn validate(&self) -> Result<()> {
        petgraph::algo::toposort(&self.graph, None).map_err(|_| SwarmError::CycleDetected)?;
        Ok(())
    }

    /// Return waves of subtask IDs that can execute in parallel.
    ///
    /// Uses Kahn's algorithm: each wave contains all nodes whose in-degree
    /// is zero (after removing previous waves). Within a wave every subtask
    /// is independent of the others.
    pub fn topological_waves(&self) -> Result<Vec<Vec<String>>> {
        let node_count = self.graph.node_count();
        if node_count == 0 {
            return Ok(vec![]);
        }

        // Compute initial in-degrees.
        let mut in_degree: HashMap<NodeIndex, usize> = HashMap::with_capacity(node_count);
        for idx in self.graph.node_indices() {
            in_degree.insert(
                idx,
                self.graph.neighbors_directed(idx, Direction::Incoming).count(),
            );
        }

        let mut waves: Vec<Vec<String>> = Vec::new();
        let mut processed = 0usize;

        loop {
            // Collect all nodes with in-degree 0 that haven't been removed.
            let wave: Vec<NodeIndex> = in_degree
                .iter()
                .filter(|(_, deg)| **deg == 0)
                .map(|(idx, _)| *idx)
                .collect();

            if wave.is_empty() {
                break;
            }

            // Record this wave's subtask IDs.
            let mut ids: Vec<String> = wave
                .iter()
                .map(|&idx| self.graph[idx].id.clone())
                .collect();
            ids.sort(); // deterministic ordering within a wave
            waves.push(ids);

            // Remove wave nodes and decrement successors' in-degrees.
            for &idx in &wave {
                in_degree.remove(&idx);
                for succ in self.graph.neighbors_directed(idx, Direction::Outgoing) {
                    if let Some(deg) = in_degree.get_mut(&succ) {
                        *deg = deg.saturating_sub(1);
                    }
                }
            }

            processed += wave.len();
        }

        if processed != node_count {
            return Err(SwarmError::CycleDetected);
        }

        Ok(waves)
    }

    /// Look up a subtask by its string ID.
    pub fn subtask(&self, id: &str) -> Option<&SubtaskNode> {
        self.index_map
            .get(id)
            .map(|&idx| &self.graph[idx])
    }
}

impl Default for TaskDag {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_retrieve_subtask() {
        let mut dag = TaskDag::new();
        dag.add_subtask(SubtaskNode {
            id: "research".into(),
            prompt: "Research the topic".into(),
            agent_role: "researcher".into(),
            input_keys: vec![],
            output_key: "research_output".into(),
        });
        assert!(dag.subtask("research").is_some());
        assert!(dag.subtask("nonexistent").is_none());
    }

    #[test]
    fn dependency_ordering() {
        let mut dag = TaskDag::new();
        dag.add_subtask(SubtaskNode {
            id: "a".into(),
            prompt: "".into(),
            agent_role: "".into(),
            input_keys: vec![],
            output_key: "a_out".into(),
        });
        dag.add_subtask(SubtaskNode {
            id: "b".into(),
            prompt: "".into(),
            agent_role: "".into(),
            input_keys: vec!["a_out".into()],
            output_key: "b_out".into(),
        });
        dag.add_dependency("a", "b").unwrap();
        let waves = dag.topological_waves().unwrap();
        assert_eq!(waves.len(), 2);
        assert!(waves[0].contains(&"a".to_string()));
        assert!(waves[1].contains(&"b".to_string()));
    }

    #[test]
    fn parallel_tasks_in_same_wave() {
        let mut dag = TaskDag::new();
        dag.add_subtask(SubtaskNode {
            id: "a".into(),
            prompt: "".into(),
            agent_role: "".into(),
            input_keys: vec![],
            output_key: "a_out".into(),
        });
        dag.add_subtask(SubtaskNode {
            id: "b".into(),
            prompt: "".into(),
            agent_role: "".into(),
            input_keys: vec![],
            output_key: "b_out".into(),
        });
        dag.add_subtask(SubtaskNode {
            id: "c".into(),
            prompt: "".into(),
            agent_role: "".into(),
            input_keys: vec!["a_out".into(), "b_out".into()],
            output_key: "c_out".into(),
        });
        dag.add_dependency("a", "c").unwrap();
        dag.add_dependency("b", "c").unwrap();
        let waves = dag.topological_waves().unwrap();
        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0].len(), 2); // a and b in parallel
        assert_eq!(waves[1], vec!["c"]);
    }

    #[test]
    fn cycle_detected() {
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
        assert!(dag.validate().is_err());
    }

    #[test]
    fn validate_acyclic() {
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
        assert!(dag.validate().is_ok());
    }
}
