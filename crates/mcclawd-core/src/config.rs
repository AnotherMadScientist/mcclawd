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
        }
    }
}
