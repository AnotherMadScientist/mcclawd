# MCP server: langextract — extract structured data from PDFs, docs, URLs
# Wrapped with supergateway to expose HTTP endpoint.
FROM python:3.12-slim

# Install Node.js 22 (for supergateway) and git (for cloning repo)
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates gnupg git \
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

# Install uv and langextract-mcp from GitHub
RUN pip install uv \
    && git clone https://github.com/larsenweigle/langextract-mcp.git /opt/langextract-mcp \
    && cd /opt/langextract-mcp \
    && uv pip install --system fastmcp langextract

WORKDIR /opt/langextract-mcp

EXPOSE 8000

CMD ["supergateway", "--stdio", "fastmcp run src/langextract_mcp/server.py", "--port", "8000", "--outputTransport", "streamableHttp", "--healthEndpoint", "/healthz"]
