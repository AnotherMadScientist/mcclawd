# McClawd UI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a beautiful React dashboard for McClawd with Axum HTTP/WebSocket backend — login, task execution with streaming, and configuration pages.

**Architecture:** Axum server (`:9090`) serves REST + WebSocket API. React/Vite frontend in `ui/` connects via fetch + native WebSocket. Auth via JWT from secrets vault unlock. Dark theme with Tailwind + shadcn/ui.

**Tech Stack:** React 19, TypeScript, Vite, pnpm, Tailwind CSS, shadcn/ui, React Query, React Router v7, Axum, tokio, tower-http

**Design Doc:** `docs/plans/2026-03-04-mcclawd-ui-design.md`

---

## Part A: Backend (Axum API)

### Task 1: Add Axum dependencies

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/mcclawd-api/Cargo.toml`

**Step 1: Add workspace dependencies**

Add to `[workspace.dependencies]` in root `Cargo.toml`:

```toml
# Web server
axum = { version = "0.8", features = ["ws"] }
axum-extra = { version = "0.10", features = ["typed-header"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace"] }
```

**Step 2: Add to mcclawd-api Cargo.toml**

Add to `[dependencies]`:

```toml
axum = { workspace = true }
axum-extra = { workspace = true }
tower = { workspace = true }
tower-http = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
```

**Step 3: Verify it compiles**

Run: `cargo check -p mcclawd-api`
Expected: compiles with no errors

**Step 4: Commit**

```bash
git add Cargo.toml crates/mcclawd-api/Cargo.toml
git commit -m "feat(api): add axum + tower-http dependencies for web server"
```

---

### Task 2: AppState and server skeleton

**Files:**
- Create: `crates/mcclawd-api/src/server/mod.rs`
- Create: `crates/mcclawd-api/src/server/state.rs`
- Modify: `crates/mcclawd-api/src/main.rs`

**Step 1: Create server module**

Create `crates/mcclawd-api/src/server/mod.rs`:

```rust
pub mod state;
pub mod routes;
```

Create `crates/mcclawd-api/src/server/state.rs`:

```rust
use mcclawd_core::McclawdConfig;
use mcclawd_tasks::TaskManager;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<McclawdConfig>>,
    pub tasks: Arc<RwLock<TaskManager>>,
    pub jwt_secret: String,
}

impl AppState {
    pub fn new(config: McclawdConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            tasks: Arc::new(RwLock::new(TaskManager::new())),
            jwt_secret: uuid::Uuid::new_v4().to_string(),
        }
    }
}
```

**Step 2: Create routes skeleton**

Create `crates/mcclawd-api/src/server/routes.rs`:

```rust
use axum::{routing::get, Router};
use super::state::AppState;

pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
}

async fn health() -> &'static str {
    "ok"
}
```

**Step 3: Add `serve` command to main.rs**

Add `mod server;` to main.rs. Add `Serve` variant to `Commands` enum:

```rust
/// Start the web server
Serve {
    /// Port to listen on
    #[arg(short, long, default_value = "9090")]
    port: u16,
},
```

Add match arm:

```rust
Commands::Serve { port } => {
    commands::serve::execute(port).await?;
}
```

**Step 4: Create serve command**

Create `crates/mcclawd-api/src/commands/serve.rs`:

```rust
use crate::server::{routes, state::AppState};
use mcclawd_core::McclawdConfig;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub async fn execute(port: u16) -> anyhow::Result<()> {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".mcclawd")
        .join("config.toml");
    let config = McclawdConfig::load(&config_path)?;
    let state = AppState::new(config);

    let app = routes::api_router()
        .with_state(state)
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("McClawd API server listening on :{port}");
    axum::serve(listener, app).await?;

    Ok(())
}
```

Add `pub mod serve;` to `commands/mod.rs`.

**Step 5: Verify it compiles and runs**

Run: `cargo build -p mcclawd-api`
Expected: compiles

Run: `cargo run -p mcclawd-api -- serve &` then `curl http://localhost:9090/api/health`
Expected: `ok`

Kill the server.

**Step 6: Commit**

```bash
git add crates/mcclawd-api/src/server/ crates/mcclawd-api/src/commands/serve.rs crates/mcclawd-api/src/commands/mod.rs crates/mcclawd-api/src/main.rs
git commit -m "feat(api): axum server skeleton with health endpoint and mc serve command"
```

---

### Task 3: Auth endpoint + JWT middleware

**Files:**
- Create: `crates/mcclawd-api/src/server/auth.rs`
- Modify: `crates/mcclawd-api/src/server/routes.rs`
- Modify: `crates/mcclawd-api/src/server/mod.rs`

**Step 1: Create auth module**

Create `crates/mcclawd-api/src/server/auth.rs`:

```rust
use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
}

pub async fn login(
    State(state): State<super::state::AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    // Phase 0: accept any non-empty password as the master key
    // Phase 1: verify against secrets vault master key
    if body.password.is_empty() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &Claims {
            sub: "mcclawd-user".to_string(),
            exp: (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
        },
        &jsonwebtoken::EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(LoginResponse { token }))
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
}

pub async fn auth_middleware(
    State(state): State<super::state::AppState>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let Some(token) = auth_header else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    );

    match token_data {
        Ok(_) => next.run(req).await,
        Err(_) => StatusCode::UNAUTHORIZED.into_response(),
    }
}
```

**Step 2: Add jsonwebtoken to mcclawd-api Cargo.toml**

```toml
jsonwebtoken = { workspace = true }
```

**Step 3: Wire auth into routes**

Update `crates/mcclawd-api/src/server/routes.rs`:

```rust
use axum::{middleware, routing::{get, post}, Router};
use super::{auth, state::AppState};

pub fn api_router() -> Router<AppState> {
    let public = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(auth::login));

    let protected = Router::new()
        .route("/api/protected-test", get(|| async { "authenticated" }))
        .layer(middleware::from_fn_with_state(
            // state is provided by with_state below
            AppState::new(mcclawd_core::McclawdConfig::default()), // placeholder, overridden
            auth::auth_middleware,
        ));

    // Better approach: use route_layer on the protected routes
    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(auth::login))
}

async fn health() -> &'static str {
    "ok"
}
```

Actually, simpler — use axum's `from_fn_with_state` on a nested router:

```rust
use axum::{middleware, routing::{get, post}, Router};
use super::{auth, state::AppState};

pub fn api_router() -> Router<AppState> {
    let protected = Router::new()
        // Protected routes will be added here
        .route_layer(middleware::from_fn(|req, next: axum::middleware::Next| async {
            // This will be replaced with proper auth below
            next.run(req).await
        }));

    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(auth::login))
        .merge(protected)
}

async fn health() -> &'static str {
    "ok"
}
```

Note: The exact middleware wiring depends on axum 0.8 API. The implementer should check `axum::middleware::from_fn_with_state` signature and adapt. The key pattern is:
- `/api/auth/login` and `/api/health` are public
- All other `/api/*` routes require `Authorization: Bearer <jwt>` header
- Use `middleware::from_fn_with_state(state, auth::auth_middleware)` as a route layer

**Step 4: Verify**

Run: `cargo check -p mcclawd-api`

**Step 5: Commit**

```bash
git add crates/mcclawd-api/src/server/auth.rs crates/mcclawd-api/src/server/routes.rs crates/mcclawd-api/src/server/mod.rs
git commit -m "feat(api): auth login endpoint + JWT middleware"
```

---

### Task 4: Tasks API endpoints

**Files:**
- Create: `crates/mcclawd-api/src/server/tasks.rs`
- Modify: `crates/mcclawd-api/src/server/routes.rs`
- Modify: `crates/mcclawd-api/src/server/state.rs`

**Step 1: Extend TaskManager for API use**

The existing `TaskManager` in `crates/mcclawd-tasks/src/manager.rs` only tracks one task and has no history. For the API we need a list of tasks. Add to `TaskManager`:

```rust
// In crates/mcclawd-tasks/src/manager.rs — add:
pub fn all_tasks(&self) -> Vec<&TaskRecord> {
    // Phase 0: returns current task as a slice
    self.current.as_ref().into_iter().collect()
}
```

Add `Serialize` derive to `TaskRecord`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: TaskId,
    pub prompt: String,
    pub status: TaskStatus,
}
```

(This requires adding `use mcclawd_core::types::TaskId;` and ensuring `TaskId` derives `Serialize, Deserialize` — check `crates/mcclawd-core/src/types.rs`)

**Step 2: Create tasks handler**

Create `crates/mcclawd-api/src/server/tasks.rs`:

```rust
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use super::state::AppState;

#[derive(Deserialize)]
pub struct CreateTaskRequest {
    pub prompt: String,
    pub workspace: Option<String>,
    pub model: Option<String>,
}

pub async fn list_tasks(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let tasks = state.tasks.read().await;
    let list: Vec<_> = tasks.all_tasks().into_iter().cloned().collect();
    Json(serde_json::to_value(&list).unwrap_or_default())
}

pub async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<CreateTaskRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut tasks = state.tasks.write().await;
    let id = tasks.start_task(body.prompt.clone());
    let response = serde_json::json!({
        "id": id.to_string(),
        "prompt": body.prompt,
        "status": "Running"
    });
    (StatusCode::CREATED, Json(response))
}

pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let tasks = state.tasks.read().await;
    match tasks.current_task() {
        Some(task) if task.id.to_string() == id => {
            Ok(Json(serde_json::to_value(task).unwrap_or_default()))
        }
        _ => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn cancel_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    let mut tasks = state.tasks.write().await;
    if let Some(task) = tasks.current_task() {
        if task.id.to_string() == id {
            let task_id = task.id.clone();
            tasks.fail_task(&task_id, "Cancelled by user".to_string());
            return StatusCode::NO_CONTENT;
        }
    }
    StatusCode::NOT_FOUND
}
```

**Step 3: Wire task routes**

In `routes.rs`, add to the protected router:

```rust
use super::tasks;

// Inside protected router:
.route("/api/tasks", get(tasks::list_tasks).post(tasks::create_task))
.route("/api/tasks/{id}", get(tasks::get_task).delete(tasks::cancel_task))
```

**Step 4: Verify**

Run: `cargo check -p mcclawd-api`

**Step 5: Commit**

```bash
git add crates/mcclawd-api/src/server/tasks.rs crates/mcclawd-api/src/server/routes.rs crates/mcclawd-tasks/src/manager.rs
git commit -m "feat(api): tasks CRUD endpoints (list, create, get, cancel)"
```

---

### Task 5: Workspace, Secrets, Config, MCP endpoints

**Files:**
- Create: `crates/mcclawd-api/src/server/workspace.rs`
- Create: `crates/mcclawd-api/src/server/secrets.rs`
- Create: `crates/mcclawd-api/src/server/config.rs`
- Create: `crates/mcclawd-api/src/server/mcp.rs`
- Modify: `crates/mcclawd-api/src/server/routes.rs`

**Step 1: Workspace handlers**

Create `crates/mcclawd-api/src/server/workspace.rs`:

```rust
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::state::AppState;

#[derive(Serialize)]
pub struct WorkspaceFile {
    name: String,
}

pub async fn list_files(
    State(state): State<AppState>,
) -> Json<Vec<WorkspaceFile>> {
    let files = vec!["SOUL.md", "AGENTS.md", "USER.md"]
        .into_iter()
        .map(|name| WorkspaceFile { name: name.to_string() })
        .collect();
    Json(files)
}

pub async fn get_file(
    State(state): State<AppState>,
    Path(file): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let config = state.config.read().await;
    let workspace = config.agent.default_workspace.clone();
    let path = config.workspaces_dir().join(&workspace).join(&file);
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(Json(serde_json::json!({ "name": file, "content": content }))),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Deserialize)]
pub struct UpdateFileRequest {
    pub content: String,
}

pub async fn update_file(
    State(state): State<AppState>,
    Path(file): Path<String>,
    Json(body): Json<UpdateFileRequest>,
) -> StatusCode {
    let config = state.config.read().await;
    let workspace = config.agent.default_workspace.clone();
    let path = config.workspaces_dir().join(&workspace).join(&file);
    match std::fs::write(&path, &body.content) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
```

**Step 2: Secrets handlers**

Create `crates/mcclawd-api/src/server/secrets.rs`:

```rust
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::state::AppState;

#[derive(Serialize)]
pub struct SecretEntry {
    name: String,
}

pub async fn list_secrets(
    State(state): State<AppState>,
) -> Json<Vec<SecretEntry>> {
    // Phase 0: return empty list. Phase 1: integrate with SecretBackend
    Json(vec![])
}

#[derive(Deserialize)]
pub struct AddSecretRequest {
    pub name: String,
    pub value: String,
}

pub async fn add_secret(
    Json(body): Json<AddSecretRequest>,
) -> StatusCode {
    // Phase 0: stub. Phase 1: store via SecretBackend
    StatusCode::CREATED
}

pub async fn delete_secret(
    Path(name): Path<String>,
) -> StatusCode {
    // Phase 0: stub. Phase 1: delete via SecretBackend
    StatusCode::NO_CONTENT
}
```

**Step 3: Config handlers**

Create `crates/mcclawd-api/src/server/config.rs`:

```rust
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};

use super::state::AppState;

pub async fn get_config(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    Json(serde_json::to_value(&*config).unwrap_or_default())
}

pub async fn update_config(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    // Phase 0: stub — just log the update request
    tracing::info!("Config update requested: {:?}", body);
    StatusCode::NO_CONTENT
}
```

**Step 4: MCP handlers**

Create `crates/mcclawd-api/src/server/mcp.rs`:

```rust
use axum::{extract::State, Json};
use serde::Serialize;

use super::state::AppState;

#[derive(Serialize)]
pub struct McpServerInfo {
    pub name: String,
    pub image: String,
    pub port: u16,
}

pub async fn list_servers(
    State(state): State<AppState>,
) -> Json<Vec<McpServerInfo>> {
    let config = state.config.read().await;
    let servers = config
        .mcp
        .servers
        .iter()
        .map(|s| McpServerInfo {
            name: s.name.clone(),
            image: s.image.clone(),
            port: s.port,
        })
        .collect();
    Json(servers)
}
```

**Step 5: Wire all routes**

Update `routes.rs` to include all endpoints:

```rust
use axum::{middleware, routing::{get, post, put, delete}, Router};
use super::{auth, tasks, workspace, secrets, config, mcp, state::AppState};

pub fn api_router() -> Router<AppState> {
    let protected = Router::new()
        .route("/api/tasks", get(tasks::list_tasks).post(tasks::create_task))
        .route("/api/tasks/{id}", get(tasks::get_task).delete(tasks::cancel_task))
        .route("/api/workspace", get(workspace::list_files))
        .route("/api/workspace/{file}", get(workspace::get_file).put(workspace::update_file))
        .route("/api/secrets", get(secrets::list_secrets).post(secrets::add_secret))
        .route("/api/secrets/{name}", delete(secrets::delete_secret))
        .route("/api/config", get(config::get_config).put(config::update_config))
        .route("/api/mcp/servers", get(mcp::list_servers));
    // TODO: add auth middleware layer once login is working end-to-end

    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(auth::login))
        .merge(protected)
}

async fn health() -> &'static str {
    "ok"
}
```

Update `crates/mcclawd-api/src/server/mod.rs`:

```rust
pub mod auth;
pub mod config;
pub mod mcp;
pub mod routes;
pub mod secrets;
pub mod state;
pub mod tasks;
pub mod workspace;
```

**Step 6: Verify**

Run: `cargo check -p mcclawd-api`

**Step 7: Commit**

```bash
git add crates/mcclawd-api/src/server/
git commit -m "feat(api): workspace, secrets, config, mcp REST endpoints"
```

---

### Task 6: WebSocket streaming endpoint

**Files:**
- Create: `crates/mcclawd-api/src/server/ws.rs`
- Modify: `crates/mcclawd-api/src/server/routes.rs`
- Modify: `crates/mcclawd-api/src/server/mod.rs`

**Step 1: Create WebSocket handler**

Create `crates/mcclawd-api/src/server/ws.rs`:

```rust
use axum::{
    extract::{Path, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::Response,
};
use mcclawd_channels::types::OutboundChunk;

use super::state::AppState;

pub async fn task_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, id))
}

async fn handle_socket(mut socket: WebSocket, state: AppState, task_id: String) {
    // Phase 0: send mock streaming events to verify frontend works
    // Phase 1: connect to actual agent execution via Channel trait

    let events = vec![
        OutboundChunk::TextDelta("Thinking about your request...".to_string()),
        OutboundChunk::ToolStart { name: "memory.recall".to_string() },
        OutboundChunk::ToolEnd { name: "memory.recall".to_string(), summary: Some("No memories found".to_string()) },
        OutboundChunk::TextBlock("Based on my analysis, here is the result.".to_string()),
        OutboundChunk::Done,
    ];

    for event in events {
        let json = serde_json::to_string(&event).unwrap_or_default();
        if socket.send(Message::Text(json.into())).await.is_err() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}
```

**Step 2: Wire WebSocket route**

In `routes.rs`, add to protected router:

```rust
use super::ws;

// Add to protected routes:
.route("/api/tasks/{id}/stream", get(ws::task_stream))
```

**Step 3: Verify**

Run: `cargo check -p mcclawd-api`

**Step 4: Commit**

```bash
git add crates/mcclawd-api/src/server/ws.rs crates/mcclawd-api/src/server/routes.rs crates/mcclawd-api/src/server/mod.rs
git commit -m "feat(api): WebSocket streaming endpoint for task execution"
```

---

## Part B: Frontend Scaffold

### Task 7: Scaffold Vite + pnpm workspace

**Files:**
- Create: `ui/pnpm-workspace.yaml`
- Create: `ui/package.json`
- Create: `ui/packages/app/package.json`
- Create: `ui/packages/app/vite.config.ts`
- Create: `ui/packages/app/tsconfig.json`
- Create: `ui/packages/app/index.html`
- Create: `ui/packages/app/src/main.tsx`
- Create: `ui/packages/app/src/App.tsx`

**Step 1: Create pnpm workspace**

Create `ui/pnpm-workspace.yaml`:

```yaml
packages:
  - "packages/*"
```

Create `ui/package.json`:

```json
{
  "name": "mcclawd-ui",
  "private": true,
  "scripts": {
    "dev": "pnpm --filter @mcclawd/app dev",
    "build": "pnpm --filter @mcclawd/app build",
    "lint": "pnpm --filter @mcclawd/app lint"
  }
}
```

**Step 2: Create app package**

Create `ui/packages/app/package.json`:

```json
{
  "name": "@mcclawd/app",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "lint": "eslint .",
    "preview": "vite preview"
  },
  "dependencies": {
    "react": "^19.0.0",
    "react-dom": "^19.0.0",
    "react-router": "^7.0.0",
    "@tanstack/react-query": "^5.0.0"
  },
  "devDependencies": {
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.0.0",
    "typescript": "^5.7.0",
    "vite": "^6.0.0",
    "tailwindcss": "^4.0.0",
    "@tailwindcss/vite": "^4.0.0"
  }
}
```

Create `ui/packages/app/vite.config.ts`:

```typescript
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 8080,
    proxy: {
      "/api": {
        target: "http://localhost:9090",
        changeOrigin: true,
        ws: true,
      },
    },
  },
});
```

Create `ui/packages/app/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2023", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "isolatedModules": true,
    "moduleDetection": "force",
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "noUncheckedSideEffectImports": true,
    "paths": {
      "@/*": ["./src/*"]
    },
    "baseUrl": "."
  },
  "include": ["src"]
}
```

Create `ui/packages/app/index.html`:

```html
<!DOCTYPE html>
<html lang="en" class="dark">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>McClawd</title>
  </head>
  <body class="bg-zinc-950 text-zinc-100 antialiased">
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

Create `ui/packages/app/src/main.tsx`:

```tsx
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./index.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
```

Create `ui/packages/app/src/index.css`:

```css
@import "tailwindcss";
```

Create `ui/packages/app/src/App.tsx`:

```tsx
export default function App() {
  return (
    <div className="flex items-center justify-center min-h-screen">
      <h1 className="text-4xl font-bold text-white">McClawd</h1>
    </div>
  );
}
```

**Step 3: Install and verify**

```bash
cd ui && pnpm install && pnpm dev
```

Expected: Vite dev server on :8080 showing "McClawd" centered on dark background.

Stop the dev server.

**Step 4: Add .gitignore for ui**

Create `ui/.gitignore`:

```
node_modules/
dist/
.vite/
```

**Step 5: Commit**

```bash
git add ui/
git commit -m "feat(ui): scaffold vite + pnpm workspace with react + tailwind"
```

---

### Task 8: shadcn/ui setup + theme

**Files:**
- Create: `ui/packages/app/src/lib/utils.ts`
- Create: `ui/packages/app/components.json`
- Modify: `ui/packages/app/src/index.css`
- Modify: `ui/packages/app/package.json`

**Step 1: Install shadcn/ui dependencies**

```bash
cd ui/packages/app
pnpm add class-variance-authority clsx tailwind-merge lucide-react
```

Create `ui/packages/app/src/lib/utils.ts`:

```typescript
import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
```

**Step 2: Set up dark theme CSS variables**

Update `ui/packages/app/src/index.css`:

```css
@import "tailwindcss";

@theme {
  --color-background: oklch(0.145 0 0);
  --color-foreground: oklch(0.985 0 0);
  --color-card: oklch(0.17 0 0);
  --color-card-foreground: oklch(0.985 0 0);
  --color-primary: oklch(0.65 0.2 250);
  --color-primary-foreground: oklch(0.985 0 0);
  --color-muted: oklch(0.25 0 0);
  --color-muted-foreground: oklch(0.6 0 0);
  --color-border: oklch(0.3 0 0);
  --color-accent: oklch(0.65 0.15 170);
  --color-destructive: oklch(0.55 0.2 25);
  --radius-sm: 0.5rem;
  --radius-md: 0.75rem;
  --radius-lg: 1rem;
}

body {
  background-color: var(--color-background);
  color: var(--color-foreground);
}
```

**Step 3: Initialize shadcn**

```bash
cd ui/packages/app
pnpm dlx shadcn@latest init -d
```

If prompted, choose: TypeScript, default style, CSS variables, zinc base color.

Then add the components we'll need:

```bash
pnpm dlx shadcn@latest add button card input dialog badge separator scroll-area textarea
```

**Step 4: Verify**

```bash
cd ui && pnpm dev
```

Expected: Still compiles and runs.

**Step 5: Commit**

```bash
git add ui/
git commit -m "feat(ui): shadcn/ui setup with dark theme and base components"
```

---

### Task 9: API client + auth context

**Files:**
- Create: `ui/packages/app/src/api/client.ts`
- Create: `ui/packages/app/src/api/types.ts`
- Create: `ui/packages/app/src/hooks/useAuth.tsx`

**Step 1: Create API types**

Create `ui/packages/app/src/api/types.ts`:

```typescript
export interface Task {
  id: string;
  prompt: string;
  status: "Running" | "Completed" | { Failed: string };
}

export interface WorkspaceFile {
  name: string;
  content?: string;
}

export interface McpServer {
  name: string;
  image: string;
  port: number;
}

export interface McclawdConfig {
  data_dir: string;
  agent: {
    max_turns: number;
    model: string;
    default_workspace: string;
  };
  providers: {
    anthropic?: { api_key_secret: string };
    openai?: { api_key_secret: string };
    ollama?: { url: string };
  };
  mcp: {
    agentgateway_url: string;
    servers: McpServer[];
  };
}

// WebSocket stream chunk types (matches Rust OutboundChunk)
export type StreamChunk =
  | { TextDelta: string }
  | { TextBlock: string }
  | { ToolStart: { name: string } }
  | { ToolEnd: { name: string; summary: string | null } }
  | "Done"
  | { Error: string };
```

**Step 2: Create API client**

Create `ui/packages/app/src/api/client.ts`:

```typescript
let authToken: string | null = null;

export function setToken(token: string) {
  authToken = token;
}

export function clearToken() {
  authToken = null;
}

export function getToken() {
  return authToken;
}

async function apiFetch<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...((options.headers as Record<string, string>) || {}),
  };

  if (authToken) {
    headers["Authorization"] = `Bearer ${authToken}`;
  }

  const res = await fetch(path, { ...options, headers });

  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${res.statusText}`);
  }

  if (res.status === 204) return undefined as T;
  return res.json();
}

export const api = {
  auth: {
    login: (password: string) =>
      apiFetch<{ token: string }>("/api/auth/login", {
        method: "POST",
        body: JSON.stringify({ password }),
      }),
  },
  tasks: {
    list: () => apiFetch<import("./types").Task[]>("/api/tasks"),
    create: (prompt: string, workspace?: string, model?: string) =>
      apiFetch<import("./types").Task>("/api/tasks", {
        method: "POST",
        body: JSON.stringify({ prompt, workspace, model }),
      }),
    get: (id: string) => apiFetch<import("./types").Task>(`/api/tasks/${id}`),
    cancel: (id: string) =>
      apiFetch<void>(`/api/tasks/${id}`, { method: "DELETE" }),
  },
  workspace: {
    list: () => apiFetch<import("./types").WorkspaceFile[]>("/api/workspace"),
    get: (file: string) =>
      apiFetch<import("./types").WorkspaceFile>(`/api/workspace/${file}`),
    update: (file: string, content: string) =>
      apiFetch<void>(`/api/workspace/${file}`, {
        method: "PUT",
        body: JSON.stringify({ content }),
      }),
  },
  secrets: {
    list: () => apiFetch<{ name: string }[]>("/api/secrets"),
    add: (name: string, value: string) =>
      apiFetch<void>("/api/secrets", {
        method: "POST",
        body: JSON.stringify({ name, value }),
      }),
    delete: (name: string) =>
      apiFetch<void>(`/api/secrets/${name}`, { method: "DELETE" }),
  },
  config: {
    get: () => apiFetch<import("./types").McclawdConfig>("/api/config"),
    update: (config: Partial<import("./types").McclawdConfig>) =>
      apiFetch<void>("/api/config", {
        method: "PUT",
        body: JSON.stringify(config),
      }),
  },
  mcp: {
    servers: () => apiFetch<import("./types").McpServer[]>("/api/mcp/servers"),
  },
};
```

**Step 3: Create auth context**

Create `ui/packages/app/src/hooks/useAuth.tsx`:

```tsx
import { createContext, useContext, useState, useCallback, type ReactNode } from "react";
import { api, setToken, clearToken, getToken } from "../api/client";

interface AuthContextType {
  isAuthenticated: boolean;
  login: (password: string) => Promise<void>;
  logout: () => void;
}

const AuthContext = createContext<AuthContextType | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [isAuthenticated, setIsAuthenticated] = useState(!!getToken());

  const login = useCallback(async (password: string) => {
    const { token } = await api.auth.login(password);
    setToken(token);
    setIsAuthenticated(true);
  }, []);

  const logout = useCallback(() => {
    clearToken();
    setIsAuthenticated(false);
  }, []);

  return (
    <AuthContext.Provider value={{ isAuthenticated, login, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
```

**Step 4: Verify**

```bash
cd ui && pnpm dev
```

Expected: compiles without errors.

**Step 5: Commit**

```bash
git add ui/packages/app/src/api/ ui/packages/app/src/hooks/
git commit -m "feat(ui): typed API client + auth context with JWT in-memory storage"
```

---

### Task 10: Router + Layout with sidebar

**Files:**
- Create: `ui/packages/app/src/components/Layout.tsx`
- Create: `ui/packages/app/src/components/Sidebar.tsx`
- Modify: `ui/packages/app/src/App.tsx`

**Step 1: Create Sidebar component**

Create `ui/packages/app/src/components/Sidebar.tsx`:

```tsx
import { NavLink } from "react-router";
import {
  LayoutDashboard,
  FileText,
  Puzzle,
  Server,
  KeyRound,
  Settings,
  ChevronDown,
  LogOut,
} from "lucide-react";
import { useState } from "react";
import { cn } from "../lib/utils";
import { useAuth } from "../hooks/useAuth";

const configItems = [
  { to: "/config/workspace", icon: FileText, label: "Workspace" },
  { to: "/config/skills", icon: Puzzle, label: "Skills" },
  { to: "/config/mcp", icon: Server, label: "MCP Servers" },
  { to: "/config/secrets", icon: KeyRound, label: "Secrets" },
  { to: "/config/settings", icon: Settings, label: "Settings" },
];

export function Sidebar() {
  const [configOpen, setConfigOpen] = useState(true);
  const { logout } = useAuth();

  return (
    <aside className="flex flex-col w-64 border-r border-border bg-zinc-950 h-screen">
      {/* Logo */}
      <div className="flex items-center gap-3 px-6 py-5 border-b border-border">
        <div className="w-8 h-8 rounded-full bg-primary flex items-center justify-center text-sm font-bold">
          M
        </div>
        <span className="text-lg font-semibold tracking-tight">McClawd</span>
      </div>

      {/* Nav */}
      <nav className="flex-1 px-3 py-4 space-y-1 overflow-y-auto">
        <NavLink
          to="/"
          end
          className={({ isActive }) =>
            cn(
              "flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors",
              isActive
                ? "bg-primary/10 text-primary"
                : "text-muted-foreground hover:bg-muted hover:text-foreground"
            )
          }
        >
          <LayoutDashboard className="w-4 h-4" />
          Tasks
        </NavLink>

        {/* Configuration section */}
        <button
          onClick={() => setConfigOpen(!configOpen)}
          className="flex items-center justify-between w-full px-3 py-2 rounded-md text-sm font-medium text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
        >
          <span className="flex items-center gap-3">
            <Settings className="w-4 h-4" />
            Configuration
          </span>
          <ChevronDown
            className={cn(
              "w-4 h-4 transition-transform",
              configOpen && "rotate-180"
            )}
          />
        </button>

        {configOpen && (
          <div className="ml-4 space-y-1">
            {configItems.map(({ to, icon: Icon, label }) => (
              <NavLink
                key={to}
                to={to}
                className={({ isActive }) =>
                  cn(
                    "flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors",
                    isActive
                      ? "bg-primary/10 text-primary"
                      : "text-muted-foreground hover:bg-muted hover:text-foreground"
                  )
                }
              >
                <Icon className="w-4 h-4" />
                {label}
              </NavLink>
            ))}
          </div>
        )}
      </nav>

      {/* Footer */}
      <div className="px-3 py-4 border-t border-border">
        <button
          onClick={logout}
          className="flex items-center gap-3 px-3 py-2 w-full rounded-md text-sm text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
        >
          <LogOut className="w-4 h-4" />
          Sign Out
        </button>
      </div>
    </aside>
  );
}
```

**Step 2: Create Layout component**

Create `ui/packages/app/src/components/Layout.tsx`:

```tsx
import { Outlet, Navigate } from "react-router";
import { Sidebar } from "./Sidebar";
import { useAuth } from "../hooks/useAuth";

export function Layout() {
  const { isAuthenticated } = useAuth();

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar />
      <main className="flex-1 overflow-y-auto p-8">
        <Outlet />
      </main>
    </div>
  );
}
```

**Step 3: Set up router in App.tsx**

Update `ui/packages/app/src/App.tsx`:

```tsx
import { BrowserRouter, Routes, Route, Navigate } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AuthProvider, useAuth } from "./hooks/useAuth";
import { Layout } from "./components/Layout";

// Placeholder pages — will be built in later tasks
function Placeholder({ name }: { name: string }) {
  return (
    <div className="flex items-center justify-center h-64">
      <p className="text-muted-foreground text-lg">{name} — coming soon</p>
    </div>
  );
}

function LoginPlaceholder() {
  return (
    <div className="flex items-center justify-center min-h-screen">
      <p className="text-muted-foreground">Login page — coming in Task 11</p>
    </div>
  );
}

const queryClient = new QueryClient();

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <BrowserRouter>
          <Routes>
            <Route path="/login" element={<LoginPlaceholder />} />
            <Route element={<Layout />}>
              <Route index element={<Placeholder name="Tasks" />} />
              <Route path="tasks/new" element={<Placeholder name="New Task" />} />
              <Route path="tasks/:id" element={<Placeholder name="Task Detail" />} />
              <Route path="config/workspace" element={<Placeholder name="Workspace" />} />
              <Route path="config/skills" element={<Placeholder name="Skills" />} />
              <Route path="config/mcp" element={<Placeholder name="MCP Servers" />} />
              <Route path="config/secrets" element={<Placeholder name="Secrets" />} />
              <Route path="config/settings" element={<Placeholder name="Settings" />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </AuthProvider>
    </QueryClientProvider>
  );
}
```

**Step 4: Install react-router**

```bash
cd ui/packages/app && pnpm add react-router
```

**Step 5: Verify**

```bash
cd ui && pnpm dev
```

Expected: sidebar renders with nav items, content area shows placeholders. Clicking nav items changes URL and active state.

Note: Since auth is not wired yet, you may need to temporarily set `isAuthenticated` to `true` in the Layout component or AuthProvider to see the layout. Revert before committing.

**Step 6: Commit**

```bash
git add ui/packages/app/src/components/ ui/packages/app/src/App.tsx
git commit -m "feat(ui): router + layout with collapsible sidebar navigation"
```

---

## Part C: Pages

### Task 11: Login page — beautiful, minimal

**Files:**
- Create: `ui/packages/app/src/pages/LoginPage.tsx`
- Create: `ui/packages/app/public/mcclawd-logo.svg`
- Modify: `ui/packages/app/src/App.tsx`

**Step 1: Create McClawd logo SVG**

Create `ui/packages/app/public/mcclawd-logo.svg` — a simple claw/paw icon in a circle. Use a stylized "M" or claw mark. The implementer should create a clean SVG, approximately:

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 200" fill="none">
  <circle cx="100" cy="100" r="96" stroke="url(#grad)" stroke-width="4" fill="rgba(255,255,255,0.03)"/>
  <defs>
    <linearGradient id="grad" x1="0" y1="0" x2="200" y2="200">
      <stop offset="0%" stop-color="#6366f1"/>
      <stop offset="100%" stop-color="#06b6d4"/>
    </linearGradient>
  </defs>
  <text x="100" y="120" text-anchor="middle" fill="white" font-size="72" font-family="system-ui" font-weight="700">M</text>
</svg>
```

**Step 2: Create LoginPage**

Create `ui/packages/app/src/pages/LoginPage.tsx`:

```tsx
import { useState } from "react";
import { useNavigate } from "react-router";
import { useAuth } from "../hooks/useAuth";

export function LoginPage() {
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const { login, isAuthenticated } = useAuth();
  const navigate = useNavigate();

  if (isAuthenticated) {
    navigate("/", { replace: true });
    return null;
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      await login(password);
      navigate("/", { replace: true });
    } catch {
      setError("Invalid password");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="relative flex items-center justify-center min-h-screen overflow-hidden bg-zinc-950">
      {/* Ambient glow */}
      <div className="absolute inset-0 overflow-hidden">
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-primary/5 rounded-full blur-3xl" />
        <div className="absolute top-1/3 left-1/3 w-[400px] h-[400px] bg-accent/5 rounded-full blur-3xl" />
      </div>

      <div className="relative z-10 flex flex-col items-center gap-8">
        {/* Circular logo with glow ring */}
        <div className="relative">
          <div className="absolute inset-0 rounded-full bg-primary/20 blur-xl animate-pulse" />
          <img
            src="/mcclawd-logo.svg"
            alt="McClawd"
            className="relative w-32 h-32 rounded-full"
          />
        </div>

        {/* Title */}
        <h1 className="text-2xl font-light tracking-widest text-zinc-400 uppercase">
          McClawd
        </h1>

        {/* Password form */}
        <form onSubmit={handleSubmit} className="flex flex-col items-center gap-4 w-72">
          <div className="relative w-full">
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Enter master password"
              autoFocus
              className="w-full bg-transparent border-0 border-b border-zinc-700 px-2 py-3 text-center text-zinc-200 placeholder:text-zinc-600 focus:outline-none focus:border-primary transition-colors"
            />
          </div>

          {error && (
            <p className="text-sm text-destructive animate-in fade-in">{error}</p>
          )}

          <button
            type="submit"
            disabled={loading || !password}
            className="w-full py-2.5 rounded-lg bg-primary/10 text-primary border border-primary/20 hover:bg-primary/20 disabled:opacity-40 transition-all text-sm font-medium"
          >
            {loading ? "Unlocking..." : "Unlock"}
          </button>
        </form>
      </div>
    </div>
  );
}
```

**Step 3: Wire LoginPage into App.tsx**

Replace the `LoginPlaceholder` import/usage with:

```tsx
import { LoginPage } from "./pages/LoginPage";

// In Routes:
<Route path="/login" element={<LoginPage />} />
```

**Step 4: Verify**

Run: `cd ui && pnpm dev`
Expected: Beautiful dark login page with circular logo, ambient glow, centered password field.

**Step 5: Commit**

```bash
git add ui/packages/app/src/pages/LoginPage.tsx ui/packages/app/public/mcclawd-logo.svg ui/packages/app/src/App.tsx
git commit -m "feat(ui): beautiful login page with circular logo and ambient glow"
```

---

### Task 12: Tasks home page — graphical dashboard

**Files:**
- Create: `ui/packages/app/src/pages/TasksPage.tsx`
- Create: `ui/packages/app/src/components/TaskCard.tsx`
- Modify: `ui/packages/app/src/App.tsx`

**Step 1: Create TaskCard component**

Create `ui/packages/app/src/components/TaskCard.tsx`:

```tsx
import { useNavigate } from "react-router";
import { Activity, CheckCircle2, XCircle, Clock } from "lucide-react";
import { cn } from "../lib/utils";
import type { Task } from "../api/types";

function statusInfo(status: Task["status"]) {
  if (status === "Running")
    return { icon: Activity, label: "Running", color: "text-blue-400", pulse: true };
  if (status === "Completed")
    return { icon: CheckCircle2, label: "Completed", color: "text-emerald-400", pulse: false };
  return { icon: XCircle, label: "Failed", color: "text-red-400", pulse: false };
}

export function TaskCard({ task }: { task: Task }) {
  const navigate = useNavigate();
  const { icon: Icon, label, color, pulse } = statusInfo(task.status);

  return (
    <button
      onClick={() => navigate(`/tasks/${task.id}`)}
      className="w-full text-left p-5 rounded-xl bg-card border border-border hover:border-primary/30 hover:bg-card/80 transition-all group"
    >
      <div className="flex items-start justify-between gap-4">
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-foreground truncate group-hover:text-primary transition-colors">
            {task.prompt}
          </p>
          <p className="text-xs text-muted-foreground mt-1">ID: {task.id.slice(0, 8)}</p>
        </div>
        <div className={cn("flex items-center gap-1.5 text-xs font-medium", color)}>
          <Icon className={cn("w-4 h-4", pulse && "animate-pulse")} />
          {label}
        </div>
      </div>
    </button>
  );
}
```

**Step 2: Create TasksPage**

Create `ui/packages/app/src/pages/TasksPage.tsx`:

```tsx
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { Plus, Zap, CheckCircle2, Server } from "lucide-react";
import { api } from "../api/client";
import { TaskCard } from "../components/TaskCard";
import type { Task } from "../api/types";

export function TasksPage() {
  const navigate = useNavigate();
  const { data: tasks = [], isLoading } = useQuery({
    queryKey: ["tasks"],
    queryFn: api.tasks.list,
    refetchInterval: 3000,
  });

  const running = tasks.filter((t) => t.status === "Running");
  const completed = tasks.filter((t) => t.status === "Completed");
  const failed = tasks.filter(
    (t) => typeof t.status === "object" && "Failed" in t.status
  );

  return (
    <div className="max-w-5xl mx-auto space-y-8">
      {/* Hero */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Tasks</h1>
          <p className="text-muted-foreground mt-1">
            Monitor and launch agent tasks
          </p>
        </div>
        <button
          onClick={() => navigate("/tasks/new")}
          className="flex items-center gap-2 px-5 py-2.5 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 transition-colors font-medium text-sm"
        >
          <Plus className="w-4 h-4" />
          New Task
        </button>
      </div>

      {/* Stats row */}
      <div className="grid grid-cols-3 gap-4">
        <StatCard
          icon={Zap}
          label="Running"
          value={running.length}
          color="text-blue-400"
        />
        <StatCard
          icon={CheckCircle2}
          label="Completed"
          value={completed.length}
          color="text-emerald-400"
        />
        <StatCard
          icon={Server}
          label="Failed"
          value={failed.length}
          color="text-red-400"
        />
      </div>

      {/* Running tasks */}
      {running.length > 0 && (
        <section>
          <h2 className="text-lg font-semibold mb-3 flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-blue-400 animate-pulse" />
            Running
          </h2>
          <div className="space-y-3">
            {running.map((t) => (
              <TaskCard key={t.id} task={t} />
            ))}
          </div>
        </section>
      )}

      {/* Recent tasks */}
      <section>
        <h2 className="text-lg font-semibold mb-3">Recent</h2>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading...</p>
        ) : tasks.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <div className="w-16 h-16 rounded-full bg-muted flex items-center justify-center mb-4">
              <Zap className="w-8 h-8 text-muted-foreground" />
            </div>
            <p className="text-muted-foreground">No tasks yet</p>
            <p className="text-sm text-muted-foreground mt-1">
              Create your first task to get started
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {[...completed, ...failed].map((t) => (
              <TaskCard key={t.id} task={t} />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function StatCard({
  icon: Icon,
  label,
  value,
  color,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  value: number;
  color: string;
}) {
  return (
    <div className="p-4 rounded-xl bg-card border border-border">
      <div className="flex items-center gap-3">
        <Icon className={cn("w-5 h-5", color)} />
        <div>
          <p className="text-2xl font-bold">{value}</p>
          <p className="text-xs text-muted-foreground">{label}</p>
        </div>
      </div>
    </div>
  );
}

// Need cn import
import { cn } from "../lib/utils";
```

**Step 3: Wire into App.tsx**

Replace the Tasks placeholder route:

```tsx
import { TasksPage } from "./pages/TasksPage";

// In Routes:
<Route index element={<TasksPage />} />
```

**Step 4: Verify**

Run backend: `cargo run -p mcclawd-api -- serve &`
Run frontend: `cd ui && pnpm dev`

Expected: Tasks page shows stat cards and empty state. "New Task" button navigates to `/tasks/new`.

**Step 5: Commit**

```bash
git add ui/packages/app/src/pages/TasksPage.tsx ui/packages/app/src/components/TaskCard.tsx ui/packages/app/src/App.tsx
git commit -m "feat(ui): graphical tasks dashboard with stats and task cards"
```

---

### Task 13: New Task page — visual resource panel

**Files:**
- Create: `ui/packages/app/src/pages/NewTaskPage.tsx`
- Create: `ui/packages/app/src/components/ResourceCard.tsx`
- Modify: `ui/packages/app/src/App.tsx`

**Step 1: Create ResourceCard component**

Create `ui/packages/app/src/components/ResourceCard.tsx`:

```tsx
import { cn } from "../lib/utils";

interface ResourceCardProps {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  description: string;
  items?: string[];
  status?: "active" | "inactive";
  color?: string;
}

export function ResourceCard({
  icon: Icon,
  title,
  description,
  items,
  status,
  color = "text-primary",
}: ResourceCardProps) {
  return (
    <div className="p-4 rounded-xl bg-card border border-border hover:border-primary/20 transition-colors">
      <div className="flex items-start gap-3">
        <div
          className={cn(
            "w-10 h-10 rounded-lg flex items-center justify-center shrink-0",
            status === "active" ? "bg-emerald-500/10" : "bg-muted"
          )}
        >
          <Icon className={cn("w-5 h-5", color)} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-medium">{title}</h3>
            {status && (
              <span
                className={cn(
                  "w-2 h-2 rounded-full",
                  status === "active" ? "bg-emerald-400" : "bg-zinc-600"
                )}
              />
            )}
          </div>
          <p className="text-xs text-muted-foreground mt-0.5">{description}</p>
          {items && items.length > 0 && (
            <div className="flex flex-wrap gap-1.5 mt-2">
              {items.map((item) => (
                <span
                  key={item}
                  className="px-2 py-0.5 rounded-md bg-muted text-xs text-muted-foreground"
                >
                  {item}
                </span>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
```

**Step 2: Create NewTaskPage**

Create `ui/packages/app/src/pages/NewTaskPage.tsx`:

```tsx
import { useState } from "react";
import { useNavigate } from "react-router";
import { useQuery, useMutation } from "@tanstack/react-query";
import {
  Brain,
  Server,
  Puzzle,
  HardDrive,
  FileText,
  Sparkles,
  ArrowRight,
} from "lucide-react";
import { api } from "../api/client";
import { ResourceCard } from "../components/ResourceCard";

export function NewTaskPage() {
  const [prompt, setPrompt] = useState("");
  const navigate = useNavigate();

  const { data: config } = useQuery({
    queryKey: ["config"],
    queryFn: api.config.get,
  });

  const { data: mcpServers = [] } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: api.mcp.servers,
  });

  const createTask = useMutation({
    mutationFn: () => api.tasks.create(prompt),
    onSuccess: (task) => navigate(`/tasks/${task.id}`),
  });

  return (
    <div className="max-w-4xl mx-auto space-y-8">
      {/* Header */}
      <div>
        <h1 className="text-3xl font-bold tracking-tight">New Task</h1>
        <p className="text-muted-foreground mt-1">
          Describe what you'd like the agent to do
        </p>
      </div>

      {/* Prompt area */}
      <div className="relative">
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          placeholder="What would you like me to do?"
          rows={4}
          autoFocus
          className="w-full p-5 rounded-xl bg-card border border-border text-foreground placeholder:text-muted-foreground resize-none focus:outline-none focus:ring-2 focus:ring-primary/30 focus:border-primary/50 transition-all text-base"
        />
        <button
          onClick={() => createTask.mutate()}
          disabled={!prompt.trim() || createTask.isPending}
          className="absolute bottom-4 right-4 flex items-center gap-2 px-4 py-2 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40 transition-all text-sm font-medium"
        >
          {createTask.isPending ? "Starting..." : "Run Task"}
          <ArrowRight className="w-4 h-4" />
        </button>
      </div>

      {/* Resource Panel */}
      <div>
        <h2 className="text-lg font-semibold mb-4 flex items-center gap-2">
          <Sparkles className="w-5 h-5 text-primary" />
          Available Resources
        </h2>
        <p className="text-sm text-muted-foreground mb-4">
          The agent has access to these tools and capabilities for your task
        </p>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          {/* Model */}
          <ResourceCard
            icon={Brain}
            title={config?.agent.model || "claude-sonnet-4-5"}
            description="AI model powering the agent"
            color="text-violet-400"
            status="active"
          />

          {/* Workspace */}
          <ResourceCard
            icon={FileText}
            title={`Workspace: ${config?.agent.default_workspace || "default"}`}
            description="Agent personality, skills, and user preferences"
            items={["SOUL.md", "AGENTS.md", "USER.md"]}
            color="text-amber-400"
            status="active"
          />

          {/* Builtin Tools */}
          <ResourceCard
            icon={HardDrive}
            title="Builtin Tools"
            description="Core tools available to every agent"
            items={["memory.store", "memory.recall"]}
            color="text-cyan-400"
            status="active"
          />

          {/* MCP Servers */}
          {mcpServers.map((server) => (
            <ResourceCard
              key={server.name}
              icon={Server}
              title={server.name}
              description={`MCP server (port ${server.port})`}
              color="text-emerald-400"
              status="active"
            />
          ))}

          {/* Skills placeholder */}
          <ResourceCard
            icon={Puzzle}
            title="Skills"
            description="No skills installed yet"
            color="text-zinc-500"
            status="inactive"
          />
        </div>
      </div>
    </div>
  );
}
```

**Step 3: Wire into App.tsx**

```tsx
import { NewTaskPage } from "./pages/NewTaskPage";

// In Routes:
<Route path="tasks/new" element={<NewTaskPage />} />
```

**Step 4: Verify**

Expected: Beautiful new task page with prompt textarea and visual resource cards showing model, workspace files, builtin tools, and MCP servers.

**Step 5: Commit**

```bash
git add ui/packages/app/src/pages/NewTaskPage.tsx ui/packages/app/src/components/ResourceCard.tsx ui/packages/app/src/App.tsx
git commit -m "feat(ui): visual new task page with resource panel showing skills, MCP, tools"
```

---

### Task 14: Task Detail page — streaming timeline

**Files:**
- Create: `ui/packages/app/src/pages/TaskDetailPage.tsx`
- Create: `ui/packages/app/src/hooks/useTaskStream.ts`
- Create: `ui/packages/app/src/components/StreamEntry.tsx`
- Modify: `ui/packages/app/src/App.tsx`

**Step 1: Create WebSocket hook**

Create `ui/packages/app/src/hooks/useTaskStream.ts`:

```typescript
import { useState, useEffect, useRef } from "react";
import type { StreamChunk } from "../api/types";

export interface StreamEvent {
  type: "thinking" | "tool-start" | "tool-end" | "text" | "done" | "error";
  content: string;
  toolName?: string;
  timestamp: Date;
}

export function useTaskStream(taskId: string | undefined) {
  const [events, setEvents] = useState<StreamEvent[]>([]);
  const [connected, setConnected] = useState(false);
  const [done, setDone] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (!taskId) return;

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${protocol}//${window.location.host}/api/tasks/${taskId}/stream`;
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => setConnected(true);
    ws.onclose = () => {
      setConnected(false);
      setDone(true);
    };
    ws.onmessage = (event) => {
      try {
        const chunk: StreamChunk = JSON.parse(event.data);
        const timestamp = new Date();

        if (chunk === "Done") {
          setEvents((prev) => [...prev, { type: "done", content: "Task complete", timestamp }]);
          setDone(true);
          return;
        }

        if ("TextDelta" in chunk) {
          setEvents((prev) => [...prev, { type: "thinking", content: chunk.TextDelta, timestamp }]);
        } else if ("TextBlock" in chunk) {
          setEvents((prev) => [...prev, { type: "text", content: chunk.TextBlock, timestamp }]);
        } else if ("ToolStart" in chunk) {
          setEvents((prev) => [
            ...prev,
            { type: "tool-start", content: `Calling ${chunk.ToolStart.name}...`, toolName: chunk.ToolStart.name, timestamp },
          ]);
        } else if ("ToolEnd" in chunk) {
          setEvents((prev) => [
            ...prev,
            {
              type: "tool-end",
              content: chunk.ToolEnd.summary || "Done",
              toolName: chunk.ToolEnd.name,
              timestamp,
            },
          ]);
        } else if ("Error" in chunk) {
          setEvents((prev) => [...prev, { type: "error", content: chunk.Error, timestamp }]);
        }
      } catch {
        // ignore parse errors
      }
    };

    return () => {
      ws.close();
    };
  }, [taskId]);

  return { events, connected, done };
}
```

**Step 2: Create StreamEntry component**

Create `ui/packages/app/src/components/StreamEntry.tsx`:

```tsx
import { useState } from "react";
import { Brain, Wrench, MessageSquare, AlertCircle, CheckCircle2, ChevronDown } from "lucide-react";
import { cn } from "../lib/utils";
import type { StreamEvent } from "../hooks/useTaskStream";

const typeConfig = {
  thinking: { icon: Brain, color: "text-violet-400", bg: "bg-violet-500/10", label: "Thinking" },
  "tool-start": { icon: Wrench, color: "text-amber-400", bg: "bg-amber-500/10", label: "Tool Call" },
  "tool-end": { icon: CheckCircle2, color: "text-emerald-400", bg: "bg-emerald-500/10", label: "Tool Result" },
  text: { icon: MessageSquare, color: "text-blue-400", bg: "bg-blue-500/10", label: "Response" },
  done: { icon: CheckCircle2, color: "text-emerald-400", bg: "bg-emerald-500/10", label: "Complete" },
  error: { icon: AlertCircle, color: "text-red-400", bg: "bg-red-500/10", label: "Error" },
};

export function StreamEntry({ event }: { event: StreamEvent }) {
  const [expanded, setExpanded] = useState(event.type === "text" || event.type === "error");
  const { icon: Icon, color, bg, label } = typeConfig[event.type];

  return (
    <div className="group">
      <button
        onClick={() => setExpanded(!expanded)}
        className={cn(
          "w-full flex items-start gap-3 p-3 rounded-lg transition-colors text-left",
          bg,
          "hover:opacity-90"
        )}
      >
        <div className={cn("w-8 h-8 rounded-lg flex items-center justify-center shrink-0", bg)}>
          <Icon className={cn("w-4 h-4", color)} />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className={cn("text-xs font-medium", color)}>{label}</span>
            {event.toolName && (
              <span className="text-xs text-muted-foreground font-mono">{event.toolName}</span>
            )}
            <span className="text-xs text-muted-foreground ml-auto">
              {event.timestamp.toLocaleTimeString()}
            </span>
          </div>
          {expanded && (
            <p className="text-sm text-foreground mt-1.5 whitespace-pre-wrap">{event.content}</p>
          )}
        </div>
        <ChevronDown
          className={cn(
            "w-4 h-4 text-muted-foreground transition-transform shrink-0 mt-1",
            expanded && "rotate-180"
          )}
        />
      </button>
    </div>
  );
}
```

**Step 3: Create TaskDetailPage**

Create `ui/packages/app/src/pages/TaskDetailPage.tsx`:

```tsx
import { useParams, useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, StopCircle } from "lucide-react";
import { api } from "../api/client";
import { useTaskStream } from "../hooks/useTaskStream";
import { StreamEntry } from "../components/StreamEntry";

export function TaskDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { events, connected, done } = useTaskStream(id);

  const { data: task } = useQuery({
    queryKey: ["task", id],
    queryFn: () => api.tasks.get(id!),
    enabled: !!id,
  });

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <button
            onClick={() => navigate("/")}
            className="p-2 rounded-lg hover:bg-muted transition-colors"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h1 className="text-xl font-bold tracking-tight">
              {task?.prompt || "Task"}
            </h1>
            <p className="text-sm text-muted-foreground">
              {id?.slice(0, 8)} &middot;{" "}
              {connected ? (
                <span className="text-emerald-400">Connected</span>
              ) : done ? (
                "Complete"
              ) : (
                "Connecting..."
              )}
            </p>
          </div>
        </div>

        {!done && (
          <button
            onClick={() => id && api.tasks.cancel(id)}
            className="flex items-center gap-2 px-4 py-2 rounded-lg border border-destructive/30 text-destructive hover:bg-destructive/10 transition-colors text-sm"
          >
            <StopCircle className="w-4 h-4" />
            Cancel
          </button>
        )}
      </div>

      {/* Stream timeline */}
      <div className="space-y-2">
        {events.length === 0 && !done && (
          <div className="flex items-center justify-center py-16">
            <div className="flex items-center gap-3 text-muted-foreground">
              <div className="w-2 h-2 rounded-full bg-primary animate-pulse" />
              Waiting for agent...
            </div>
          </div>
        )}
        {events.map((event, i) => (
          <StreamEntry key={i} event={event} />
        ))}
      </div>
    </div>
  );
}
```

**Step 4: Wire into App.tsx**

```tsx
import { TaskDetailPage } from "./pages/TaskDetailPage";

// In Routes:
<Route path="tasks/:id" element={<TaskDetailPage />} />
```

**Step 5: Verify**

Run backend + frontend. Create a task via the New Task page, then click into the detail. Should see streaming events appear in the timeline (mock data from the WebSocket handler).

**Step 6: Commit**

```bash
git add ui/packages/app/src/pages/TaskDetailPage.tsx ui/packages/app/src/hooks/useTaskStream.ts ui/packages/app/src/components/StreamEntry.tsx ui/packages/app/src/App.tsx
git commit -m "feat(ui): task detail page with WebSocket streaming timeline"
```

---

### Task 15: Configuration pages — Workspace, Secrets, Settings

**Files:**
- Create: `ui/packages/app/src/pages/WorkspacePage.tsx`
- Create: `ui/packages/app/src/pages/SecretsPage.tsx`
- Create: `ui/packages/app/src/pages/SettingsPage.tsx`
- Create: `ui/packages/app/src/pages/SkillsPage.tsx`
- Create: `ui/packages/app/src/pages/McpServersPage.tsx`
- Modify: `ui/packages/app/src/App.tsx`

**Step 1: WorkspacePage**

Create `ui/packages/app/src/pages/WorkspacePage.tsx`:

```tsx
import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { FileText, Save } from "lucide-react";
import { api } from "../api/client";
import { cn } from "../lib/utils";

const files = ["SOUL.md", "AGENTS.md", "USER.md"];

export function WorkspacePage() {
  const [selected, setSelected] = useState("SOUL.md");
  const [content, setContent] = useState("");
  const queryClient = useQueryClient();

  const { isLoading } = useQuery({
    queryKey: ["workspace", selected],
    queryFn: () => api.workspace.get(selected),
    select: (data) => {
      setContent(data.content || "");
      return data;
    },
  });

  const save = useMutation({
    mutationFn: () => api.workspace.update(selected, content),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["workspace", selected] }),
  });

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Workspace Files</h1>

      <div className="flex gap-2">
        {files.map((f) => (
          <button
            key={f}
            onClick={() => setSelected(f)}
            className={cn(
              "flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors",
              selected === f
                ? "bg-primary/10 text-primary border border-primary/20"
                : "text-muted-foreground hover:bg-muted"
            )}
          >
            <FileText className="w-4 h-4" />
            {f}
          </button>
        ))}
      </div>

      <div className="relative">
        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          className="w-full h-96 p-4 rounded-xl bg-card border border-border font-mono text-sm resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
          disabled={isLoading}
        />
        <button
          onClick={() => save.mutate()}
          disabled={save.isPending}
          className="absolute top-3 right-3 flex items-center gap-2 px-3 py-1.5 rounded-lg bg-primary/10 text-primary hover:bg-primary/20 text-sm transition-colors"
        >
          <Save className="w-4 h-4" />
          {save.isPending ? "Saving..." : "Save"}
        </button>
      </div>
    </div>
  );
}
```

**Step 2: SecretsPage**

Create `ui/packages/app/src/pages/SecretsPage.tsx`:

```tsx
import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import { api } from "../api/client";

export function SecretsPage() {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const queryClient = useQueryClient();

  const { data: secrets = [] } = useQuery({
    queryKey: ["secrets"],
    queryFn: api.secrets.list,
  });

  const add = useMutation({
    mutationFn: () => api.secrets.add(name, value),
    onSuccess: () => {
      setName("");
      setValue("");
      queryClient.invalidateQueries({ queryKey: ["secrets"] });
    },
  });

  const remove = useMutation({
    mutationFn: (n: string) => api.secrets.delete(n),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["secrets"] }),
  });

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Secrets</h1>
      <p className="text-sm text-muted-foreground">
        Encrypted secrets for API keys. Values are never displayed.
      </p>

      {/* Add form */}
      <div className="flex gap-3">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Secret name (e.g. ANTHROPIC_API_KEY)"
          className="flex-1 px-4 py-2 rounded-lg bg-card border border-border text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
        />
        <input
          type="password"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder="Value"
          className="flex-1 px-4 py-2 rounded-lg bg-card border border-border text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
        />
        <button
          onClick={() => add.mutate()}
          disabled={!name || !value}
          className="px-4 py-2 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40 text-sm"
        >
          <Plus className="w-4 h-4" />
        </button>
      </div>

      {/* List */}
      <div className="space-y-2">
        {secrets.map((s) => (
          <div
            key={s.name}
            className="flex items-center justify-between p-4 rounded-xl bg-card border border-border"
          >
            <div className="flex items-center gap-3">
              <KeyRound className="w-4 h-4 text-amber-400" />
              <span className="text-sm font-mono">{s.name}</span>
            </div>
            <button
              onClick={() => remove.mutate(s.name)}
              className="p-2 rounded-lg text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
            >
              <Trash2 className="w-4 h-4" />
            </button>
          </div>
        ))}
        {secrets.length === 0 && (
          <p className="text-sm text-muted-foreground text-center py-8">No secrets stored</p>
        )}
      </div>
    </div>
  );
}
```

**Step 3: SettingsPage**

Create `ui/packages/app/src/pages/SettingsPage.tsx`:

```tsx
import { useQuery } from "@tanstack/react-query";
import { Settings } from "lucide-react";
import { api } from "../api/client";

export function SettingsPage() {
  const { data: config } = useQuery({
    queryKey: ["config"],
    queryFn: api.config.get,
  });

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Settings</h1>

      <div className="space-y-4">
        <Field label="Model" value={config?.agent.model} />
        <Field label="Max Turns" value={config?.agent.max_turns?.toString()} />
        <Field label="Default Workspace" value={config?.agent.default_workspace} />
        <Field label="Data Directory" value={config?.data_dir} />
        <Field label="AgentGateway URL" value={config?.mcp.agentgateway_url} />
      </div>
    </div>
  );
}

function Field({ label, value }: { label: string; value?: string }) {
  return (
    <div className="p-4 rounded-xl bg-card border border-border">
      <label className="text-xs text-muted-foreground">{label}</label>
      <p className="text-sm font-mono mt-1">{value || "—"}</p>
    </div>
  );
}
```

**Step 4: SkillsPage and McpServersPage (stubs)**

Create `ui/packages/app/src/pages/SkillsPage.tsx`:

```tsx
import { Puzzle } from "lucide-react";

export function SkillsPage() {
  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Skills</h1>
      <div className="flex flex-col items-center justify-center py-16">
        <Puzzle className="w-12 h-12 text-muted-foreground mb-4" />
        <p className="text-muted-foreground">No skills installed</p>
        <p className="text-sm text-muted-foreground mt-1">ClawHub integration coming in Phase 1+</p>
      </div>
    </div>
  );
}
```

Create `ui/packages/app/src/pages/McpServersPage.tsx`:

```tsx
import { useQuery } from "@tanstack/react-query";
import { Server } from "lucide-react";
import { api } from "../api/client";

export function McpServersPage() {
  const { data: servers = [] } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: api.mcp.servers,
  });

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">MCP Servers</h1>

      <div className="space-y-3">
        {servers.map((s) => (
          <div
            key={s.name}
            className="flex items-center justify-between p-4 rounded-xl bg-card border border-border"
          >
            <div className="flex items-center gap-3">
              <Server className="w-5 h-5 text-emerald-400" />
              <div>
                <p className="text-sm font-medium">{s.name}</p>
                <p className="text-xs text-muted-foreground font-mono">{s.image}</p>
              </div>
            </div>
            <span className="text-xs text-muted-foreground">:{s.port}</span>
          </div>
        ))}
        {servers.length === 0 && (
          <p className="text-sm text-muted-foreground text-center py-8">No MCP servers configured</p>
        )}
      </div>
    </div>
  );
}
```

**Step 5: Wire all pages into App.tsx**

```tsx
import { WorkspacePage } from "./pages/WorkspacePage";
import { SkillsPage } from "./pages/SkillsPage";
import { McpServersPage } from "./pages/McpServersPage";
import { SecretsPage } from "./pages/SecretsPage";
import { SettingsPage } from "./pages/SettingsPage";

// Replace config placeholders:
<Route path="config/workspace" element={<WorkspacePage />} />
<Route path="config/skills" element={<SkillsPage />} />
<Route path="config/mcp" element={<McpServersPage />} />
<Route path="config/secrets" element={<SecretsPage />} />
<Route path="config/settings" element={<SettingsPage />} />
```

**Step 6: Verify**

All 5 configuration pages render correctly with data from the API.

**Step 7: Commit**

```bash
git add ui/packages/app/src/pages/
git commit -m "feat(ui): configuration pages — workspace, skills, mcp, secrets, settings"
```

---

## Part D: Integration & Polish

### Task 16: Wire App.tsx with all real page imports

**Files:**
- Modify: `ui/packages/app/src/App.tsx`

**Step 1: Final App.tsx**

Ensure all placeholder routes are replaced with real page components. Remove the `Placeholder` function. The final `App.tsx` should import all pages and wire them to their routes. Remove any temporary `isAuthenticated = true` overrides.

**Step 2: Verify full flow**

1. Start backend: `cargo run -p mcclawd-api -- serve`
2. Start frontend: `cd ui && pnpm dev`
3. Navigate to `http://localhost:8080`
4. Should redirect to `/login`
5. Enter any password → unlock → redirects to Tasks page
6. Click "New Task" → see visual resource panel
7. Enter a prompt, click "Run Task" → navigates to task detail with streaming timeline
8. Check all config pages via sidebar

**Step 3: Commit**

```bash
git add ui/packages/app/src/App.tsx
git commit -m "feat(ui): wire all pages into router, remove placeholders"
```

---

### Task 17: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

**Step 1: Add UI section to CLAUDE.md**

Add after the "Run" section:

```markdown
## UI Development

```bash
cd ui && pnpm install                   # install frontend deps
cd ui && pnpm dev                       # start Vite dev server (:8080)
cargo run -p mcclawd-api -- serve       # start Axum API server (:9090)
```

The Vite dev server proxies `/api` requests to the Axum backend.

### UI Tech Stack
- React 19 + TypeScript + Vite + Tailwind CSS + shadcn/ui
- Located in `ui/packages/app/`
- API client: `ui/packages/app/src/api/client.ts`
- Pages: `ui/packages/app/src/pages/`
```

**Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add UI development section to CLAUDE.md"
```

---

## Summary

| Task | Description | Part |
|------|-------------|------|
| 1 | Add Axum dependencies | Backend |
| 2 | AppState + server skeleton + `mc serve` | Backend |
| 3 | Auth endpoint + JWT middleware | Backend |
| 4 | Tasks CRUD endpoints | Backend |
| 5 | Workspace, Secrets, Config, MCP endpoints | Backend |
| 6 | WebSocket streaming endpoint | Backend |
| 7 | Scaffold Vite + pnpm workspace | Frontend |
| 8 | shadcn/ui setup + dark theme | Frontend |
| 9 | API client + auth context | Frontend |
| 10 | Router + Layout with sidebar | Frontend |
| 11 | Login page — beautiful, minimal | Pages |
| 12 | Tasks home page — graphical dashboard | Pages |
| 13 | New Task page — visual resource panel | Pages |
| 14 | Task Detail page — streaming timeline | Pages |
| 15 | Config pages (workspace, secrets, settings, skills, mcp) | Pages |
| 16 | Wire all pages, remove placeholders | Integration |
| 17 | Update CLAUDE.md | Docs |
