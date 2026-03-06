//! Tool resolution — maps requested skills to a resolved set of MCP tools,
//! install steps, and a deterministic Docker image cache key.
//!
//! Uses [`DepResolver`] for topological ordering so dependencies are installed
//! before the skills that need them.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

use crate::clawhub::DepResolver;
use crate::config::McpServerConfig;
use crate::skills::LoadedSkill;

/// The fully-resolved set of tools/skills needed for a task.
#[derive(Debug, Clone)]
pub struct ResolvedToolSet {
    /// Skills in topological install order (dependencies first).
    pub skills: Vec<LoadedSkill>,
    /// MCP server names required (matched from skill mcp_tools prefixes).
    pub required_servers: Vec<String>,
    /// Allowed tool prefixes (union of all resolved skills' mcp_tools).
    pub allowed_tools: HashSet<String>,
    /// Aggregated install steps for Docker image build (deduped, ordered).
    pub install_steps: Vec<String>,
    /// Combined skill context for agent system prompt.
    pub skill_context: String,
    /// Deterministic cache key for the image (hash of base + install_steps).
    pub image_hash: String,
}

/// Resolves requested skills into a complete tool set with dependency ordering.
pub struct ToolResolver;

impl ToolResolver {
    /// Resolve requested skills → dependency graph → tool set → image hash.
    ///
    /// 1. Walks dependency graph via `DepResolver` (Kahn's topo sort)
    /// 2. Collects MCP tool prefixes, matches to configured MCP servers
    /// 3. Deduplicates and orders install steps
    /// 4. Computes deterministic image hash from base_image + install_steps
    pub fn resolve(
        requested: &[String],
        all_skills: &HashMap<String, LoadedSkill>,
        mcp_servers: &[McpServerConfig],
        base_image: &str,
    ) -> anyhow::Result<ResolvedToolSet> {
        // Validate that all requested skills exist
        for name in requested {
            if !all_skills.contains_key(name) {
                anyhow::bail!("Skill not found: {name}");
            }
        }

        // Build dependency map for only the requested skills and their transitive deps
        let dep_map = Self::build_dep_map(requested, all_skills)?;

        // Topological sort
        let ordered_names = DepResolver::resolve_order(&dep_map)?;

        // Collect resolved skills in topo order
        let skills: Vec<LoadedSkill> = ordered_names
            .iter()
            .filter_map(|name| all_skills.get(name).cloned())
            .collect();

        // Collect all MCP tool prefixes
        let allowed_tools: HashSet<String> = skills
            .iter()
            .flat_map(|s| s.mcp_tools.iter().cloned())
            .collect();

        // Match tool prefixes to configured MCP servers
        let required_servers: Vec<String> = mcp_servers
            .iter()
            .filter(|server| allowed_tools.contains(&server.name))
            .map(|server| server.name.clone())
            .collect();

        // Aggregate install steps (deduped, preserving topo order)
        let mut seen_steps = HashSet::new();
        let install_steps: Vec<String> = skills
            .iter()
            .flat_map(|s| s.install_steps.iter())
            .filter(|step| seen_steps.insert(step.to_string()))
            .cloned()
            .collect();

        // Build combined skill context
        let skill_context = skills
            .iter()
            .filter(|s| !s.context.is_empty())
            .map(|s| format!("## Skill: {}\n{}", s.name, s.context))
            .collect::<Vec<_>>()
            .join("\n\n");

        let image_hash = Self::compute_image_hash(base_image, &install_steps);

        Ok(ResolvedToolSet {
            skills,
            required_servers,
            allowed_tools,
            install_steps,
            skill_context,
            image_hash,
        })
    }

    /// Build transitive dependency map starting from the requested skills.
    fn build_dep_map(
        requested: &[String],
        all_skills: &HashMap<String, LoadedSkill>,
    ) -> anyhow::Result<HashMap<String, Vec<String>>> {
        let mut dep_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut to_visit: Vec<String> = requested.to_vec();
        let mut visited = HashSet::new();

        while let Some(name) = to_visit.pop() {
            if visited.contains(&name) {
                continue;
            }
            visited.insert(name.clone());

            let deps = all_skills
                .get(&name)
                .map(|s| s.dependencies.clone())
                .unwrap_or_default();

            for dep in &deps {
                if !all_skills.contains_key(dep) {
                    anyhow::bail!(
                        "Skill '{name}' depends on '{dep}', which is not installed"
                    );
                }
                to_visit.push(dep.clone());
            }

            dep_map.insert(name, deps);
        }

        Ok(dep_map)
    }

    /// Compute deterministic image hash from base image + sorted install steps.
    ///
    /// Returns first 12 hex chars of SHA-256(base_image + "|" + sorted steps joined by "|").
    fn compute_image_hash(base_image: &str, install_steps: &[String]) -> String {
        let mut sorted_steps = install_steps.to_vec();
        sorted_steps.sort();

        let mut hasher = Sha256::new();
        hasher.update(base_image.as_bytes());
        for step in &sorted_steps {
            hasher.update(b"|");
            hasher.update(step.as_bytes());
        }
        let result = hasher.finalize();
        // First 12 hex chars
        hex_encode(&result[..6])
    }
}

/// Encode bytes as lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(name: &str, deps: &[&str], tools: &[&str], steps: &[&str]) -> LoadedSkill {
        LoadedSkill {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            author: "test".to_string(),
            description: format!("Test skill {name}"),
            mcp_tools: tools.iter().map(|s| s.to_string()).collect(),
            install_steps: steps.iter().map(|s| s.to_string()).collect(),
            context: format!("Context for {name}"),
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            instructions: String::new(),
            examples: String::new(),
            config_section: String::new(),
        }
    }

    fn make_server(name: &str) -> McpServerConfig {
        McpServerConfig {
            name: name.to_string(),
            image: format!("mcp-{name}:latest"),
            port: 8000,
            env: vec![],
            volumes: vec![],
        }
    }

    #[test]
    fn resolve_single_skill_no_deps() {
        let mut skills = HashMap::new();
        skills.insert("a".into(), make_skill("a", &[], &["filesystem"], &["pip install pandas"]));
        let servers = vec![make_server("filesystem")];

        let result = ToolResolver::resolve(&["a".into()], &skills, &servers, "base:latest").unwrap();

        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "a");
        assert_eq!(result.required_servers, vec!["filesystem"]);
        assert!(result.allowed_tools.contains("filesystem"));
        assert_eq!(result.install_steps, vec!["pip install pandas"]);
        assert!(!result.image_hash.is_empty());
        assert_eq!(result.image_hash.len(), 12);
    }

    #[test]
    fn resolve_with_dependencies() {
        let mut skills = HashMap::new();
        skills.insert("web-scraper".into(), make_skill("web-scraper", &[], &["scrapling"], &["pip install scrapling"]));
        skills.insert("data-analyst".into(), make_skill("data-analyst", &["web-scraper"], &["filesystem"], &["pip install pandas"]));
        let servers = vec![make_server("scrapling"), make_server("filesystem")];

        let result = ToolResolver::resolve(&["data-analyst".into()], &skills, &servers, "base:latest").unwrap();

        // web-scraper comes before data-analyst (topo order)
        let names: Vec<&str> = result.skills.iter().map(|s| s.name.as_str()).collect();
        let ws_pos = names.iter().position(|&n| n == "web-scraper").unwrap();
        let da_pos = names.iter().position(|&n| n == "data-analyst").unwrap();
        assert!(ws_pos < da_pos, "dependency must come first");

        assert_eq!(result.required_servers.len(), 2);
        assert_eq!(result.install_steps.len(), 2);
    }

    #[test]
    fn resolve_deduplicates_install_steps() {
        let mut skills = HashMap::new();
        skills.insert("a".into(), make_skill("a", &[], &[], &["pip install numpy", "pip install pandas"]));
        skills.insert("b".into(), make_skill("b", &[], &[], &["pip install pandas", "pip install scipy"]));

        let result = ToolResolver::resolve(&["a".into(), "b".into()], &skills, &[], "base:latest").unwrap();

        // "pip install pandas" should appear only once
        let pandas_count = result.install_steps.iter().filter(|s| *s == "pip install pandas").count();
        assert_eq!(pandas_count, 1);
        assert_eq!(result.install_steps.len(), 3);
    }

    #[test]
    fn image_hash_is_deterministic() {
        let mut skills = HashMap::new();
        skills.insert("a".into(), make_skill("a", &[], &[], &["pip install pandas"]));

        let r1 = ToolResolver::resolve(&["a".into()], &skills, &[], "base:latest").unwrap();
        let r2 = ToolResolver::resolve(&["a".into()], &skills, &[], "base:latest").unwrap();

        assert_eq!(r1.image_hash, r2.image_hash);
    }

    #[test]
    fn different_base_image_produces_different_hash() {
        let mut skills = HashMap::new();
        skills.insert("a".into(), make_skill("a", &[], &[], &["pip install pandas"]));

        let r1 = ToolResolver::resolve(&["a".into()], &skills, &[], "base:v1").unwrap();
        let r2 = ToolResolver::resolve(&["a".into()], &skills, &[], "base:v2").unwrap();

        assert_ne!(r1.image_hash, r2.image_hash);
    }

    #[test]
    fn missing_skill_returns_error() {
        let skills = HashMap::new();
        let result = ToolResolver::resolve(&["nonexistent".into()], &skills, &[], "base:latest");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn missing_dependency_returns_error() {
        let mut skills = HashMap::new();
        skills.insert("a".into(), make_skill("a", &["missing-dep"], &[], &[]));

        let result = ToolResolver::resolve(&["a".into()], &skills, &[], "base:latest");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not installed"));
    }

    #[test]
    fn unmatched_tools_excluded_from_servers() {
        let mut skills = HashMap::new();
        skills.insert("a".into(), make_skill("a", &[], &["unknown-tool"], &[]));
        let servers = vec![make_server("filesystem")];

        let result = ToolResolver::resolve(&["a".into()], &skills, &servers, "base:latest").unwrap();

        assert!(result.required_servers.is_empty());
        assert!(result.allowed_tools.contains("unknown-tool"));
    }

    #[test]
    fn skill_context_combines_all_skills() {
        let mut skills = HashMap::new();
        skills.insert("a".into(), make_skill("a", &[], &[], &[]));
        skills.insert("b".into(), make_skill("b", &[], &[], &[]));

        let result = ToolResolver::resolve(&["a".into(), "b".into()], &skills, &[], "base:latest").unwrap();

        assert!(result.skill_context.contains("Skill: a"));
        assert!(result.skill_context.contains("Skill: b"));
    }
}
