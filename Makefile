.PHONY: dev dev-api dev-ui test test-e2e build

# Start both API (with auto-restart) and UI dev server
dev:
	@echo "Starting dev environment (auto-restart on code changes)..."
	@echo "API: http://localhost:9090 (cargo-watch)"
	@echo "UI:  http://localhost:8080 (vite)"
	@$(MAKE) -j2 dev-api dev-ui

# API server with auto-restart on any Rust file change
dev-api:
	cargo watch -x 'run -p mcclawd-api -- serve' -w crates/

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
