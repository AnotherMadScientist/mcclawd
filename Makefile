.PHONY: dev dev-api dev-ui test test-e2e build

# Start both API (with auto-restart) and UI dev server.
# Uses a trap to ensure Ctrl+C kills both background processes cleanly.
dev:
	@echo "Starting dev environment (auto-restart on code changes)..."
	@echo "API: http://localhost:8081 (cargo-watch)"
	@echo "UI:  http://localhost:8080 (vite)"
	@echo "Press Ctrl+C to stop both servers."
	@trap 'kill 0' INT TERM; \
		(cd ui && pnpm dev) & \
		cargo watch -w crates/ -x 'run -p mcclawd-api -- serve' & \
		wait

# API server with auto-restart on any Rust file change
dev-api:
	cargo watch -w crates/ -x 'run -p mcclawd-api -- serve'

# UI dev server (Vite already has HMR)
dev-ui:
	cd ui && pnpm dev

# Run all Rust tests
test:
	cargo test --workspace

# Run Playwright E2E tests (requires dev servers running)
test-e2e:
	cd ui && pnpm exec playwright test

# Release build
build:
	cargo build --release -p mcclawd-api

# Build runner Docker image (BuildKit cached — fast after first build)
docker-runner:
	DOCKER_BUILDKIT=1 docker build -t mcclawd-runner:latest -f docker/agent-runner/Dockerfile .

# Full build: binary + runner image
build-all: build docker-runner
