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
