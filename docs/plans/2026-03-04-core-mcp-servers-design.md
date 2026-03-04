# Core MCP Servers Design

**Date:** 2026-03-04
**Status:** Approved

## Goal

Add three core MCP tool servers to McClawd's Docker Compose stack, routed through AgentGateway, giving agents the ability to parse documents, scrape the web, and work with the filesystem.

## MCP Servers

### 1. langextract-mcp (Document Parsing)

- **Source:** [langextract-mcp](https://github.com/larsenweigle/langextract-mcp)
- **Purpose:** Extract structured information from PDFs, text documents, and URLs using LLMs
- **Runtime:** Python, installed via `pip install langextract-mcp`
- **Transport:** stdio (AgentGateway manages the process)
- **Container image:** Python 3.12 slim, `uv pip install langextract-mcp`
- **Volumes:** `/data` (read-only, for local document access)
- **Secrets:** `GOOGLE_API_KEY` (for Gemini models used by langextract) delivered via env from AgentGateway

### 2. scrapling-fetch-mcp (Web Scraping)

- **Source:** [scrapling-fetch-mcp](https://pypi.org/project/scrapling-fetch-mcp/)
- **Purpose:** Fetch and parse web pages with anti-bot bypass (Cloudflare Turnstile, etc.)
- **Runtime:** Python, installed via `pip install "scrapling[ai]"` + `scrapling install`
- **Transport:** stdio
- **Container image:** Python 3.12 + Chromium (for dynamic content fetching)
- **Volumes:** None (no host access needed)
- **Tools exposed:** `get`, `bulk_get`, `fetch`, `bulk_fetch`

### 3. mcp-filesystem (File System)

- **Source:** [@modelcontextprotocol/server-filesystem](https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem)
- **Purpose:** Read/write/search files within a scoped directory
- **Runtime:** Node.js, `npx @modelcontextprotocol/server-filesystem /data`
- **Transport:** stdio
- **Container image:** Node 22 alpine
- **Volumes:** `/data` (read-write, the agent's working directory)
- **Security:** Scoped to `/data` only — no access outside mounted volume

## Architecture

```
McClawd (mc binary)
  │
  │ rmcp SSE client
  ▼
AgentGateway (:3030)
  │
  ├── stdio → mcp-langextract container
  ├── stdio → mcp-scrapling container
  ├── stdio → mcp-filesystem container
  │
  └── SSE/HTTP → remote MCPs (GitHub, Notion, etc.)
```

- **McClawd connects to AgentGateway only** — single SSE endpoint
- **AgentGateway routes tool calls** to the correct MCP server by tool name
- **Local MCPs** run as Docker Compose services with stdio transport
- **Remote MCPs** registered as SSE targets in AgentGateway config

## Docker Compose Services

```yaml
  agentgateway:
    image: ghcr.io/modelcontextprotocol/agentgateway:latest
    ports:
      - "3030:3030"
    volumes:
      - ./config/agentgateway.yaml:/config/agentgateway.yaml:ro
    depends_on:
      - mcp-langextract
      - mcp-scrapling
      - mcp-filesystem
    restart: unless-stopped

  mcp-langextract:
    build:
      context: ./docker/mcp-langextract
    volumes:
      - mcclawd-data:/data:ro
    environment:
      - GOOGLE_API_KEY=${GOOGLE_API_KEY}
    restart: unless-stopped

  mcp-scrapling:
    build:
      context: ./docker/mcp-scrapling
    restart: unless-stopped

  mcp-filesystem:
    build:
      context: ./docker/mcp-filesystem
    volumes:
      - mcclawd-data:/data
    restart: unless-stopped

volumes:
  mcclawd-data:
```

## AgentGateway Config

```yaml
# config/agentgateway.yaml
servers:
  langextract:
    transport: stdio
    command: langextract-mcp
    description: "Extract structured data from documents and URLs"

  scrapling:
    transport: stdio
    command: scrapling-fetch-mcp
    description: "Web scraping with anti-bot bypass"

  filesystem:
    transport: stdio
    command: npx
    args: ["@modelcontextprotocol/server-filesystem", "/data"]
    description: "Read/write files in the agent working directory"
```

## Container Dockerfiles

### mcp-langextract
```dockerfile
FROM python:3.12-slim
RUN pip install uv && uv pip install --system langextract-mcp
CMD ["langextract-mcp"]
```

### mcp-scrapling
```dockerfile
FROM python:3.12-slim
RUN pip install uv && uv pip install --system "scrapling[ai]"
RUN scrapling install
CMD ["scrapling-fetch-mcp"]
```

### mcp-filesystem
```dockerfile
FROM node:22-alpine
RUN npm install -g @modelcontextprotocol/server-filesystem
CMD ["mcp-server-filesystem", "/data"]
```

## Security

- Each container runs with minimal privileges
- Filesystem MCP scoped to `/data` volume only
- Scrapling has no host mounts (network-only access)
- Langextract gets read-only `/data` access
- API keys delivered via environment variables from `.env`, never baked into images
- AgentGateway enforces per-agent RBAC (Phase 2: agents only see tools assigned in AGENTS.md)

## Integration with McClawd

In `mcclawd-tools/src/mcp.rs`, the MCP client connects to AgentGateway via SSE:

```rust
let agentgateway_url = config.agentgateway_url; // "http://localhost:3030"
// rmcp SSE client connects to AgentGateway
// All MCP tools from all servers appear as available tools
// Rig agent gets them via ToolServer
```

The agent sees all MCP tools as native tools — langextract, scrapling, and filesystem tools appear alongside builtin tools (memory.store/recall).
