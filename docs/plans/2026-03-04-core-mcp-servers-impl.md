# Core MCP Servers Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add three core MCP tool servers (langextract, scrapling, filesystem) as pre-built Docker containers, declared in workspace config, auto-started on `mc run`, routed through AgentGateway.

**Architecture:** Each MCP server is a pre-built Docker image with `supergateway` wrapping stdio→HTTP. A workspace `mcp.toml` config declares which MCP images to use. On `mc run`, McClawd ensures declared MCP containers and AgentGateway are running, then connects via rmcp StreamableHttp. These 3 MCPs are "common/core" — pre-built and downloadable. Future MCPs from ClawHub follow the same pattern.

**Tech Stack:** Docker (bollard crate for container management), supergateway (stdio→HTTP), AgentGateway (MCP routing), rmcp 1.x, rig-core 0.31

---

## Architecture

```
mc run "parse this PDF"
  │
  │ 1. Read workspace mcp.toml
  │ 2. Ensure MCP containers running (pull if needed)
  │ 3. Ensure AgentGateway running
  │ 4. Connect via rmcp StreamableHttp
  ▼
AgentGateway (:3000)  [official image, unmodified]
  │
  ├── mcp: http://mcp-langextract:8000   → pre-built image
  ├── mcp: http://mcp-scrapling:8000     → pre-built image
  ├── mcp: http://mcp-filesystem:8000    → pre-built image
  │
  └── (future: ClawHub MCPs, remote MCPs)
```

**Config-driven:** MCP servers declared in `~/.mcclawd/workspaces/<name>/mcp.toml`:

```toml
# Core MCPs — pre-built images, downloaded on first use
[[servers]]
name = "langextract"
image = "ghcr.io/macleodlabs/mcp-langextract:latest"
port = 8001
env = ["GOOGLE_API_KEY"]

[[servers]]
name = "scrapling"
image = "ghcr.io/macleodlabs/mcp-scrapling:latest"
port = 8002

[[servers]]
name = "filesystem"
image = "ghcr.io/macleodlabs/mcp-filesystem:latest"
port = 8003
volumes = ["/data:/data"]
```

**Phase 0 (this plan):** Dockerfiles built locally, config hardcoded with defaults.
**Phase 1+:** Images published to ghcr.io, `mc mcp add/remove` CLI, ClawHub registry integration.

---

### Task 1: Create mcp-langextract Dockerfile

**Files:**
- Create: `docker/mcp-langextract/Dockerfile`

**Step 1: Create the directory**

```bash
mkdir -p docker/mcp-langextract
```

**Step 2: Write the Dockerfile**

Create `docker/mcp-langextract/Dockerfile`:

```dockerfile
# MCP server: langextract — extract structured data from PDFs, docs, URLs
# Wrapped with supergateway to expose HTTP endpoint.
FROM python:3.12-slim

# Install Node.js 22 (for supergateway)
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates gnupg \
    && mkdir -p /etc/apt/keyrings \
    && curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
       | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg \
    && echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_22.x nodistro main" \
       > /etc/apt/sources.list.d/nodesource.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Install supergateway (stdio → HTTP wrapper)
RUN npm install -g supergateway

# Install langextract-mcp
RUN pip install uv && uv pip install --system langextract-mcp

EXPOSE 8000

CMD ["supergateway", "--stdio", "langextract-mcp", "--port", "8000", "--outputTransport", "streamableHttp"]
```

> **Note for executor:** Verify the `langextract-mcp` entry point name:
> ```bash
> docker run --rm python:3.12-slim sh -c "pip install uv && uv pip install --system langextract-mcp && ls /usr/local/bin/ | grep -i lang"
> ```

**Step 3: Verify build**

```bash
docker build -t mcp-langextract docker/mcp-langextract/
```

**Step 4: Commit**

```bash
git add docker/mcp-langextract/Dockerfile
git commit -m "feat: add mcp-langextract Dockerfile with supergateway HTTP proxy"
```

---

### Task 2: Create mcp-scrapling Dockerfile

**Files:**
- Create: `docker/mcp-scrapling/Dockerfile`

**Step 1: Create the directory**

```bash
mkdir -p docker/mcp-scrapling
```

**Step 2: Write the Dockerfile**

Create `docker/mcp-scrapling/Dockerfile`:

```dockerfile
# MCP server: scrapling — web scraping with anti-bot bypass
# Needs Chromium for dynamic content fetching.
FROM python:3.12-slim

# Install Node.js 22 (for supergateway) and Chromium (for scrapling)
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       curl ca-certificates gnupg chromium \
    && mkdir -p /etc/apt/keyrings \
    && curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
       | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg \
    && echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_22.x nodistro main" \
       > /etc/apt/sources.list.d/nodesource.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g supergateway

RUN pip install uv \
    && uv pip install --system "scrapling[ai]" \
    && scrapling install

EXPOSE 8000

CMD ["supergateway", "--stdio", "scrapling-fetch-mcp", "--port", "8000", "--outputTransport", "streamableHttp"]
```

**Step 3: Verify build**

```bash
docker build -t mcp-scrapling docker/mcp-scrapling/
```

**Step 4: Commit**

```bash
git add docker/mcp-scrapling/Dockerfile
git commit -m "feat: add mcp-scrapling Dockerfile with Chromium and supergateway"
```

---

### Task 3: Create mcp-filesystem Dockerfile

**Files:**
- Create: `docker/mcp-filesystem/Dockerfile`

**Step 1: Create the directory**

```bash
mkdir -p docker/mcp-filesystem
```

**Step 2: Write the Dockerfile**

Create `docker/mcp-filesystem/Dockerfile`:

```dockerfile
# MCP server: filesystem — read/write/search files scoped to /data
FROM node:22-alpine

RUN npm install -g supergateway @modelcontextprotocol/server-filesystem

VOLUME /data
EXPOSE 8000

# Scoped to /data — no access outside the mounted volume
CMD ["supergateway", "--stdio", "mcp-server-filesystem /data", "--port", "8000", "--outputTransport", "streamableHttp"]
```

**Step 3: Verify build**

```bash
docker build -t mcp-filesystem docker/mcp-filesystem/
```

**Step 4: Commit**

```bash
git add docker/mcp-filesystem/Dockerfile
git commit -m "feat: add mcp-filesystem Dockerfile with supergateway"
```

---

### Task 4: Create AgentGateway config

**Files:**
- Create: `config/agentgateway.yaml`

**Step 1: Create the directory**

```bash
mkdir -p config
```

**Step 2: Write the config**

Create `config/agentgateway.yaml`:

```yaml
# AgentGateway — multiplexes MCP tool calls to containerized MCP servers.
# Each MCP server runs in its own Docker container with supergateway HTTP endpoint.
#
# Phase 0: This file is static. Phase 1+: generated dynamically from mcp.toml.
binds:
  - port: 3000
    listeners:
      - routes:
          - backends:
              - mcp:
                  targets:
                    - name: langextract
                      mcp:
                        host: http://mcp-langextract:8000

                    - name: scrapling
                      mcp:
                        host: http://mcp-scrapling:8000

                    - name: filesystem
                      mcp:
                        host: http://mcp-filesystem:8000
```

**Step 3: Commit**

```bash
git add config/agentgateway.yaml
git commit -m "feat: add AgentGateway config with 3 remote MCP targets"
```

---

### Task 5: Create docker-compose.yml for local development

**Files:**
- Create: `docker-compose.yml`

> This docker-compose.yml is for **local development only**. In production (Phase 1+),
> `mc run` will manage containers directly via bollard. But for Phase 0, docker-compose
> lets us bring up the full stack manually.

**Step 1: Write docker-compose.yml**

Create `docker-compose.yml`:

```yaml
# Development stack — MCP servers + AgentGateway
# Production: mc manages containers directly via bollard (Phase 1+)
services:
  agentgateway:
    image: ghcr.io/modelcontextprotocol/agentgateway:latest
    ports:
      - "3000:3000"
    volumes:
      - ./config/agentgateway.yaml:/config/agentgateway.yaml:ro
    depends_on:
      mcp-langextract:
        condition: service_started
      mcp-scrapling:
        condition: service_started
      mcp-filesystem:
        condition: service_started
    restart: unless-stopped
    healthcheck:
      test: ["CMD-SHELL", "curl -sf http://localhost:3000/ || exit 1"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 15s

  mcp-langextract:
    build:
      context: ./docker/mcp-langextract
    environment:
      - GOOGLE_API_KEY=${GOOGLE_API_KEY}
    restart: unless-stopped
    healthcheck:
      test: ["CMD-SHELL", "curl -sf http://localhost:8000/ || exit 1"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 10s

  mcp-scrapling:
    build:
      context: ./docker/mcp-scrapling
    restart: unless-stopped
    healthcheck:
      test: ["CMD-SHELL", "curl -sf http://localhost:8000/ || exit 1"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 10s

  mcp-filesystem:
    build:
      context: ./docker/mcp-filesystem
    volumes:
      - mcclawd-data:/data
    restart: unless-stopped
    healthcheck:
      test: ["CMD-SHELL", "curl -sf http://localhost:8000/ || exit 1"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 10s

volumes:
  mcclawd-data:
```

**Step 2: Commit**

```bash
git add docker-compose.yml
git commit -m "feat: add docker-compose for local MCP development stack"
```

---

### Task 6: Update .env with GOOGLE_API_KEY

**Files:**
- Modify: `.env`

**Step 1: Append GOOGLE_API_KEY**

```
GOOGLE_API_KEY=your-google-api-key-here
```

**Step 2: Verify .env is gitignored**

Run: `grep '.env' .gitignore`
Expected: `.env` is listed

**Step 3: No commit** — `.env` is gitignored.

---

### Task 7: Build and verify Docker stack

**Step 1: Build all images**

```bash
docker compose build --no-cache
```

Expected: All 3 MCP images build. AgentGateway uses pre-built image.

**Step 2: If build fails — debug one at a time**

```bash
docker build -t test-langextract docker/mcp-langextract/
docker build -t test-scrapling docker/mcp-scrapling/
docker build -t test-filesystem docker/mcp-filesystem/
```

Common fixes:
- Wrong entry point name → check with `pip show` / `npm list -g`
- Chromium install fails → try `chromium-browser` or different base
- supergateway not found → verify `npm install -g supergateway` succeeded

**Step 3: Start the stack**

```bash
docker compose up -d
```

**Step 4: Verify all containers**

```bash
docker compose ps
docker compose logs mcp-filesystem
docker compose logs mcp-langextract
docker compose logs mcp-scrapling
docker compose logs agentgateway
```

Expected: All services "Up", supergateway listening on 8000 in each MCP container, AgentGateway connected to targets.

**Step 5: Quick connectivity test**

```bash
curl -s http://localhost:3000/ | head -5
```

**Step 6: Commit any fixes**

```bash
git add docker/ config/ docker-compose.yml
git commit -m "fix: correct Dockerfile paths after build verification"
```

---

### Task 8: Add MCP config types and agentgateway_url to McclawdConfig

**Files:**
- Modify: `crates/mcclawd-core/src/config.rs`
- Modify: `crates/mcclawd-core/tests/config_test.rs`

**Step 1: Write the failing tests**

Add to `crates/mcclawd-core/tests/config_test.rs`:

```rust
use mcclawd_core::config::{McclawdConfig, McpConfig, McpServerConfig};

#[test]
fn config_has_agentgateway_url_default() {
    let config = McclawdConfig::default();
    assert_eq!(config.mcp.agentgateway_url, "http://localhost:3000");
}

#[test]
fn config_parses_agentgateway_url() {
    let toml_str = r#"
[mcp]
agentgateway_url = "http://custom-host:9090"
"#;
    let config: McclawdConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.mcp.agentgateway_url, "http://custom-host:9090");
}

#[test]
fn config_has_default_mcp_servers() {
    let config = McclawdConfig::default();
    assert_eq!(config.mcp.servers.len(), 3);
    assert!(config.mcp.servers.iter().any(|s| s.name == "langextract"));
    assert!(config.mcp.servers.iter().any(|s| s.name == "scrapling"));
    assert!(config.mcp.servers.iter().any(|s| s.name == "filesystem"));
}

#[test]
fn mcp_server_config_has_image_and_port() {
    let config = McclawdConfig::default();
    let fs = config.mcp.servers.iter().find(|s| s.name == "filesystem").unwrap();
    assert!(fs.image.contains("mcp-filesystem"));
    assert_eq!(fs.port, 8003);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p mcclawd-core -- mcp`
Expected: FAIL — types don't exist yet

**Step 3: Implement McpConfig and McpServerConfig**

Add to `crates/mcclawd-core/src/config.rs`:

```rust
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
```

Add `mcp` field to `McclawdConfig` struct and its `Default` impl:

```rust
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
```

**Step 4: Run tests**

Run: `cargo test -p mcclawd-core -- mcp`
Expected: All PASS

**Step 5: Run all core tests**

Run: `cargo test -p mcclawd-core`
Expected: All pass

**Step 6: Commit**

```bash
git add crates/mcclawd-core/src/config.rs crates/mcclawd-core/tests/config_test.rs
git commit -m "feat: add McpConfig with server declarations and agentgateway_url

Three core MCPs (langextract, scrapling, filesystem) declared as defaults.
Config-driven: servers specified by Docker image, port, env, volumes."
```

---

### Task 9: Enable rmcp feature on rig-core

**Files:**
- Modify: `Cargo.toml` (workspace root, line 19)

**Step 1: Update rig-core dependency**

Change:
```toml
rig-core = "0.31"
```
To:
```toml
rig-core = { version = "0.31", features = ["rmcp"] }
```

**Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: Compiles

> **Note:** If feature name differs, check: `cargo metadata --format-version=1 | jq '.packages[] | select(.name == "rig-core") | .features'`

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: enable rmcp feature on rig-core for MCP tool bridging"
```

---

### Task 10: Implement McpClient in mcclawd-tools

**Files:**
- Create: `crates/mcclawd-tools/src/mcp.rs`
- Modify: `crates/mcclawd-tools/src/lib.rs`
- Create: `crates/mcclawd-tools/tests/mcp_test.rs`

**Step 1: Write the failing tests**

Create `crates/mcclawd-tools/tests/mcp_test.rs`:

```rust
use mcclawd_tools::mcp::McpClient;

#[tokio::test]
async fn mcp_client_creates_from_url() {
    let client = McpClient::new("http://localhost:3000");
    assert_eq!(client.url(), "http://localhost:3000");
}

#[tokio::test]
async fn mcp_client_connect_fails_when_no_server() {
    let client = McpClient::new("http://localhost:19999");
    let result = client.connect().await;
    assert!(result.is_err(), "Should fail when no server is running");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p mcclawd-tools -- mcp`
Expected: FAIL — module `mcp` not found

**Step 3: Implement McpClient**

Create `crates/mcclawd-tools/src/mcp.rs`:

```rust
//! MCP client — connects to AgentGateway via StreamableHttp, discovers and calls tools.
//!
//! Usage:
//! ```rust,ignore
//! let client = McpClient::new("http://localhost:3000");
//! let conn = client.connect().await?;
//! let (tools, sink) = conn.into_rig_tools();
//! agent_builder.rmcp_tools(tools, sink);
//! ```

use anyhow::{Context, Result};
use rmcp::{
    model::{CallToolRequestParam, Tool as McpToolDef},
    service::ServiceExt,
};

/// Client that connects to AgentGateway and discovers MCP tools.
pub struct McpClient {
    url: String,
}

/// Active MCP connection with discovered tools.
pub struct McpConnection {
    service: rmcp::service::RunningService<rmcp::service::RoleClient, ()>,
    tools: Vec<McpToolDef>,
}

impl McpClient {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Connect to AgentGateway and discover available MCP tools.
    pub async fn connect(&self) -> Result<McpConnection> {
        let transport = rmcp::transport::StreamableHttpClientTransport::builder(
            self.url.parse().context("invalid AgentGateway URL")?,
        )
        .build();

        let service = ()
            .serve(transport)
            .await
            .context("failed to connect to AgentGateway")?;

        let tool_list = service
            .list_tools(Default::default())
            .await
            .context("failed to list MCP tools")?;

        let tools = tool_list.tools;
        tracing::info!("Discovered {} MCP tools from AgentGateway", tools.len());
        for tool in &tools {
            tracing::debug!("  tool: {} — {}", tool.name, tool.description.as_deref().unwrap_or(""));
        }

        Ok(McpConnection { service, tools })
    }
}

impl McpConnection {
    pub fn tools(&self) -> &[McpToolDef] {
        &self.tools
    }

    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Consume connection, returning (tools, sink) for Rig's `.rmcp_tools()`.
    pub fn into_rig_tools(self) -> (Vec<McpToolDef>, rmcp::service::ServerSink) {
        let sink = self.service.peer().clone();
        (self.tools, sink)
    }

    /// Call a tool by name with JSON arguments.
    pub async fn call_tool(&self, name: &str, args: serde_json::Value) -> Result<String> {
        let result = self
            .service
            .call_tool(CallToolRequestParam {
                name: name.into(),
                arguments: args.as_object().cloned(),
            })
            .await
            .context("MCP tool call failed")?;

        let text: String = result
            .content
            .iter()
            .filter_map(|c| {
                if let rmcp::model::Content::Text(t) = c {
                    Some(t.text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(text)
    }

    pub async fn shutdown(self) -> Result<()> {
        self.service.cancel().await?;
        Ok(())
    }
}
```

> **Note for executor:** Verify rmcp API by running `cargo doc -p rmcp --no-deps --open`.
> Key things to check: `StreamableHttpClientTransport::builder()`, `service.peer()` → `ServerSink`,
> `rmcp::model::Content::Text` variant name.

**Step 4: Update lib.rs**

Replace `crates/mcclawd-tools/src/lib.rs`:

```rust
//! McClawd tools — builtin tools, MCP client

pub mod mcp;
```

**Step 5: Run tests**

Run: `cargo test -p mcclawd-tools -- mcp`
Expected: Both tests PASS

**Step 6: Commit**

```bash
git add crates/mcclawd-tools/src/mcp.rs crates/mcclawd-tools/src/lib.rs crates/mcclawd-tools/tests/mcp_test.rs
git commit -m "feat: implement McpClient for AgentGateway connection

Connects via StreamableHttp, discovers tools, bridges to Rig via into_rig_tools()."
```

---

### Task 11: Wire MCP tools into agent builder

**Files:**
- Create: `crates/mcclawd-agent/src/mcp_integration.rs`
- Modify: `crates/mcclawd-agent/src/lib.rs`
- Modify: `crates/mcclawd-agent/Cargo.toml`

**Step 1: Add mcclawd-tools dependency**

Add to `crates/mcclawd-agent/Cargo.toml` under `[dependencies]`:

```toml
mcclawd-tools = { workspace = true }
```

**Step 2: Create mcp_integration module**

Create `crates/mcclawd-agent/src/mcp_integration.rs`:

```rust
//! MCP tool integration — connects to AgentGateway and provides tools for the Rig agent.

use anyhow::Result;
use mcclawd_core::config::McclawdConfig;
use mcclawd_tools::mcp::McpClient;

/// Connect to AgentGateway and return MCP tools for the Rig agent.
/// Returns None if AgentGateway is unavailable (agent runs without MCP tools).
pub async fn connect_mcp_tools(
    config: &McclawdConfig,
) -> Result<Option<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)>> {
    let client = McpClient::new(&config.mcp.agentgateway_url);
    match client.connect().await {
        Ok(conn) => {
            tracing::info!(
                "Connected to AgentGateway at {}, {} MCP tools available",
                config.mcp.agentgateway_url,
                conn.tool_count()
            );
            Ok(Some(conn.into_rig_tools()))
        }
        Err(e) => {
            tracing::warn!(
                "AgentGateway not available at {}, running without MCP tools: {e}",
                config.mcp.agentgateway_url
            );
            Ok(None)
        }
    }
}
```

**Step 3: Export the module**

Add to `crates/mcclawd-agent/src/lib.rs`:

```rust
//! McClawd agent — workspace loader, context assembly, Rig agent builder

pub mod mcp_integration;
```

**Step 4: If Rig agent builder exists (Phase 0 Task 8), wire it in**

```rust
// In the agent building function:
if let Some((tools, sink)) = mcp_integration::connect_mcp_tools(&config).await? {
    agent_builder = agent_builder.rmcp_tools(tools, sink);
}
```

**Step 5: Verify it compiles**

Run: `cargo check -p mcclawd-agent`
Expected: Compiles

**Step 6: Commit**

```bash
git add crates/mcclawd-agent/
git commit -m "feat: wire MCP tools into agent builder via AgentGateway

Graceful fallback if AgentGateway is unavailable."
```

---

### Task 12: E2E smoke test

**Files:**
- Create: `crates/mcclawd-tools/tests/mcp_integration.rs`

**Step 1: Write the integration test**

Create `crates/mcclawd-tools/tests/mcp_integration.rs`:

```rust
//! Integration test: connects to AgentGateway and discovers MCP tools.
//!
//! Requires: `docker compose up -d` on localhost:3000.
//! Run: `cargo test -p mcclawd-tools --test mcp_integration -- --ignored --nocapture`

use mcclawd_tools::mcp::McpClient;

#[tokio::test]
#[ignore]
async fn discovers_mcp_tools_from_agentgateway() {
    let client = McpClient::new("http://localhost:3000");
    let conn = client
        .connect()
        .await
        .expect("AgentGateway should be running (docker compose up -d)");

    let tools = conn.tools();
    assert!(!tools.is_empty(), "Should discover MCP tools");

    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    println!("Discovered {} MCP tools: {:?}", tools.len(), names);

    assert!(
        names.iter().any(|n| n.contains("read") || n.contains("list") || n.contains("directory")),
        "Should find filesystem tools, got: {:?}",
        names
    );

    conn.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn filesystem_tool_lists_data_directory() {
    let client = McpClient::new("http://localhost:3000");
    let conn = client.connect().await.expect("AgentGateway should be running");

    let result = conn
        .call_tool("list_directory", serde_json::json!({ "path": "/data" }))
        .await;

    println!("list_directory result: {:?}", result);
    assert!(result.is_ok(), "list_directory should succeed: {:?}", result);

    conn.shutdown().await.unwrap();
}
```

**Step 2: Verify test compiles**

Run: `cargo test -p mcclawd-tools --test mcp_integration --no-run`

**Step 3: Run with Docker stack**

```bash
docker compose up -d && sleep 15
cargo test -p mcclawd-tools --test mcp_integration -- --ignored --nocapture
```

**Step 4: Commit**

```bash
git add crates/mcclawd-tools/tests/mcp_integration.rs
git commit -m "test: add MCP integration smoke tests

Run: cargo test -p mcclawd-tools --test mcp_integration -- --ignored"
```

---

### Task 13: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

**Step 1: Add Docker and MCP sections**

Add after "Run" section:

```markdown
## Docker (MCP Infrastructure)

```bash
docker compose build --no-cache          # build MCP server images
docker compose up -d                     # start AgentGateway + 3 MCP containers
docker compose logs -f agentgateway      # watch MCP tool discovery
docker compose down                      # stop everything
cargo test -p mcclawd-tools --test mcp_integration -- --ignored  # E2E test
```

Three core MCP servers run as separate Docker containers with supergateway (stdio→HTTP). AgentGateway (unmodified official image) routes tool calls to each container. The `mc` binary connects to AgentGateway at `http://localhost:3000`.

MCP servers are config-driven (`McpConfig` in `mcclawd-core`). Phase 1+ adds `mc mcp add/remove` CLI and ClawHub registry.
```

**Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add Docker MCP infrastructure section to CLAUDE.md"
```

---

## Summary

| File | Action | Purpose |
|------|--------|---------|
| `docker/mcp-langextract/Dockerfile` | Create | Python + supergateway + langextract-mcp |
| `docker/mcp-scrapling/Dockerfile` | Create | Python + supergateway + scrapling + Chromium |
| `docker/mcp-filesystem/Dockerfile` | Create | Node.js + supergateway + filesystem MCP |
| `config/agentgateway.yaml` | Create | AgentGateway routing to 3 remote HTTP targets |
| `docker-compose.yml` | Create | Dev stack: gateway + 3 MCP containers |
| `.env` | Modify | Add GOOGLE_API_KEY |
| `crates/mcclawd-core/src/config.rs` | Modify | McpConfig + McpServerConfig types |
| `crates/mcclawd-core/tests/config_test.rs` | Modify | MCP config tests |
| `Cargo.toml` (root) | Modify | Enable rmcp feature on rig-core |
| `crates/mcclawd-tools/src/mcp.rs` | Create | McpClient + McpConnection |
| `crates/mcclawd-tools/src/lib.rs` | Modify | Export mcp module |
| `crates/mcclawd-tools/tests/mcp_test.rs` | Create | Unit tests |
| `crates/mcclawd-tools/tests/mcp_integration.rs` | Create | E2E smoke tests |
| `crates/mcclawd-agent/src/mcp_integration.rs` | Create | MCP wiring into Rig agent |
| `crates/mcclawd-agent/Cargo.toml` | Modify | Add mcclawd-tools dep |
| `CLAUDE.md` | Modify | Docker section |

## Future Work (Phase 1+)

- `mc mcp add <name>` CLI — pulls image from ClawHub registry
- `mc mcp remove <name>` — stops container, removes from config
- `mc mcp list` — shows configured/running MCPs
- Dynamic AgentGateway config generation from mcp.toml
- Bollard-based container management (replace docker-compose)
- Published pre-built images to ghcr.io/macleodlabs/mcp-*
- ClawHub MCP server marketplace
