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
