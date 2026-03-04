# McClawd v5 Phase 0: "One Agent Completes a Task" Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** A single Rust binary (`mc`) that takes a natural language prompt, routes it through a CLI channel to an agent backed by Anthropic (via Rig), calls MCP tools (via rmcp/AgentGateway), streams response tokens to stdout, with secrets encrypted at rest and workspace identity files (SOUL.md/AGENTS.md/USER.md) loaded into context.

**Architecture:** Cargo workspace with 6 crates. `mcclawd-core` provides shared types, config, secrets (AES-256-GCM-SIV), identity (JWT), and hook traits. `mcclawd-agent` owns context assembly and delegates the agent loop to Rig's built-in multi-turn tool-calling agent. `mcclawd-tools` wraps MCP tools via rmcp and provides builtins (memory.store/recall). `mcclawd-channels` defines the Channel trait, InboundPipeline, and CLI adapter. `mcclawd-tasks` provides task lifecycle. `mcclawd-api` is the binary crate with CLI entrypoint.

**Tech Stack:** Rust 2024 edition, rig-core 0.31+ (Anthropic provider, tool calling, streaming), rmcp (MCP client), tokio, axum (future), aes-gcm-siv, argon2, jsonwebtoken, serde/serde_json, toml, tracing, clap.

**Key Simplification:** Rig already provides a multi-turn agent loop via `.prompt().max_turns(N)` with tool dispatch. We do NOT write a manual ReAct loop. Instead, we implement Rig's `Tool` trait for our tools and build the agent using Rig's builder. The v5 doc's `engine.rs` ReAct loop is replaced by Rig's native agent + PromptHook for our SecurityHook integration.

---

## Task 1: Workspace scaffold + workspace Cargo.toml

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/mcclawd-core/Cargo.toml`
- Create: `crates/mcclawd-core/src/lib.rs`
- Create: `crates/mcclawd-agent/Cargo.toml`
- Create: `crates/mcclawd-agent/src/lib.rs`
- Create: `crates/mcclawd-tools/Cargo.toml`
- Create: `crates/mcclawd-tools/src/lib.rs`
- Create: `crates/mcclawd-channels/Cargo.toml`
- Create: `crates/mcclawd-channels/src/lib.rs`
- Create: `crates/mcclawd-tasks/Cargo.toml`
- Create: `crates/mcclawd-tasks/src/lib.rs`
- Create: `crates/mcclawd-api/Cargo.toml`
- Create: `crates/mcclawd-api/src/main.rs`

**Step 1: Create workspace root Cargo.toml**

```toml
[workspace]
resolver = "2"
members = [
    "crates/mcclawd-core",
    "crates/mcclawd-agent",
    "crates/mcclawd-tools",
    "crates/mcclawd-channels",
    "crates/mcclawd-tasks",
    "crates/mcclawd-api",
]

[workspace.package]
edition = "2024"
version = "0.1.0"
license = "MIT"

[workspace.dependencies]
# LLM layer
rig-core = { version = "0.31", features = ["anthropic"] }

# MCP
rmcp = { version = "0.1", features = ["client", "transport-child-process", "transport-sse-client"] }

# Async runtime
tokio = { version = "1", features = ["full"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"

# Config
toml = "0.8"

# Error handling
thiserror = "2"
anyhow = "1"

# Tracing
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# YAML frontmatter (SKILL.md)
gray_matter = "0.2"

# Secrets
aes-gcm-siv = "0.11"
argon2 = "0.5"
zeroize = { version = "1", features = ["derive"] }
rand = "0.8"

# Identity
jsonwebtoken = "9"
chrono = { version = "0.4", features = ["serde"] }

# CLI
clap = { version = "4", features = ["derive"] }

# Concurrent state
dashmap = "6"

# Async trait
async-trait = "0.1"

# UUID
uuid = { version = "1", features = ["v4"] }

# Futures
futures = "0.3"

# Internal crates
mcclawd-core = { path = "crates/mcclawd-core" }
mcclawd-agent = { path = "crates/mcclawd-agent" }
mcclawd-tools = { path = "crates/mcclawd-tools" }
mcclawd-channels = { path = "crates/mcclawd-channels" }
mcclawd-tasks = { path = "crates/mcclawd-tasks" }
mcclawd-api = { path = "crates/mcclawd-api" }
```

**Step 2: Create each crate's Cargo.toml and stub lib.rs/main.rs**

`crates/mcclawd-core/Cargo.toml`:
```toml
[package]
name = "mcclawd-core"
edition.workspace = true
version.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
toml = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
aes-gcm-siv = { workspace = true }
argon2 = { workspace = true }
zeroize = { workspace = true }
rand = { workspace = true }
jsonwebtoken = { workspace = true }

[dev-dependencies]
tokio = { workspace = true }
tempfile = "3"
```

`crates/mcclawd-agent/Cargo.toml`:
```toml
[package]
name = "mcclawd-agent"
edition.workspace = true
version.workspace = true

[dependencies]
mcclawd-core = { workspace = true }
mcclawd-tools = { workspace = true }
rig-core = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
tokio = { workspace = true }
futures = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

`crates/mcclawd-tools/Cargo.toml`:
```toml
[package]
name = "mcclawd-tools"
edition.workspace = true
version.workspace = true

[dependencies]
mcclawd-core = { workspace = true }
rig-core = { workspace = true }
rmcp = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
tokio = { workspace = true }
dashmap = { workspace = true }
gray_matter = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

`crates/mcclawd-channels/Cargo.toml`:
```toml
[package]
name = "mcclawd-channels"
edition.workspace = true
version.workspace = true

[dependencies]
mcclawd-core = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
tokio = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
futures = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

`crates/mcclawd-tasks/Cargo.toml`:
```toml
[package]
name = "mcclawd-tasks"
edition.workspace = true
version.workspace = true

[dependencies]
mcclawd-core = { workspace = true }
mcclawd-agent = { workspace = true }
mcclawd-tools = { workspace = true }
mcclawd-channels = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
tokio = { workspace = true }
uuid = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

`crates/mcclawd-api/Cargo.toml`:
```toml
[package]
name = "mcclawd-api"
edition.workspace = true
version.workspace = true

[[bin]]
name = "mc"
path = "src/main.rs"

[dependencies]
mcclawd-core = { workspace = true }
mcclawd-agent = { workspace = true }
mcclawd-tools = { workspace = true }
mcclawd-channels = { workspace = true }
mcclawd-tasks = { workspace = true }
clap = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
tokio = { workspace = true }
futures = { workspace = true }
```

Each `src/lib.rs` starts with:
```rust
//! McClawd <crate-name>
```

`crates/mcclawd-api/src/main.rs`:
```rust
fn main() {
    println!("McClawd v0.1.0");
}
```

**Step 3: Verify workspace compiles**

Run: `cd /Users/velniukas/dev/macleodlabs/mcclawd && cargo build`
Expected: All 6 crates compile with no errors.

**Step 4: Initialize git and commit**

```bash
cd /Users/velniukas/dev/macleodlabs/mcclawd
git init
echo '/target\n*.swp\n*.swo\n.DS_Store' > .gitignore
git add Cargo.toml crates/ .gitignore docs/
git commit -m "feat: scaffold cargo workspace with 6 crates (Phase 0)"
```

---

## Task 2: mcclawd-core — Types, Config, Error

**Files:**
- Create: `crates/mcclawd-core/src/types.rs`
- Create: `crates/mcclawd-core/src/config.rs`
- Create: `crates/mcclawd-core/src/error.rs`
- Create: `crates/mcclawd-core/src/hooks.rs`
- Modify: `crates/mcclawd-core/src/lib.rs`
- Test: `crates/mcclawd-core/tests/config_test.rs`

**Step 1: Write types.rs**

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl TaskId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

**Step 2: Write error.rs**

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum McclawdError {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Secret error: {0}")]
    Secret(String),

    #[error("Identity error: {0}")]
    Identity(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, McclawdError>;
```

**Step 3: Write config.rs**

```rust
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

fn default_max_turns() -> usize { 20 }
fn default_model() -> String { "claude-sonnet-4-5".to_string() }
fn default_workspace() -> String { "default".to_string() }
fn default_ollama_url() -> String { "http://localhost:11434".to_string() }

impl McclawdConfig {
    pub fn load(path: &Path) -> crate::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)
                .map_err(|e| crate::McclawdError::Config(e.to_string()))?;
            toml::from_str(&content)
                .map_err(|e| crate::McclawdError::Config(e.to_string()))
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
```

Note: Add `dirs = "6"` to mcclawd-core's Cargo.toml dependencies.

**Step 4: Write hooks.rs**

```rust
use async_trait::async_trait;

/// Hook called before/after tool dispatch. Phase 0: audit logging via tracing.
/// Phase 3+: DLP scanning, secret detection, taint tracking.
#[async_trait]
pub trait SecurityHook: Send + Sync {
    async fn before_tool_call(&self, tool_name: &str, args: &serde_json::Value) -> crate::Result<()>;
    async fn after_tool_call(&self, tool_name: &str, result: &serde_json::Value) -> crate::Result<()>;
}

/// Phase 0 implementation: logs tool calls via tracing.
pub struct AuditHook;

#[async_trait]
impl SecurityHook for AuditHook {
    async fn before_tool_call(&self, tool_name: &str, args: &serde_json::Value) -> crate::Result<()> {
        tracing::info!(tool = %tool_name, args = %args, "tool_call_start");
        Ok(())
    }

    async fn after_tool_call(&self, tool_name: &str, result: &serde_json::Value) -> crate::Result<()> {
        tracing::info!(tool = %tool_name, result_size = result.to_string().len(), "tool_call_end");
        Ok(())
    }
}
```

**Step 5: Update lib.rs to export modules**

```rust
pub mod types;
pub mod config;
pub mod error;
pub mod hooks;

pub use error::{McclawdError, Result};
pub use types::{TaskId, AgentId, SessionId};
pub use config::McclawdConfig;
```

**Step 6: Write test**

```rust
// crates/mcclawd-core/tests/config_test.rs
use mcclawd_core::config::McclawdConfig;
use tempfile::NamedTempFile;
use std::io::Write;

#[test]
fn test_default_config() {
    let config = McclawdConfig::default();
    assert_eq!(config.agent.max_turns, 20);
    assert_eq!(config.agent.default_workspace, "default");
}

#[test]
fn test_load_config_from_toml() {
    let mut f = NamedTempFile::new().unwrap();
    writeln!(f, r#"
[agent]
max_turns = 10
model = "claude-opus-4-5"
"#).unwrap();

    let config = McclawdConfig::load(f.path()).unwrap();
    assert_eq!(config.agent.max_turns, 10);
    assert_eq!(config.agent.model, "claude-opus-4-5");
}

#[test]
fn test_load_missing_config_returns_default() {
    let config = McclawdConfig::load(std::path::Path::new("/nonexistent/config.toml")).unwrap();
    assert_eq!(config.agent.max_turns, 20);
}
```

**Step 7: Run tests**

Run: `cargo test -p mcclawd-core`
Expected: 3 tests pass.

**Step 8: Commit**

```bash
git add crates/mcclawd-core/
git commit -m "feat(core): types, config, error, hooks"
```

---

## Task 3: mcclawd-core — Secrets (Encrypted File Backend)

**Files:**
- Create: `crates/mcclawd-core/src/secrets/mod.rs`
- Create: `crates/mcclawd-core/src/secrets/encrypted_file.rs`
- Modify: `crates/mcclawd-core/src/lib.rs` (add `pub mod secrets;`)
- Test: `crates/mcclawd-core/tests/secrets_test.rs`

**Step 1: Write the failing test**

```rust
// crates/mcclawd-core/tests/secrets_test.rs
use mcclawd_core::secrets::{SecretBackend, EncryptedFileBackend};
use tempfile::TempDir;

#[tokio::test]
async fn test_set_and_get_secret() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    backend.set("API_KEY", "sk-test-123").await.unwrap();
    let value = backend.get("API_KEY").await.unwrap();
    assert_eq!(value, Some("sk-test-123".to_string()));
}

#[tokio::test]
async fn test_get_missing_secret() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    let value = backend.get("NONEXISTENT").await.unwrap();
    assert_eq!(value, None);
}

#[tokio::test]
async fn test_list_secrets() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    backend.set("KEY_A", "val_a").await.unwrap();
    backend.set("KEY_B", "val_b").await.unwrap();

    let keys = backend.list().await.unwrap();
    assert!(keys.contains(&"KEY_A".to_string()));
    assert!(keys.contains(&"KEY_B".to_string()));
}

#[tokio::test]
async fn test_delete_secret() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    backend.set("KEY", "value").await.unwrap();
    backend.delete("KEY").await.unwrap();
    let value = backend.get("KEY").await.unwrap();
    assert_eq!(value, None);
}

#[tokio::test]
async fn test_persistence_across_instances() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");

    {
        let backend = EncryptedFileBackend::new(&path, "passphrase").unwrap();
        backend.set("PERSIST_KEY", "persist_value").await.unwrap();
    }

    {
        let backend = EncryptedFileBackend::new(&path, "passphrase").unwrap();
        let value = backend.get("PERSIST_KEY").await.unwrap();
        assert_eq!(value, Some("persist_value".to_string()));
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p mcclawd-core -- secrets`
Expected: FAIL — module `secrets` not found.

**Step 3: Write secrets/mod.rs (trait)**

```rust
use async_trait::async_trait;

pub mod encrypted_file;
pub use encrypted_file::EncryptedFileBackend;

/// Trait for secret storage backends.
/// Phase 0: EncryptedFileBackend (AES-256-GCM-SIV + argon2).
/// Future: VaultBackend, KeychainBackend.
#[async_trait]
pub trait SecretBackend: Send + Sync {
    async fn get(&self, key: &str) -> crate::Result<Option<String>>;
    async fn set(&self, key: &str, value: &str) -> crate::Result<()>;
    async fn delete(&self, key: &str) -> crate::Result<()>;
    async fn list(&self) -> crate::Result<Vec<String>>;
}
```

**Step 4: Write secrets/encrypted_file.rs**

```rust
use aes_gcm_siv::{
    aead::{Aead, KeyInit, OsRng},
    Aes256GcmSiv, Nonce,
};
use argon2::Argon2;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use zeroize::Zeroizing;

use super::SecretBackend;
use crate::{McclawdError, Result};

/// Encrypted file-based secret storage.
/// Secrets are stored as JSON encrypted with AES-256-GCM-SIV.
/// The encryption key is derived from a passphrase via argon2.
pub struct EncryptedFileBackend {
    path: PathBuf,
    key: Zeroizing<[u8; 32]>,
    cache: RwLock<HashMap<String, String>>,
}

impl EncryptedFileBackend {
    pub fn new(path: &Path, passphrase: &str) -> Result<Self> {
        let key = derive_key(passphrase)?;
        let mut backend = Self {
            path: path.to_path_buf(),
            key: Zeroizing::new(key),
            cache: RwLock::new(HashMap::new()),
        };
        backend.load_from_disk()?;
        Ok(backend)
    }

    fn load_from_disk(&mut self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        let ciphertext = std::fs::read(&self.path)
            .map_err(|e| McclawdError::Secret(format!("Failed to read secrets file: {e}")))?;
        if ciphertext.len() < 12 {
            return Err(McclawdError::Secret("Secrets file too short".into()));
        }
        let (nonce_bytes, encrypted) = ciphertext.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let cipher = Aes256GcmSiv::new_from_slice(self.key.as_ref())
            .map_err(|e| McclawdError::Secret(format!("Cipher init error: {e}")))?;
        let plaintext = cipher
            .decrypt(nonce, encrypted)
            .map_err(|e| McclawdError::Secret(format!("Decryption failed: {e}")))?;
        let map: HashMap<String, String> = serde_json::from_slice(&plaintext)?;
        *self.cache.get_mut() = map;
        Ok(())
    }

    async fn save_to_disk(&self) -> Result<()> {
        let cache = self.cache.read().await;
        let plaintext = serde_json::to_vec(&*cache)?;
        let cipher = Aes256GcmSiv::new_from_slice(self.key.as_ref())
            .map_err(|e| McclawdError::Secret(format!("Cipher init error: {e}")))?;
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| McclawdError::Secret(format!("Encryption failed: {e}")))?;
        let mut output = nonce_bytes.to_vec();
        output.extend_from_slice(&ciphertext);

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| McclawdError::Secret(format!("Failed to create dir: {e}")))?;
        }
        std::fs::write(&self.path, &output)
            .map_err(|e| McclawdError::Secret(format!("Failed to write secrets: {e}")))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl SecretBackend for EncryptedFileBackend {
    async fn get(&self, key: &str) -> Result<Option<String>> {
        let cache = self.cache.read().await;
        Ok(cache.get(key).cloned())
    }

    async fn set(&self, key: &str, value: &str) -> Result<()> {
        {
            let mut cache = self.cache.write().await;
            cache.insert(key.to_string(), value.to_string());
        }
        self.save_to_disk().await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        {
            let mut cache = self.cache.write().await;
            cache.remove(key);
        }
        self.save_to_disk().await
    }

    async fn list(&self) -> Result<Vec<String>> {
        let cache = self.cache.read().await;
        Ok(cache.keys().cloned().collect())
    }
}

fn derive_key(passphrase: &str) -> Result<[u8; 32]> {
    let salt = b"mcclawd-secrets-v1"; // Fixed salt — acceptable for local-only use
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| McclawdError::Secret(format!("Key derivation failed: {e}")))?;
    Ok(key)
}
```

Note: Add `tokio = { workspace = true }` and `dirs = "6"` to mcclawd-core Cargo.toml if not already present.

**Step 5: Update lib.rs**

Add `pub mod secrets;` to `crates/mcclawd-core/src/lib.rs`.

**Step 6: Run tests**

Run: `cargo test -p mcclawd-core -- secrets`
Expected: 5 tests pass.

**Step 7: Commit**

```bash
git add crates/mcclawd-core/src/secrets/ crates/mcclawd-core/tests/secrets_test.rs crates/mcclawd-core/src/lib.rs crates/mcclawd-core/Cargo.toml
git commit -m "feat(core): encrypted file secret backend (AES-256-GCM-SIV + argon2)"
```

---

## Task 4: mcclawd-core — Identity (JWT)

**Files:**
- Create: `crates/mcclawd-core/src/identity/mod.rs`
- Create: `crates/mcclawd-core/src/identity/jwt.rs`
- Modify: `crates/mcclawd-core/src/lib.rs` (add `pub mod identity;`)
- Test: `crates/mcclawd-core/tests/identity_test.rs`

**Step 1: Write the failing test**

```rust
// crates/mcclawd-core/tests/identity_test.rs
use mcclawd_core::identity::{IdentityProvider, JwtIdentityProvider};
use mcclawd_core::types::AgentId;

#[tokio::test]
async fn test_issue_and_verify_token() {
    let provider = JwtIdentityProvider::new("test-secret-key");
    let agent = AgentId("coding".to_string());
    let token = provider.issue(&agent).await.unwrap();
    let claims = provider.verify(&token).await.unwrap();
    assert_eq!(claims.agent_id, "coding");
}

#[tokio::test]
async fn test_invalid_token_fails() {
    let provider = JwtIdentityProvider::new("test-secret-key");
    let result = provider.verify("invalid-token").await;
    assert!(result.is_err());
}
```

**Step 2: Write identity/mod.rs**

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::types::AgentId;

pub mod jwt;
pub use jwt::JwtIdentityProvider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentClaims {
    pub agent_id: String,
    pub iat: i64,
    pub exp: i64,
}

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    async fn issue(&self, agent: &AgentId) -> crate::Result<String>;
    async fn verify(&self, token: &str) -> crate::Result<AgentClaims>;
}
```

**Step 3: Write identity/jwt.rs**

```rust
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use crate::identity::{AgentClaims, IdentityProvider};
use crate::types::AgentId;
use crate::{McclawdError, Result};

pub struct JwtIdentityProvider {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtIdentityProvider {
    pub fn new(secret: &str) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
        }
    }
}

#[async_trait::async_trait]
impl IdentityProvider for JwtIdentityProvider {
    async fn issue(&self, agent: &AgentId) -> Result<String> {
        let now = chrono::Utc::now().timestamp();
        let claims = AgentClaims {
            agent_id: agent.0.clone(),
            iat: now,
            exp: now + 3600, // 1 hour
        };
        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| McclawdError::Identity(format!("JWT encode failed: {e}")))
    }

    async fn verify(&self, token: &str) -> Result<AgentClaims> {
        let data = decode::<AgentClaims>(token, &self.decoding_key, &Validation::default())
            .map_err(|e| McclawdError::Identity(format!("JWT verify failed: {e}")))?;
        Ok(data.claims)
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p mcclawd-core -- identity`
Expected: 2 tests pass.

**Step 5: Commit**

```bash
git add crates/mcclawd-core/src/identity/ crates/mcclawd-core/tests/identity_test.rs crates/mcclawd-core/src/lib.rs
git commit -m "feat(core): JWT identity provider"
```

---

## Task 5: mcclawd-agent — Workspace Loader

**Files:**
- Create: `crates/mcclawd-agent/src/workspace.rs`
- Create: `crates/mcclawd-agent/src/agents_parser.rs`
- Modify: `crates/mcclawd-agent/src/lib.rs`
- Test: `crates/mcclawd-agent/tests/workspace_test.rs`

**Step 1: Write the failing test**

```rust
// crates/mcclawd-agent/tests/workspace_test.rs
use mcclawd_agent::workspace::{Workspace, WorkspaceLoader};
use tempfile::TempDir;
use std::fs;

#[test]
fn test_load_workspace_with_all_files() {
    let dir = TempDir::new().unwrap();
    let ws_dir = dir.path().join("workspaces").join("default");
    fs::create_dir_all(&ws_dir).unwrap();
    fs::write(ws_dir.join("SOUL.md"), "# Soul\nYou are McClawd.").unwrap();
    fs::write(ws_dir.join("AGENTS.md"), "# Agents\n## Default Skills\n- memory").unwrap();
    fs::write(ws_dir.join("USER.md"), "# User\nName: Test User").unwrap();

    let loader = WorkspaceLoader::new(dir.path().join("workspaces"));
    let ws = loader.load("default").unwrap();

    assert!(ws.soul.is_some());
    assert!(ws.agents.is_some());
    assert!(ws.user.is_some());
    assert!(ws.soul.unwrap().contains("McClawd"));
}

#[test]
fn test_load_workspace_missing_optional_files() {
    let dir = TempDir::new().unwrap();
    let ws_dir = dir.path().join("workspaces").join("minimal");
    fs::create_dir_all(&ws_dir).unwrap();
    fs::write(ws_dir.join("SOUL.md"), "# Soul\nMinimal agent.").unwrap();

    let loader = WorkspaceLoader::new(dir.path().join("workspaces"));
    let ws = loader.load("minimal").unwrap();

    assert!(ws.soul.is_some());
    assert!(ws.agents.is_none());
    assert!(ws.user.is_none());
}

#[test]
fn test_load_nonexistent_workspace_fails() {
    let dir = TempDir::new().unwrap();
    let loader = WorkspaceLoader::new(dir.path().join("workspaces"));
    let result = loader.load("nonexistent");
    assert!(result.is_err());
}
```

**Step 2: Write workspace.rs**

```rust
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Workspace {
    pub name: String,
    pub soul: Option<String>,
    pub agents: Option<String>,
    pub user: Option<String>,
    pub path: PathBuf,
}

pub struct WorkspaceLoader {
    base_dir: PathBuf,
}

impl WorkspaceLoader {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn load(&self, name: &str) -> mcclawd_core::Result<Workspace> {
        let ws_path = self.base_dir.join(name);
        if !ws_path.exists() {
            return Err(mcclawd_core::McclawdError::Config(
                format!("Workspace '{}' not found at {}", name, ws_path.display()),
            ));
        }

        Ok(Workspace {
            name: name.to_string(),
            soul: read_optional(&ws_path.join("SOUL.md")),
            agents: read_optional(&ws_path.join("AGENTS.md")),
            user: read_optional(&ws_path.join("USER.md")),
            path: ws_path,
        })
    }

    pub fn scaffold(&self, name: &str) -> mcclawd_core::Result<PathBuf> {
        let ws_path = self.base_dir.join(name);
        std::fs::create_dir_all(&ws_path)?;

        let soul = "# Soul\n\nYou are McClawd, a security-focused AI assistant.\n\n\
            ## Personality\n- Direct and technical.\n- When uncertain, say so.\n\n\
            ## Rules\n- Never execute destructive operations without confirmation.\n\
            - Never store secrets in plaintext.\n";

        let agents = "# Agents\n\n## Default Skills\n- memory-management\n\n\
            ## Available Agents\n\n### default\n- **Specialty:** General purpose\n\
            - **Model:** claude-sonnet-4-5\n";

        let user = "# User\n\n## Preferences\n- Concise responses\n";

        std::fs::write(ws_path.join("SOUL.md"), soul)?;
        std::fs::write(ws_path.join("AGENTS.md"), agents)?;
        std::fs::write(ws_path.join("USER.md"), user)?;

        Ok(ws_path)
    }
}

fn read_optional(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}
```

**Step 3: Write agents_parser.rs**

```rust
/// Parsed from AGENTS.md markdown.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub id: String,
    pub specialty: Option<String>,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub delegate_when: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentsConfig {
    pub default_skills: Vec<String>,
    pub agents: Vec<AgentSpec>,
    pub delegation_rules: Vec<String>,
    pub raw_markdown: String,
}

impl AgentsConfig {
    pub fn parse(markdown: &str) -> Self {
        let mut default_skills = vec![];
        let mut agents = vec![];
        let mut delegation_rules = vec![];
        let mut current_agent: Option<AgentSpec> = None;
        let mut section = Section::None;
        let mut sub_field = SubField::None;

        for line in markdown.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("## Default Skills") {
                flush_agent(&mut current_agent, &mut agents);
                section = Section::DefaultSkills;
                sub_field = SubField::None;
                continue;
            }
            if trimmed.starts_with("## Available Agents") {
                flush_agent(&mut current_agent, &mut agents);
                section = Section::Agents;
                sub_field = SubField::None;
                continue;
            }
            if trimmed.starts_with("## Delegation Rules") {
                flush_agent(&mut current_agent, &mut agents);
                section = Section::DelegationRules;
                sub_field = SubField::None;
                continue;
            }
            if trimmed.starts_with("## ") {
                flush_agent(&mut current_agent, &mut agents);
                section = Section::None;
                sub_field = SubField::None;
                continue;
            }

            // New agent heading (### <id>)
            if trimmed.starts_with("### ") && section == Section::Agents {
                flush_agent(&mut current_agent, &mut agents);
                let id = trimmed.trim_start_matches("### ").trim().to_lowercase();
                current_agent = Some(AgentSpec {
                    id,
                    specialty: None,
                    model: None,
                    tools: vec![],
                    skills: vec![],
                    delegate_when: None,
                });
                sub_field = SubField::None;
                continue;
            }

            // Parse bullet items
            if let Some(item) = trimmed.strip_prefix("- ") {
                match section {
                    Section::DefaultSkills => {
                        default_skills.push(item.trim().to_string());
                    }
                    Section::DelegationRules => {
                        delegation_rules.push(item.trim().to_string());
                    }
                    Section::Agents => {
                        if let Some(ref mut agent) = current_agent {
                            if let Some(val) = item.strip_prefix("**Specialty:**") {
                                agent.specialty = Some(val.trim().to_string());
                                sub_field = SubField::None;
                            } else if let Some(val) = item.strip_prefix("**Model:**") {
                                agent.model = Some(val.trim().to_string());
                                sub_field = SubField::None;
                            } else if item.starts_with("**Tools:**") {
                                let val = item.strip_prefix("**Tools:**").unwrap_or("").trim();
                                if !val.is_empty() {
                                    agent.tools = val.split(',').map(|s| s.trim().to_string()).collect();
                                }
                                sub_field = SubField::Tools;
                            } else if item.starts_with("**Skills:**") {
                                sub_field = SubField::Skills;
                            } else if let Some(val) = item.strip_prefix("**Delegate when:**") {
                                agent.delegate_when = Some(val.trim().to_string());
                                sub_field = SubField::None;
                            } else {
                                // Sub-bullet for skills or tools
                                match sub_field {
                                    SubField::Skills => agent.skills.push(item.trim().to_string()),
                                    SubField::Tools => agent.tools.push(item.trim().to_string()),
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Indented sub-bullet (  - item)
            if let Some(item) = trimmed.strip_prefix("  - ") {
                // This catches sub-bullets under Skills: or Tools:
                // Already handled above since we trim
            }
        }

        flush_agent(&mut current_agent, &mut agents);

        Self {
            default_skills,
            agents,
            delegation_rules,
            raw_markdown: markdown.to_string(),
        }
    }

    pub fn skills_for(&self, agent_id: &str) -> Vec<String> {
        let mut skills = self.default_skills.clone();
        if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
            skills.extend(agent.skills.clone());
        }
        skills.dedup();
        skills
    }

    pub fn agent_spec(&self, agent_id: &str) -> Option<&AgentSpec> {
        self.agents.iter().find(|a| a.id == agent_id)
    }
}

#[derive(PartialEq)]
enum Section { None, DefaultSkills, Agents, DelegationRules }

#[derive(PartialEq)]
enum SubField { None, Skills, Tools }

fn flush_agent(current: &mut Option<AgentSpec>, agents: &mut Vec<AgentSpec>) {
    if let Some(agent) = current.take() {
        agents.push(agent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agents_md() {
        let md = r#"# Agents

## Default Skills
- memory-management
- task-status

## Available Agents

### coding
- **Specialty:** Code generation, debugging
- **Model:** claude-sonnet-4-5
- **Tools:** exec, read, write
- **Skills:**
  - git-workflow
  - code-review
- **Delegate when:** User asks for code changes

### research
- **Specialty:** Deep research
- **Model:** claude-opus-4-5
- **Skills:**
  - academic-search

## Delegation Rules
- Always confirm before delegating
"#;

        let config = AgentsConfig::parse(md);
        assert_eq!(config.default_skills, vec!["memory-management", "task-status"]);
        assert_eq!(config.agents.len(), 2);

        let coding = config.agent_spec("coding").unwrap();
        assert_eq!(coding.model.as_deref(), Some("claude-sonnet-4-5"));
        assert!(coding.skills.contains(&"git-workflow".to_string()));

        let skills = config.skills_for("coding");
        assert!(skills.contains(&"memory-management".to_string()));
        assert!(skills.contains(&"git-workflow".to_string()));

        assert_eq!(config.delegation_rules.len(), 1);
    }
}
```

**Step 4: Update lib.rs**

```rust
pub mod workspace;
pub mod agents_parser;
```

**Step 5: Run tests**

Run: `cargo test -p mcclawd-agent`
Expected: All tests pass (workspace + agents_parser).

**Step 6: Commit**

```bash
git add crates/mcclawd-agent/
git commit -m "feat(agent): workspace loader + AGENTS.md parser"
```

---

## Task 6: mcclawd-tools — Builtin Memory Tool + MCP Client

**Files:**
- Create: `crates/mcclawd-tools/src/registry.rs`
- Create: `crates/mcclawd-tools/src/builtin/mod.rs`
- Create: `crates/mcclawd-tools/src/builtin/memory.rs`
- Create: `crates/mcclawd-tools/src/mcp.rs`
- Modify: `crates/mcclawd-tools/src/lib.rs`
- Test: `crates/mcclawd-tools/tests/memory_test.rs`

**Step 1: Write the failing test for memory tool**

```rust
// crates/mcclawd-tools/tests/memory_test.rs
use mcclawd_tools::builtin::memory::{MemoryStore, MemoryRecall};
use rig::tool::Tool;

#[tokio::test]
async fn test_memory_store_and_recall() {
    let store_tool = MemoryStore::new_shared();
    let recall_tool = MemoryRecall::from_shared(&store_tool);

    // Store
    let store_args = serde_json::from_value(serde_json::json!({
        "key": "user_name",
        "value": "Alice"
    })).unwrap();
    let result = store_tool.call(store_args).await.unwrap();
    assert!(result.contains("Stored"));

    // Recall
    let recall_args = serde_json::from_value(serde_json::json!({
        "key": "user_name"
    })).unwrap();
    let result = recall_tool.call(recall_args).await.unwrap();
    assert!(result.contains("Alice"));
}

#[tokio::test]
async fn test_memory_recall_missing_key() {
    let store_tool = MemoryStore::new_shared();
    let recall_tool = MemoryRecall::from_shared(&store_tool);

    let recall_args = serde_json::from_value(serde_json::json!({
        "key": "nonexistent"
    })).unwrap();
    let result = recall_tool.call(recall_args).await.unwrap();
    assert!(result.contains("not found") || result.contains("No value"));
}
```

**Step 2: Write builtin/memory.rs**

```rust
use dashmap::DashMap;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
#[error("Memory error: {0}")]
pub struct MemoryError(String);

// --- memory.store ---

#[derive(Deserialize)]
pub struct StoreArgs {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MemoryStore {
    #[serde(skip)]
    pub store: Arc<DashMap<String, String>>,
}

impl MemoryStore {
    pub fn new_shared() -> Self {
        Self {
            store: Arc::new(DashMap::new()),
        }
    }
}

impl Tool for MemoryStore {
    const NAME: &'static str = "memory_store";
    type Error = MemoryError;
    type Args = StoreArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        serde_json::from_value(json!({
            "name": "memory_store",
            "description": "Store a key-value pair in working memory for the current session.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "The key to store" },
                    "value": { "type": "string", "description": "The value to store" }
                },
                "required": ["key", "value"]
            }
        }))
        .expect("valid tool definition")
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.store.insert(args.key.clone(), args.value);
        Ok(format!("Stored key '{}'", args.key))
    }
}

// --- memory.recall ---

#[derive(Deserialize)]
pub struct RecallArgs {
    pub key: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MemoryRecall {
    #[serde(skip)]
    pub store: Arc<DashMap<String, String>>,
}

impl MemoryRecall {
    pub fn from_shared(memory_store: &MemoryStore) -> Self {
        Self {
            store: memory_store.store.clone(),
        }
    }
}

impl Tool for MemoryRecall {
    const NAME: &'static str = "memory_recall";
    type Error = MemoryError;
    type Args = RecallArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        serde_json::from_value(json!({
            "name": "memory_recall",
            "description": "Recall a value from working memory by key.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "The key to recall" }
                },
                "required": ["key"]
            }
        }))
        .expect("valid tool definition")
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        match self.store.get(&args.key) {
            Some(value) => Ok(format!("{}", value.value())),
            None => Ok(format!("No value found for key '{}'", args.key)),
        }
    }
}
```

**Step 3: Write builtin/mod.rs**

```rust
pub mod memory;
```

**Step 4: Write mcp.rs (MCP client wrapper)**

```rust
use rmcp::{
    model::CallToolRequestParam,
    service::ServiceExt,
    transport::TokioChildProcess,
};
use tokio::process::Command;
use std::collections::HashMap;

/// Connects to an MCP server via stdio transport and wraps it for Rig tool dispatch.
pub struct McpConnection {
    // This will hold the rmcp service handle
    // For Phase 0, we establish the connection pattern.
    // Full integration with Rig's Tool trait happens when we wire up the agent.
}

impl McpConnection {
    /// Connect to an MCP server via AgentGateway SSE endpoint.
    pub async fn connect_sse(url: &str) -> anyhow::Result<()> {
        // Phase 0: establish the pattern. Full impl when AgentGateway is configured.
        tracing::info!(url = %url, "MCP SSE connection placeholder");
        Ok(())
    }

    /// Connect to an MCP server via stdio (child process).
    pub async fn connect_stdio(command: &str, args: &[&str]) -> anyhow::Result<()> {
        tracing::info!(command = %command, "MCP stdio connection placeholder");
        Ok(())
    }
}
```

**Step 5: Write lib.rs**

```rust
pub mod builtin;
pub mod mcp;
```

**Step 6: Run tests**

Run: `cargo test -p mcclawd-tools`
Expected: 2 tests pass.

**Step 7: Commit**

```bash
git add crates/mcclawd-tools/
git commit -m "feat(tools): memory.store/recall builtin tools + MCP client stub"
```

---

## Task 7: mcclawd-channels — Channel Trait + CLI Adapter

**Files:**
- Create: `crates/mcclawd-channels/src/types.rs`
- Create: `crates/mcclawd-channels/src/traits.rs`
- Create: `crates/mcclawd-channels/src/pipeline.rs`
- Create: `crates/mcclawd-channels/src/session.rs`
- Create: `crates/mcclawd-channels/src/cli.rs`
- Modify: `crates/mcclawd-channels/src/lib.rs`
- Test: `crates/mcclawd-channels/tests/pipeline_test.rs`

**Step 1: Write types.rs**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub id: String,
    pub channel: ChannelKind,
    pub peer: Peer,
    pub content: MessageContent,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub id: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    Text(String),
    Command { name: String, args: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutboundChunk {
    TextDelta(String),
    TextBlock(String),
    ToolStart { name: String },
    ToolEnd { name: String, summary: Option<String> },
    Done,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelKind {
    Cli,
    Web,
    Telegram,
    Discord,
    Custom(String),
}

impl std::fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelKind::Cli => write!(f, "cli"),
            ChannelKind::Web => write!(f, "web"),
            ChannelKind::Telegram => write!(f, "telegram"),
            ChannelKind::Discord => write!(f, "discord"),
            ChannelKind::Custom(name) => write!(f, "{}", name),
        }
    }
}
```

**Step 2: Write traits.rs**

```rust
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use crate::types::*;

#[async_trait]
pub trait Channel: Send + Sync + 'static {
    fn kind(&self) -> ChannelKind;

    async fn start(
        &self,
        inbound_tx: mpsc::Sender<InboundMessage>,
        shutdown: CancellationToken,
    ) -> mcclawd_core::Result<()>;

    async fn send_chunk(&self, chunk: OutboundChunk) -> mcclawd_core::Result<()>;
}
```

Note: Add `tokio-util = { version = "0.7", features = ["rt"] }` to workspace deps and mcclawd-channels Cargo.toml.

**Step 3: Write session.rs**

```rust
use mcclawd_core::types::SessionId;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub channel: String,
    pub peer_id: String,
}

pub struct SessionManager {
    sessions: HashMap<SessionKey, SessionId>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { sessions: HashMap::new() }
    }

    pub fn get_or_create(&mut self, key: SessionKey) -> SessionId {
        self.sessions
            .entry(key)
            .or_insert_with(SessionId::new)
            .clone()
    }
}
```

**Step 4: Write pipeline.rs**

```rust
use tokio::sync::mpsc;
use crate::types::InboundMessage;

/// Inbound pipeline: normalize → route → dispatch.
/// Phase 0: passthrough (single agent, single channel).
pub struct InboundPipeline {
    rx: mpsc::Receiver<InboundMessage>,
}

impl InboundPipeline {
    pub fn new(rx: mpsc::Receiver<InboundMessage>) -> Self {
        Self { rx }
    }

    pub async fn next(&mut self) -> Option<InboundMessage> {
        self.rx.recv().await
    }
}
```

**Step 5: Write cli.rs**

```rust
use crate::types::*;
use async_trait::async_trait;
use chrono::Utc;
use std::io::{self, BufRead, Write};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct CliChannel;

impl CliChannel {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl crate::traits::Channel for CliChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Cli
    }

    async fn start(
        &self,
        inbound_tx: mpsc::Sender<InboundMessage>,
        shutdown: CancellationToken,
    ) -> mcclawd_core::Result<()> {
        // CLI channel reads from stdin in a blocking thread
        let tx = inbound_tx.clone();
        let token = shutdown.clone();

        tokio::task::spawn_blocking(move || {
            let stdin = io::stdin();
            let mut reader = stdin.lock();
            loop {
                if token.is_cancelled() {
                    break;
                }
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim().to_string();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let msg = InboundMessage {
                            id: Uuid::new_v4().to_string(),
                            channel: ChannelKind::Cli,
                            peer: Peer {
                                id: "local".to_string(),
                                display_name: Some("User".to_string()),
                            },
                            content: MessageContent::Text(trimmed),
                            timestamp: Utc::now(),
                        };
                        if tx.blocking_send(msg).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(())
    }

    async fn send_chunk(&self, chunk: OutboundChunk) -> mcclawd_core::Result<()> {
        match chunk {
            OutboundChunk::TextDelta(text) => {
                print!("{}", text);
                io::stdout().flush().ok();
            }
            OutboundChunk::TextBlock(text) => {
                println!("{}", text);
            }
            OutboundChunk::ToolStart { name } => {
                eprintln!("[tool: {}]", name);
            }
            OutboundChunk::ToolEnd { name, summary } => {
                if let Some(s) = summary {
                    eprintln!("[/{}: {}]", name, s);
                }
            }
            OutboundChunk::Done => {
                println!();
            }
            OutboundChunk::Error(msg) => {
                eprintln!("Error: {}", msg);
            }
        }
        Ok(())
    }
}
```

**Step 6: Write lib.rs**

```rust
pub mod types;
pub mod traits;
pub mod pipeline;
pub mod session;
pub mod cli;

pub use types::*;
pub use traits::Channel;
pub use cli::CliChannel;
pub use pipeline::InboundPipeline;
pub use session::{SessionManager, SessionKey};
```

**Step 7: Write pipeline test**

```rust
// crates/mcclawd-channels/tests/pipeline_test.rs
use mcclawd_channels::{InboundMessage, InboundPipeline, ChannelKind, Peer, MessageContent};
use chrono::Utc;

#[tokio::test]
async fn test_pipeline_receives_messages() {
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let mut pipeline = InboundPipeline::new(rx);

    let msg = InboundMessage {
        id: "1".to_string(),
        channel: ChannelKind::Cli,
        peer: Peer { id: "user".to_string(), display_name: None },
        content: MessageContent::Text("hello".to_string()),
        timestamp: Utc::now(),
    };

    tx.send(msg).await.unwrap();
    let received = pipeline.next().await.unwrap();
    assert!(matches!(received.content, MessageContent::Text(ref t) if t == "hello"));
}
```

**Step 8: Run tests**

Run: `cargo test -p mcclawd-channels`
Expected: 1 test passes.

**Step 9: Commit**

```bash
git add crates/mcclawd-channels/
git commit -m "feat(channels): Channel trait, InboundPipeline, CLI adapter"
```

---

## Task 8: mcclawd-agent — Context Assembly + Agent Builder (Rig Integration)

**Files:**
- Create: `crates/mcclawd-agent/src/context.rs`
- Create: `crates/mcclawd-agent/src/engine.rs`
- Modify: `crates/mcclawd-agent/src/lib.rs`
- Test: `crates/mcclawd-agent/tests/context_test.rs`

**Step 1: Write the failing test**

```rust
// crates/mcclawd-agent/tests/context_test.rs
use mcclawd_agent::context::ContextBuilder;
use mcclawd_agent::workspace::Workspace;
use std::path::PathBuf;

#[test]
fn test_context_builds_system_prompt_with_soul_first() {
    let ws = Workspace {
        name: "test".to_string(),
        soul: Some("You are a test agent.".to_string()),
        agents: Some("# Agents\n## Default Skills\n- memory".to_string()),
        user: Some("# User\nName: Alice".to_string()),
        path: PathBuf::from("/tmp"),
    };

    let builder = ContextBuilder::new(ws);
    let prompt = builder.build_system_prompt();

    // SOUL.md must come first
    assert!(prompt.starts_with("You are a test agent."));
    // All sections present
    assert!(prompt.contains("Alice"));
    assert!(prompt.contains("memory"));
}

#[test]
fn test_context_handles_missing_optional_files() {
    let ws = Workspace {
        name: "minimal".to_string(),
        soul: Some("Minimal agent.".to_string()),
        agents: None,
        user: None,
        path: PathBuf::from("/tmp"),
    };

    let builder = ContextBuilder::new(ws);
    let prompt = builder.build_system_prompt();
    assert!(prompt.contains("Minimal agent."));
}
```

**Step 2: Write context.rs**

```rust
use crate::workspace::Workspace;

pub struct ContextBuilder {
    workspace: Workspace,
}

impl ContextBuilder {
    pub fn new(workspace: Workspace) -> Self {
        Self { workspace }
    }

    /// Build the system prompt from workspace files.
    /// Priority order: SOUL.md → USER.md → AGENTS.md → capabilities.
    pub fn build_system_prompt(&self) -> String {
        let mut sections = vec![];

        // 1. SOUL.md (always first)
        if let Some(soul) = &self.workspace.soul {
            sections.push(soul.clone());
        }

        // 2. USER.md
        if let Some(user) = &self.workspace.user {
            sections.push(format!("\n---\n\n{}", user));
        }

        // 3. AGENTS.md (informational in Phase 0)
        if let Some(agents) = &self.workspace.agents {
            sections.push(format!("\n---\n\n{}", agents));
        }

        sections.join("\n")
    }
}
```

**Step 3: Write engine.rs**

```rust
use crate::context::ContextBuilder;
use crate::workspace::Workspace;
use mcclawd_tools::builtin::memory::{MemoryRecall, MemoryStore};
use rig::providers::anthropic;

/// Build a Rig agent from workspace + tools.
/// Rig handles the multi-turn ReAct loop natively via .prompt().max_turns(N).
pub struct AgentBuilder;

impl AgentBuilder {
    /// Create a Rig agent configured with workspace context and tools.
    pub fn build(
        workspace: Workspace,
        api_key: &str,
        max_turns: usize,
    ) -> anyhow::Result<(
        rig::agent::Agent<anthropic::completion::CompletionModel>,
        MemoryStore,
    )> {
        let context = ContextBuilder::new(workspace);
        let system_prompt = context.build_system_prompt();

        let client = anthropic::Client::new(api_key);
        let memory_store = MemoryStore::new_shared();
        let memory_recall = MemoryRecall::from_shared(&memory_store);

        let agent = client
            .agent(anthropic::completion::CLAUDE_3_5_SONNET)
            .preamble(&system_prompt)
            .max_tokens(8192)
            .tool(memory_store.clone())
            .tool(memory_recall)
            .build();

        Ok((agent, memory_store))
    }
}
```

**Step 4: Update lib.rs**

```rust
pub mod workspace;
pub mod agents_parser;
pub mod context;
pub mod engine;
```

**Step 5: Run tests**

Run: `cargo test -p mcclawd-agent`
Expected: All tests pass.

**Step 6: Commit**

```bash
git add crates/mcclawd-agent/
git commit -m "feat(agent): context assembly + Rig agent builder"
```

---

## Task 9: mcclawd-tasks — Task Manager (Phase 0: Single Interactive)

**Files:**
- Create: `crates/mcclawd-tasks/src/manager.rs`
- Modify: `crates/mcclawd-tasks/src/lib.rs`

**Step 1: Write manager.rs**

```rust
use mcclawd_core::types::TaskId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct TaskRecord {
    pub id: TaskId,
    pub prompt: String,
    pub status: TaskStatus,
}

/// Phase 0: single interactive task at a time.
/// Phase 2: concurrent tasks with interactive + background modes.
pub struct TaskManager {
    current: Option<TaskRecord>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self { current: None }
    }

    pub fn start_task(&mut self, prompt: String) -> TaskId {
        let id = TaskId::new();
        self.current = Some(TaskRecord {
            id: id.clone(),
            prompt,
            status: TaskStatus::Running,
        });
        id
    }

    pub fn complete_task(&mut self, id: &TaskId) {
        if let Some(ref mut task) = self.current {
            if task.id == *id {
                task.status = TaskStatus::Completed;
            }
        }
    }

    pub fn fail_task(&mut self, id: &TaskId, error: String) {
        if let Some(ref mut task) = self.current {
            if task.id == *id {
                task.status = TaskStatus::Failed(error);
            }
        }
    }

    pub fn current_task(&self) -> Option<&TaskRecord> {
        self.current.as_ref()
    }
}
```

**Step 2: Update lib.rs**

```rust
pub mod manager;
pub use manager::TaskManager;
```

**Step 3: Run tests**

Run: `cargo build -p mcclawd-tasks`
Expected: Compiles cleanly.

**Step 4: Commit**

```bash
git add crates/mcclawd-tasks/
git commit -m "feat(tasks): TaskManager for Phase 0 (single interactive task)"
```

---

## Task 10: mcclawd-api — CLI Binary (`mc run` + `mc secrets`)

**Files:**
- Modify: `crates/mcclawd-api/src/main.rs`
- Create: `crates/mcclawd-api/src/commands/mod.rs`
- Create: `crates/mcclawd-api/src/commands/run.rs`
- Create: `crates/mcclawd-api/src/commands/secrets.rs`
- Create: `crates/mcclawd-api/src/commands/workspace.rs`

**Step 1: Write main.rs with clap CLI**

```rust
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(name = "mc", version, about = "McClawd Agent Platform")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a single agent task
    Run {
        /// The prompt/task to execute
        prompt: String,

        /// Workspace to use
        #[arg(short, long, default_value = "default")]
        workspace: String,
    },

    /// Manage encrypted secrets
    Secrets {
        #[command(subcommand)]
        action: SecretsAction,
    },

    /// Manage agent workspaces
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
}

#[derive(Subcommand)]
enum SecretsAction {
    /// Set a secret value
    Set {
        /// Secret key name
        key: String,
    },
    /// Get a secret value (masked)
    Get {
        /// Secret key name
        key: String,
    },
    /// List all secret keys
    List,
    /// Delete a secret
    Delete {
        /// Secret key name
        key: String,
    },
}

#[derive(Subcommand)]
enum WorkspaceAction {
    /// Initialize a new workspace with template files
    Init {
        /// Workspace name
        #[arg(default_value = "default")]
        name: String,
    },
    /// List all workspaces
    List,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("mcclawd=info".parse()?)
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run { prompt, workspace } => {
            commands::run::execute(&prompt, &workspace).await?;
        }
        Commands::Secrets { action } => match action {
            SecretsAction::Set { key } => commands::secrets::set(&key).await?,
            SecretsAction::Get { key } => commands::secrets::get(&key).await?,
            SecretsAction::List => commands::secrets::list().await?,
            SecretsAction::Delete { key } => commands::secrets::delete(&key).await?,
        },
        Commands::Workspace { action } => match action {
            WorkspaceAction::Init { name } => commands::workspace::init(&name).await?,
            WorkspaceAction::List => commands::workspace::list().await?,
        },
    }

    Ok(())
}
```

**Step 2: Write commands/mod.rs**

```rust
pub mod run;
pub mod secrets;
pub mod workspace;
```

**Step 3: Write commands/secrets.rs**

```rust
use mcclawd_core::config::McclawdConfig;
use mcclawd_core::secrets::{EncryptedFileBackend, SecretBackend};
use std::io::{self, Write};

fn get_backend() -> anyhow::Result<EncryptedFileBackend> {
    let config = McclawdConfig::default();
    // Phase 0: use a fixed passphrase derived from machine identity.
    // Phase 1+: prompt for passphrase or use OS keychain.
    let passphrase = "mcclawd-local-dev"; // TODO: derive from machine ID
    Ok(EncryptedFileBackend::new(&config.secrets_path(), passphrase)?)
}

pub async fn set(key: &str) -> anyhow::Result<()> {
    eprint!("Enter value for {}: ", key);
    io::stderr().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim();

    let backend = get_backend()?;
    backend.set(key, value).await?;
    println!("Secret '{}' saved.", key);
    Ok(())
}

pub async fn get(key: &str) -> anyhow::Result<()> {
    let backend = get_backend()?;
    match backend.get(key).await? {
        Some(value) => {
            // Show masked value
            let masked = if value.len() > 8 {
                format!("{}...{}", &value[..4], &value[value.len()-4..])
            } else {
                "****".to_string()
            };
            println!("{}: {}", key, masked);
        }
        None => println!("Secret '{}' not found.", key),
    }
    Ok(())
}

pub async fn list() -> anyhow::Result<()> {
    let backend = get_backend()?;
    let keys = backend.list().await?;
    if keys.is_empty() {
        println!("No secrets stored.");
    } else {
        for key in keys {
            println!("  {}", key);
        }
    }
    Ok(())
}

pub async fn delete(key: &str) -> anyhow::Result<()> {
    let backend = get_backend()?;
    backend.delete(key).await?;
    println!("Secret '{}' deleted.", key);
    Ok(())
}
```

**Step 4: Write commands/workspace.rs**

```rust
use mcclawd_core::config::McclawdConfig;
use mcclawd_agent::workspace::WorkspaceLoader;

pub async fn init(name: &str) -> anyhow::Result<()> {
    let config = McclawdConfig::default();
    let loader = WorkspaceLoader::new(config.workspaces_dir());
    let path = loader.scaffold(name)?;
    println!("Workspace '{}' created at {}", name, path.display());
    Ok(())
}

pub async fn list() -> anyhow::Result<()> {
    let config = McclawdConfig::default();
    let ws_dir = config.workspaces_dir();
    if !ws_dir.exists() {
        println!("No workspaces found. Run `mc workspace init` to create one.");
        return Ok(());
    }
    for entry in std::fs::read_dir(ws_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            println!("  {}", entry.file_name().to_string_lossy());
        }
    }
    Ok(())
}
```

**Step 5: Write commands/run.rs**

```rust
use mcclawd_agent::engine::AgentBuilder;
use mcclawd_agent::workspace::WorkspaceLoader;
use mcclawd_core::config::McclawdConfig;
use mcclawd_core::secrets::{EncryptedFileBackend, SecretBackend};
use rig::completion::Prompt;
use futures::StreamExt;

pub async fn execute(prompt: &str, workspace_name: &str) -> anyhow::Result<()> {
    let config = McclawdConfig::default();

    // 1. Load workspace
    let loader = WorkspaceLoader::new(config.workspaces_dir());
    let workspace = loader.load(workspace_name)?;

    // 2. Get API key from secrets
    let passphrase = "mcclawd-local-dev";
    let secrets = EncryptedFileBackend::new(&config.secrets_path(), passphrase)?;
    let api_key = secrets
        .get("ANTHROPIC_API_KEY")
        .await?
        .ok_or_else(|| anyhow::anyhow!(
            "ANTHROPIC_API_KEY not found. Run: mc secrets set ANTHROPIC_API_KEY"
        ))?;

    // 3. Build agent
    let max_turns = config.agent.max_turns;
    let (agent, _memory) = AgentBuilder::build(workspace, &api_key, max_turns)?;

    // 4. Run with streaming
    eprintln!("McClawd v0.1.0 — thinking...\n");

    // Use Rig's streaming agent
    use rig::agent::prompt_request::streaming::MultiTurnStreamItem;
    use rig::streaming::StreamedAssistantContent;

    let mut stream = agent.stream_prompt(prompt).await?;

    while let Some(chunk) = stream.next().await {
        match chunk? {
            MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::Text(text)
            ) => {
                print!("{}", text.text);
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
            MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ToolCall { tool_call, .. }
            ) => {
                eprintln!("\n[tool: {} args: {}]", tool_call.function.name, tool_call.function.arguments);
            }
            MultiTurnStreamItem::FinalResponse(resp) => {
                println!("\n");
            }
            _ => {}
        }
    }

    Ok(())
}
```

**Step 6: Build and verify**

Run: `cargo build -p mcclawd-api`
Expected: Compiles with binary `mc`.

**Step 7: Commit**

```bash
git add crates/mcclawd-api/
git commit -m "feat(api): mc CLI with run, secrets, workspace commands"
```

---

## Task 11: End-to-End Smoke Test

**Files:**
- Create: `workspaces/default/SOUL.md`
- Create: `workspaces/default/AGENTS.md`
- Create: `workspaces/default/USER.md`

**Step 1: Create default workspace template files**

`workspaces/default/SOUL.md`:
```markdown
# Soul

You are McClawd, a security-focused AI assistant.

## Personality
- Direct and technical. Skip pleasantries when the user is in flow.
- When uncertain, say so. Never fabricate tool output.
- Prefer showing code over describing code.

## Rules
- Never execute destructive operations (rm -rf, DROP TABLE) without explicit confirmation.
- Always explain security implications of suggested changes.
- Refuse to store secrets in plaintext, even if asked.

## Identity
- Name: McClawd
- Version: 0.1.0
```

`workspaces/default/AGENTS.md`:
```markdown
# Agents

## Default Skills
- memory-management

## Available Agents

### default
- **Specialty:** General purpose assistant
- **Model:** claude-sonnet-4-5
- **Tools:** memory_store, memory_recall
```

`workspaces/default/USER.md`:
```markdown
# User

## Preferences
- Concise responses preferred
- Show code examples when relevant
```

**Step 2: Set up secrets and run**

```bash
# Build
cargo build --release -p mcclawd-api

# Initialize workspace (uses ~/.mcclawd/workspaces/default/)
./target/release/mc workspace init

# Set API key
./target/release/mc secrets set ANTHROPIC_API_KEY
# Enter your Anthropic API key when prompted

# Run a prompt
./target/release/mc run "What is 2 + 2? Store the answer in memory with key 'result'."
```

Expected: Agent responds with "4", calls memory_store tool, streams response to stdout.

**Step 3: Verify secrets are encrypted**

```bash
# Verify secrets file is binary (encrypted), not plaintext
file ~/.mcclawd/secrets.enc
# Expected: "data" (binary), NOT "ASCII text"

# Verify listing works
./target/release/mc secrets list
# Expected: ANTHROPIC_API_KEY
```

**Step 4: Commit workspace templates**

```bash
git add workspaces/
git commit -m "feat: default workspace template (SOUL.md, AGENTS.md, USER.md)"
```

---

## Task 12: CLAUDE.md + .gitignore + Final Cleanup

**Files:**
- Create: `CLAUDE.md`
- Modify: `.gitignore`

**Step 1: Write CLAUDE.md for the project**

```markdown
# McClawd v5

## Architecture
See `mcclawd-v5-architecture.md` for the full design doc.

## Build
```
cargo build --release -p mcclawd-api
```

## Test
```
cargo test --workspace
```

## Run
```
# Initialize workspace
./target/release/mc workspace init

# Set API key
./target/release/mc secrets set ANTHROPIC_API_KEY

# Run a task
./target/release/mc run "your prompt here"
```

## Crate Structure
- `mcclawd-core` — types, config, secrets, identity, hooks
- `mcclawd-agent` — workspace loader, context assembly, Rig agent builder
- `mcclawd-tools` — builtin tools (memory), MCP client
- `mcclawd-channels` — Channel trait, pipeline, CLI adapter
- `mcclawd-tasks` — task lifecycle
- `mcclawd-api` — `mc` binary (CLI entrypoint)

## Key Decisions
- Rig handles the agent loop natively (no manual ReAct loop)
- All workspace files loaded in Phase 0 (SOUL.md, AGENTS.md, USER.md)
- Secrets encrypted at rest (AES-256-GCM-SIV + argon2)
- No sandbox yet (Phase 1) — tools run in-process
- No skills yet (Phase 1) — MCP tools only
- CLI is the only channel (Phase 1 adds web)
```

**Step 2: Update .gitignore**

```
/target
*.swp
*.swo
.DS_Store
secrets.enc
.env
```

**Step 3: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass.

**Step 4: Final commit**

```bash
git add CLAUDE.md .gitignore
git commit -m "docs: CLAUDE.md + gitignore for Phase 0"
```

---

## Summary

| Task | What | Crate | Tests |
|------|------|-------|-------|
| 1 | Workspace scaffold | all | cargo build |
| 2 | Types, config, error, hooks | mcclawd-core | 3 |
| 3 | Encrypted secrets backend | mcclawd-core | 5 |
| 4 | JWT identity | mcclawd-core | 2 |
| 5 | Workspace loader + AGENTS.md parser | mcclawd-agent | 4+ |
| 6 | Memory tools + MCP stub | mcclawd-tools | 2 |
| 7 | Channel trait + CLI adapter | mcclawd-channels | 1 |
| 8 | Context assembly + Rig agent builder | mcclawd-agent | 2 |
| 9 | Task manager | mcclawd-tasks | 0 (build) |
| 10 | CLI binary (mc run/secrets/workspace) | mcclawd-api | 0 (build) |
| 11 | E2E smoke test | - | manual |
| 12 | CLAUDE.md + cleanup | - | full suite |

**Phase 0 Demo:** `mc run "Use memory to store that my name is Alice, then recall it"` → streams response with tool calls visible.
