FROM node:22-slim AS builder

WORKDIR /app
COPY app/package.json app/package-lock.json ./
RUN npm ci --ignore-scripts
COPY app/ .
RUN npm run build
# Prune dev deps so we can copy node_modules directly to runtime
RUN npm prune --omit=dev

# --- Runtime ---
FROM node:22-slim

WORKDIR /app
COPY --from=builder /app/package.json /app/package-lock.json ./
COPY --from=builder /app/node_modules/ ./node_modules/
COPY --from=builder /app/api/ ./api/
COPY --from=builder /app/server/ ./server/
COPY --from=builder /app/src-tauri/sidecar/local-api-server.mjs ./local-api-server.mjs
COPY --from=builder /app/dist/ ./dist/

# Thin wrapper — added outside app/, never conflicts with upstream
COPY serve.mjs ./serve.mjs

# Container customization files (defaults, overlays, styling)
COPY container-defaults.js ./container-defaults.js
COPY container-overlays.js ./container-overlays.js
COPY container-overlays.css ./container-overlays.css

ENV NODE_ENV=production
ENV LOCAL_API_PORT=3000
ENV LOCAL_API_MODE=container
ENV LOCAL_API_CLOUD_FALLBACK=true
EXPOSE 3000

CMD ["node", "serve.mjs"]
