# MCP server: filesystem — read/write/search files scoped to /data
FROM node:22-alpine

RUN npm install -g supergateway @modelcontextprotocol/server-filesystem

VOLUME /data
EXPOSE 8000

# Scoped to /data — no access outside the mounted volume
CMD ["supergateway", "--stdio", "mcp-server-filesystem /data", "--port", "8000", "--outputTransport", "sse", "--healthEndpoint", "/healthz"]
