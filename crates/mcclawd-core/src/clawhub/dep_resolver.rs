//! Skill dependency resolution — topological sort and cycle detection.
//!
//! Uses Kahn's algorithm (BFS-based) to produce a valid installation order.
//! A dependency edge means "dep must be installed before the skill that needs it".

use std::collections::{HashMap, HashSet, VecDeque};

/// Resolves skill dependency graphs into installation order.
pub struct DepResolver;

impl DepResolver {
    /// Given a map of `skill_name -> [dependencies]`, returns skills in the order
    /// they should be installed (dependencies first, dependents last).
    ///
    /// Returns an error if a cycle is detected, with the names of the nodes in the cycle.
    pub fn resolve_order(deps: &HashMap<String, Vec<String>>) -> anyhow::Result<Vec<String>> {
        // Collect all node names (skills + their deps, in case deps aren't keys)
        let all_nodes: HashSet<String> = deps
            .keys()
            .cloned()
            .chain(deps.values().flatten().cloned())
            .collect();

        // Build adjacency list: dep -> [skills that need it] (i.e., dep must come before)
        // and in_degree: skill -> number of unresolved dependencies
        let mut in_degree: HashMap<String, usize> =
            all_nodes.iter().map(|n| (n.clone(), 0)).collect();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();

        for (skill, skill_deps) in deps {
            for dep in skill_deps {
                // edge: dep -> skill (dep must be installed first)
                adj.entry(dep.clone()).or_default().push(skill.clone());
                *in_degree.entry(skill.clone()).or_insert(0) += 1;
            }
        }

        // Kahn's BFS: start with nodes that have no dependencies
        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(n, _)| n.clone())
            .collect();

        // Sort for deterministic output
        let mut queue_vec: Vec<String> = queue.drain(..).collect();
        queue_vec.sort();
        queue.extend(queue_vec);

        let mut result = Vec::new();

        while let Some(node) = queue.pop_front() {
            result.push(node.clone());
            if let Some(neighbors) = adj.get(&node) {
                let mut next = Vec::new();
                for neighbor in neighbors {
                    let deg = in_degree.get_mut(neighbor).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        next.push(neighbor.clone());
                    }
                }
                next.sort();
                queue.extend(next);
            }
        }

        if result.len() != all_nodes.len() {
            let cycle_nodes: Vec<String> = all_nodes
                .into_iter()
                .filter(|n| !result.contains(n))
                .collect();
            anyhow::bail!(
                "Skill dependency cycle detected involving: {}",
                cycle_nodes.join(", ")
            );
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_linear_chain() {
        // A depends on B, B depends on C -> install order: C, B, A
        let deps: HashMap<String, Vec<String>> = [
            ("a".to_string(), vec!["b".to_string()]),
            ("b".to_string(), vec!["c".to_string()]),
            ("c".to_string(), vec![]),
        ]
        .into();
        let order = DepResolver::resolve_order(&deps).unwrap();
        let a_pos = order.iter().position(|x| x == "a").unwrap();
        let b_pos = order.iter().position(|x| x == "b").unwrap();
        let c_pos = order.iter().position(|x| x == "c").unwrap();
        assert!(c_pos < b_pos, "c must come before b");
        assert!(b_pos < a_pos, "b must come before a");
    }

    #[test]
    fn test_resolve_diamond() {
        // A depends on B and C, both depend on D -> D installed once, first
        let deps: HashMap<String, Vec<String>> = [
            ("a".to_string(), vec!["b".to_string(), "c".to_string()]),
            ("b".to_string(), vec!["d".to_string()]),
            ("c".to_string(), vec!["d".to_string()]),
            ("d".to_string(), vec![]),
        ]
        .into();
        let order = DepResolver::resolve_order(&deps).unwrap();
        assert_eq!(order.len(), 4);
        let d_pos = order.iter().position(|x| x == "d").unwrap();
        let b_pos = order.iter().position(|x| x == "b").unwrap();
        let c_pos = order.iter().position(|x| x == "c").unwrap();
        let a_pos = order.iter().position(|x| x == "a").unwrap();
        assert!(d_pos < b_pos, "d before b");
        assert!(d_pos < c_pos, "d before c");
        assert!(b_pos < a_pos || c_pos < a_pos, "b or c before a");
    }

    #[test]
    fn test_detect_cycle() {
        // A depends on B, B depends on A -> cycle
        let deps: HashMap<String, Vec<String>> = [
            ("a".to_string(), vec!["b".to_string()]),
            ("b".to_string(), vec!["a".to_string()]),
        ]
        .into();
        let result = DepResolver::resolve_order(&deps);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cycle"), "error should mention cycle: {err}");
    }

    #[test]
    fn test_single_skill_no_deps() {
        let deps: HashMap<String, Vec<String>> = [("a".to_string(), vec![])].into();
        let order = DepResolver::resolve_order(&deps).unwrap();
        assert_eq!(order, vec!["a"]);
    }

    #[test]
    fn test_already_installed_skipped_by_caller() {
        // The resolver just produces an order — callers filter out already-installed skills.
        // This test verifies all nodes appear in output.
        let deps: HashMap<String, Vec<String>> = [
            ("a".to_string(), vec!["b".to_string()]),
            ("b".to_string(), vec![]),
        ]
        .into();
        let order = DepResolver::resolve_order(&deps).unwrap();
        assert!(order.contains(&"a".to_string()));
        assert!(order.contains(&"b".to_string()));
    }
}
