use serde::{Deserialize, Serialize};

/// A parsed skill from a SKILL.md file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedSkill {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    /// MCP tool name prefixes for filtering (e.g., ["filesystem", "langextract"])
    pub mcp_tools: Vec<String>,
    /// Shell commands to run during Docker image build
    pub install_steps: Vec<String>,
    /// Context text injected into agent preamble
    pub context: String,
    /// Skill names this skill depends on (Gap 4: dependency resolution)
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Instructions for the agent on how to use this skill
    #[serde(default)]
    pub instructions: String,
    /// Usage examples for the agent
    #[serde(default)]
    pub examples: String,
    /// Configuration reference (not injected into prompt by default)
    #[serde(default)]
    pub config_section: String,
}

/// Configuration for a sandbox container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Base Docker image name (e.g., "mcclawd-sandbox")
    pub base_image: String,
    /// Workspace directory on host to bind-mount
    pub workspace_dir: String,
    /// Docker network name (e.g., "mcclawd_default")
    pub network: String,
    /// AgentGateway URL accessible from inside container
    pub agentgateway_url: String,
    /// Max memory in bytes (default: 512MB)
    pub memory_limit: Option<i64>,
    /// CPU limit in nano-CPUs (1_000_000_000 = 1 CPU)
    pub cpu_limit: Option<i64>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            base_image: "mcclawd-sandbox".to_string(),
            workspace_dir: ".".to_string(),
            network: "mcclawd_default".to_string(),
            agentgateway_url: "http://agentgateway:3000".to_string(),
            memory_limit: Some(512 * 1024 * 1024), // 512MB
            cpu_limit: Some(1_000_000_000),         // 1 CPU
        }
    }
}
