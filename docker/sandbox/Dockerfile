FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    python3 \
    python3-pip \
    python3-venv \
    nodejs \
    npm \
    git \
    jq \
    && rm -rf /var/lib/apt/lists/*

RUN pip install --break-system-packages uv

RUN useradd -m -s /bin/bash agent

WORKDIR /workspace
USER agent

# Keep container alive — the host orchestrator drives execution via docker exec
ENTRYPOINT ["sleep", "infinity"]
