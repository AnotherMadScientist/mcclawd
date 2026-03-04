# Custom AgentGateway with MCP server binaries installed.
# Uses stdio targets (no HTTP proxy) to avoid upstream body-read bug.
# Based on official AgentGateway image.
FROM cr.agentgateway.dev/agentgateway:1.0.0-alpha.3 AS agentgateway

FROM python:3.12-slim

# Install Node.js 22 + system deps (Chromium for scrapling, git for langextract)
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       curl ca-certificates gnupg git chromium \
    && mkdir -p /etc/apt/keyrings \
    && curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
       | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg \
    && echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_22.x nodistro main" \
       > /etc/apt/sources.list.d/nodesource.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Install MCP servers

# 1. filesystem MCP server (Node.js)
RUN npm install -g @modelcontextprotocol/server-filesystem

# 2. scrapling MCP server (Python) — scrapling-fetch-mcp is a separate package
RUN pip install uv \
    && uv pip install --system "scrapling[ai]" scrapling-fetch-mcp \
    && scrapling install

# 3. langextract MCP server (Python)
RUN git clone https://github.com/larsenweigle/langextract-mcp.git /opt/langextract-mcp \
    && cd /opt/langextract-mcp \
    && uv pip install --system fastmcp langextract

# Copy AgentGateway binary from official image
COPY --from=agentgateway /app/agentgateway /usr/local/bin/agentgateway

# Data volume for filesystem MCP server
VOLUME /data

EXPOSE 3000

ENTRYPOINT ["agentgateway"]
CMD ["-f", "/config/agentgateway.yaml"]
