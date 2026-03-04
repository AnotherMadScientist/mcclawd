use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McclawdConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    #[serde(default)]
    pub agent: AgentConfig,

    #[serde(default)]
    pub providers: ProvidersConfig,

    #[serde(default)]
    pub mcp: McpConfig,
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
            agent: AgentConfig::default(),
            providers: ProvidersConfig::default(),
            mcp: McpConfig::default(),
        }
    }
}
