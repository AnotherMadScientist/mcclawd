# syntax=docker/dockerfile:1

# ── Stage 1: Rust builder with BuildKit cache mounts ──────────────
# After first build, only changed Rust code recompiles (~30s).
# Cargo registry, git index, and target dir persist across builds.
FROM rust:1-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release -p mcclawd-runner && \
    cp target/release/agent-runner /agent-runner

# ── Stage 2: Minimal runtime ─────────────────────────────────────
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

COPY --from=builder /agent-runner /usr/local/bin/agent-runner

WORKDIR /workspace
USER agent
ENTRYPOINT ["agent-runner"]
