use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McclawdConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    /// PostgreSQL connection URL. When set, tasks/events/chat_history are persisted.
    /// Example: `postgres://postgres:mcclawd@localhost:5432/mcclawd`
    #[serde(default)]
    pub database_url: Option<String>,

    #[serde(default)]
    pub agent: AgentConfig,

    #[serde(default)]
    pub providers: ProvidersConfig,

    #[serde(default)]
    pub mcp: McpConfig,

    #[serde(default)]
    pub skills: SkillsConfig,

    #[serde(default)]
    pub sandbox: SandboxConfig,

    #[serde(default)]
    pub compat: CompatConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,

    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default = "default_workspace")]
    pub default_workspace: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: default_max_turns(),
            model: default_model(),
            default_workspace: default_workspace(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersConfig {
    pub anthropic: Option<ProviderConfig>,
    pub openai: Option<ProviderConfig>,
    pub ollama: Option<OllamaConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Name of the secret key (looked up via SecretBackend, NOT the raw API key)
    pub api_key_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_url")]
    pub url: String,
}

/// Configuration for ClawHub skill management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    /// Directory where managed (installed) skills are stored.
    #[serde(default = "default_skills_managed_dir")]
    pub managed_dir: PathBuf,

    /// ClawHub registry API base URL.
    #[serde(default = "default_clawhub_api")]
    pub clawhub_api: String,

    /// Directory for local skill catalog cache.
    #[serde(default = "default_skills_cache_dir")]
    pub cache_dir: PathBuf,

    /// Version pins: skill_name -> version. Pinned skills are not upgraded automatically.
    #[serde(default)]
    pub pinned_versions: std::collections::HashMap<String, String>,

    /// Max characters of skill context to inject into agent system prompt (Gap 6).
    #[serde(default = "default_max_skill_context_chars")]
    pub max_skill_context_chars: usize,
}

fn default_max_skill_context_chars() -> usize {
    50_000
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            managed_dir: default_skills_managed_dir(),
            clawhub_api: default_clawhub_api(),
            cache_dir: default_skills_cache_dir(),
            pinned_versions: std::collections::HashMap::new(),
            max_skill_context_chars: default_max_skill_context_chars(),
        }
    }
}

fn default_skills_managed_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mcclawd")
        .join("skills")
}

fn default_clawhub_api() -> String {
    "https://clawhub.ai".to_string()
}

fn default_skills_cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mcclawd")
        .join("cache")
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mcclawd")
}

fn default_max_turns() -> usize {
    20
}
fn default_model() -> String {
    "claude-sonnet-4-5".to_string()
}
fn default_workspace() -> String {
    "default".to_string()
}
fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_agentgateway_url")]
    pub agentgateway_url: String,

    #[serde(default = "default_mcp_servers")]
    pub servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub image: String,
    #[serde(default = "default_mcp_port")]
    pub port: u16,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub volumes: Vec<String>,
}

impl McpServerConfig {
    /// Direct connection URL for this MCP server (bypasses AgentGateway).
    pub fn url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            agentgateway_url: default_agentgateway_url(),
            servers: default_mcp_servers(),
        }
    }
}

fn default_agentgateway_url() -> String {
    "http://localhost:3000".to_string()
}

fn default_mcp_port() -> u16 {
    8000
}

fn default_mcp_servers() -> Vec<McpServerConfig> {
    vec![
        McpServerConfig {
            name: "langextract".to_string(),
            image: "ghcr.io/macleodlabs/mcp-langextract:latest".to_string(),
            port: 8001,
            env: vec!["GOOGLE_API_KEY".to_string()],
            volumes: vec![],
        },
        McpServerConfig {
            name: "scrapling".to_string(),
            image: "ghcr.io/macleodlabs/mcp-scrapling:latest".to_string(),
            port: 8002,
            env: vec![],
            volumes: vec![],
        },
        McpServerConfig {
            name: "filesystem".to_string(),
            image: "ghcr.io/macleodlabs/mcp-filesystem:latest".to_string(),
            port: 8003,
            env: vec![],
            volumes: vec!["/data:/data".to_string()],
        },
    ]
}

/// Configuration for Docker sandbox execution.
///
/// All agent execution runs inside Docker containers — there is no host-mode fallback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Docker image to use as base for agent containers.
    #[serde(default = "default_sandbox_image")]
    pub base_image: String,
    /// Memory limit for containers (bytes). Default: 512MB.
    #[serde(default = "default_sandbox_memory")]
    pub memory_limit: Option<i64>,
    /// CPU limit in nano-CPUs. Default: 1 CPU (1_000_000_000).
    #[serde(default)]
    pub cpu_limit: Option<i64>,
    /// Docker network name for agent + MCP communication. Default: "mcclawd_tools".
    #[serde(default = "default_sandbox_network")]
    pub network: String,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            base_image: default_sandbox_image(),
            memory_limit: default_sandbox_memory(),
            cpu_limit: None,
            network: default_sandbox_network(),
        }
    }
}

fn default_sandbox_image() -> String {
    "mcclawd-sandbox:latest".to_string()
}

fn default_sandbox_memory() -> Option<i64> {
    Some(512 * 1024 * 1024) // 512MB
}

fn default_sandbox_network() -> String {
    "mcclawd_tools".to_string()
}

/// OpenClaw compatibility settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatConfig {
    /// Auto-detect and offer to import `openclaw.json` / `.mcp.json` on startup.
    #[serde(default = "default_true")]
    pub openclaw_config: bool,
}

impl Default for CompatConfig {
    fn default() -> Self {
        Self {
            openclaw_config: default_true(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// Detect an OpenClaw config file in well-known locations.
///
/// Checks (in order):
/// 1. `~/.openclaw/openclaw.json`
/// 2. `.mcp.json` in the current directory
///
/// Returns the first path found, or `None`.
pub fn detect_openclaw_config() -> Option<PathBuf> {
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".openclaw").join("openclaw.json");
        if path.exists() {
            return Some(path);
        }
    }
    let mcp_path = PathBuf::from(".mcp.json");
    if mcp_path.exists() {
        return Some(mcp_path);
    }
    None
}

impl McclawdConfig {
    pub fn load(path: &Path) -> crate::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)
                .map_err(|e| crate::McclawdError::Config(e.to_string()))?;
            toml::from_str(&content).map_err(|e| crate::McclawdError::Config(e.to_string()))
        } else {
            Ok(Self::default())
        }
    }

    /// Write the current config to a TOML file on disk.
    pub fn save(&self, path: &Path) -> crate::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| crate::McclawdError::Config(format!("Cannot create config dir: {e}")))?;
        }
        let toml_str = toml::to_string_pretty(self)
            .map_err(|e| crate::McclawdError::Config(format!("Failed to serialize config: {e}")))?;
        std::fs::write(path, toml_str)
            .map_err(|e| crate::McclawdError::Config(format!("Failed to write config: {e}")))?;
        Ok(())
    }

    pub fn workspaces_dir(&self) -> PathBuf {
        self.data_dir.join("workspaces")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.data_dir.join("skills")
    }

    pub fn secrets_path(&self) -> PathBuf {
        self.data_dir.join("secrets.enc")
    }
}

impl Default for McclawdConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            database_url: None,
            agent: AgentConfig::default(),
            providers: ProvidersConfig::default(),
            mcp: McpConfig::default(),
            skills: SkillsConfig::default(),
            sandbox: SandboxConfig::default(),
            compat: CompatConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default_has_skills() {
        let config = McclawdConfig::default();
        assert_eq!(config.skills.clawhub_api, "https://clawhub.ai");
        assert!(config.skills.managed_dir.ends_with("skills"));
    }

    #[test]
    fn test_config_with_skills_section_parsed() {
        let toml_str = r#"
[skills]
managed_dir = "/custom/skills"
clawhub_api = "https://custom.registry.io"
cache_dir = "/custom/cache"
"#;
        let config: McclawdConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.skills.managed_dir, PathBuf::from("/custom/skills"));
        assert_eq!(config.skills.clawhub_api, "https://custom.registry.io");
        assert_eq!(config.skills.cache_dir, PathBuf::from("/custom/cache"));
    }

    #[test]
    fn test_config_skills_defaults_applied_when_missing() {
        let toml_str = r#"
[agent]
max_turns = 10
"#;
        let config: McclawdConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.skills.clawhub_api, "https://clawhub.ai");
        assert!(config.skills.managed_dir.ends_with("skills"));
        assert!(config.skills.cache_dir.ends_with("cache"));
    }

    #[test]
    fn test_skills_config_serde_roundtrip() {
        let config = SkillsConfig {
            managed_dir: PathBuf::from("/tmp/skills"),
            clawhub_api: "https://test.clawhub.com".to_string(),
            cache_dir: PathBuf::from("/tmp/cache"),
            pinned_versions: Default::default(),
            max_skill_context_chars: 50_000,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: SkillsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.managed_dir, PathBuf::from("/tmp/skills"));
        assert_eq!(parsed.clawhub_api, "https://test.clawhub.com");
        assert_eq!(parsed.cache_dir, PathBuf::from("/tmp/cache"));
    }

    #[test]
    fn compat_config_defaults_to_enabled() {
        let compat = CompatConfig::default();
        assert!(compat.openclaw_config);
    }

    #[test]
    fn mcclawd_config_default_includes_compat() {
        let config = McclawdConfig::default();
        assert!(config.compat.openclaw_config);
    }

    #[test]
    fn compat_config_deserializes_from_empty_toml() {
        let toml_str = "";
        let config: McclawdConfig = toml::from_str(toml_str).unwrap();
        assert!(config.compat.openclaw_config);
    }

    #[test]
    fn compat_config_deserializes_disabled() {
        let toml_str = r#"
[compat]
openclaw_config = false
"#;
        let config: McclawdConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.compat.openclaw_config);
    }

    #[test]
    fn detect_openclaw_config_does_not_panic() {
        // Verifies the function runs without panicking.
        // Actual file presence depends on the environment.
        let _result = detect_openclaw_config();
    }

    #[test]
    fn test_config_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut config = McclawdConfig::default();
        config.agent.model = "gpt-4o".to_string();
        config.agent.max_turns = 42;
        config.agent.default_workspace = "myws".to_string();

        config.save(&path).unwrap();
        let loaded = McclawdConfig::load(&path).unwrap();

        assert_eq!(loaded.agent.model, "gpt-4o");
        assert_eq!(loaded.agent.max_turns, 42);
        assert_eq!(loaded.agent.default_workspace, "myws");
    }

    #[test]
    fn test_config_save_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("config.toml");

        let config = McclawdConfig::default();
        config.save(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_config_save_preserves_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut config = McclawdConfig::default();
        config.agent.model = "custom-model".to_string();
        config.mcp.agentgateway_url = "http://custom:5000".to_string();

        config.save(&path).unwrap();
        let loaded = McclawdConfig::load(&path).unwrap();

        assert_eq!(loaded.agent.model, "custom-model");
        assert_eq!(loaded.mcp.agentgateway_url, "http://custom:5000");
    }
}
