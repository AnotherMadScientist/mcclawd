# McClawd UI Design

**Date:** 2026-03-04
**Status:** Approved

## Goal

Add a web-based dashboard to McClawd for task execution, monitoring, and configuration. TypeScript/React frontend in a monorepo alongside the existing Rust workspace, backed by a new Axum HTTP/WebSocket API.

## Architecture

```
Browser (React + Vite + Tailwind)
  │
  │ REST + WebSocket
  ▼
Axum Server (:9090)            ← new routes in mcclawd-api
  │
  ├── POST /api/auth/login      → SecretBackend (unlock vault)
  ├── GET/POST /api/tasks       → TaskManager
  ├── WS /api/tasks/:id/stream  → Channel (streaming chunks)
  ├── GET/PUT /api/workspace/*  → Workspace files
  ├── GET/POST/DELETE /api/secrets → SecretBackend
  ├── GET/PUT /api/config       → McclawdConfig
  └── GET /api/mcp/servers      → MCP server list
```

## Monorepo Layout

```
mcclawd/
├── crates/                     # Rust workspace (existing)
│   └── mcclawd-api/            # mc binary + new Axum server
├── ui/                         # NEW — TypeScript/React
│   ├── pnpm-workspace.yaml
│   ├── package.json
│   └── packages/
│       └── app/                # Vite React app
│           ├── src/
│           │   ├── pages/      # Route-level components
│           │   ├── components/ # Shared UI components
│           │   ├── hooks/      # useWebSocket, useApi, etc.
│           │   └── api/        # Typed fetch/WS clients
│           ├── tailwind.config.ts
│           └── vite.config.ts
└── docker-compose.yml
```

## Tech Stack

| Layer | Choice | Rationale |
|-------|--------|-----------|
| Frontend | React 19 + TypeScript | Industry standard, large ecosystem |
| Build | Vite | Fast dev server, simple config |
| Monorepo | pnpm workspaces | Lightweight, fast installs |
| Styling | Tailwind CSS + shadcn/ui | Utility-first, polished components, dark mode |
| Data fetching | React Query (TanStack) | Caching, refetching, loading states |
| Routing | React Router v7 | Standard, nested routes |
| WebSocket | Native WebSocket hook | No extra library needed |
| Backend API | Axum (Rust) | Already in the Rust workspace, async, fast |
| State | React Query + URL state | No global state manager needed for Phase 1 |

## Pages & Navigation

Left sidebar with sections and sub-menus. Dark theme default.

### 1. Login Page (`/login`)

Beautiful, minimal full-screen page:
- Dark gradient background with subtle animated particles/glow
- Single circular McClawd logo (large, centered, with soft glow ring)
- Password field below the logo — minimal, borderless, with subtle underline
- No user accounts — single-user tool, the password is the secrets master key
- After login, JWT stored in memory (not localStorage)
- Smooth transition/fade to Tasks page on success

### 2. Tasks Page (`/` — home)

Beautiful graphical dashboard:
- Hero section with greeting + quick stats (running tasks, completed today, available tools)
- **Running Tasks** — large animated cards with pulsing status indicator, model badge, elapsed time
- **Recent Tasks** — clean list with status icons, duration, token count
- **Scheduled Tasks** — placeholder section for Phase 2 (subtle "coming soon" styling)
- "New Task" prominent CTA button — opens the New Task page

### 3. New Task Page (`/tasks/new`)

Visual, layman-friendly task creation:
- Large prompt textarea with placeholder guidance ("What would you like me to do?")
- **Resource Panel** — graphical cards showing what the agent has access to:
  - **Skills** — icons + names of installed SKILL.md files with descriptions
  - **MCP Servers** — status badges (green=running, gray=stopped), tool names exposed by each
  - **Builtin Tools** — memory.store, memory.recall shown as capability cards
  - **Model** — dropdown with model description/capability summary
  - **Workspace** — which workspace files (SOUL/AGENTS/USER) are loaded
- Each resource card is clickable for a tooltip/popover with details
- "Run Task" button starts execution and navigates to Task Detail

### 4. Task Detail Page (`/tasks/:id`)

Streaming terminal-like view with structured timeline:

- `Thinking...` — agent reasoning (collapsible, subtle animation)
- `Tool: memory.store(...)` → result (expandable card with tool icon)
- `Tool: mcp.langextract(...)` → result (expandable card with tool icon)
- `Response: ...` — final answer (full display, markdown rendered)

Each entry is an expandable card (collapsed by default). Cancel button sends interrupt. Metadata sidebar shows: model, workspace, start time, token usage, resources used.

### 4. Configuration Section (sidebar sub-navigation)

Configuration is NOT tabs — each sub-item is its own page via sidebar sub-menu:

**Sidebar structure:**
```
Tasks          ← /
Configuration
  ├── Workspace    ← /config/workspace
  ├── Skills       ← /config/skills
  ├── MCP Servers  ← /config/mcp
  ├── Secrets      ← /config/secrets
  └── Settings     ← /config/settings
```

#### 4a. Workspace (`/config/workspace`)
- List and edit workspace markdown files: SOUL.md, AGENTS.md, USER.md
- Inline markdown editor with preview
- Save button persists to disk

#### 4b. Skills (`/config/skills`)
- List installed SKILL.md files
- Download from ClawHub (Phase 1+ — placeholder UI for now)
- Each skill shows: name, description, tools provided

#### 4c. MCP Servers (`/config/mcp`)
- List configured MCP servers from config
- Show status (running/stopped) from Docker
- Add/remove servers (updates config + docker-compose)

#### 4d. Secrets (`/config/secrets`)
- Masked list of stored secret names (values never shown)
- Add new secret (name + value input)
- Delete existing secret
- No edit — delete and re-add

#### 4e. Settings (`/config/settings`)
- Form fields for McclawdConfig: model, max_turns, data_dir
- Provider configuration: Anthropic, OpenAI, Ollama settings
- Save persists to config.toml

## REST API

| Method | Path | Request | Response |
|--------|------|---------|----------|
| `POST` | `/api/auth/login` | `{ password }` | `{ token }` (JWT) |
| `GET` | `/api/tasks` | — | `[Task]` |
| `POST` | `/api/tasks` | `{ prompt, workspace?, model? }` | `Task` (created) |
| `GET` | `/api/tasks/:id` | — | `Task` (with history) |
| `DELETE` | `/api/tasks/:id` | — | `204` (cancel) |
| `GET` | `/api/workspace` | — | `[{ name, path }]` |
| `GET` | `/api/workspace/:file` | — | `{ content }` (markdown) |
| `PUT` | `/api/workspace/:file` | `{ content }` | `204` |
| `GET` | `/api/secrets` | — | `[{ name }]` (names only) |
| `POST` | `/api/secrets` | `{ name, value }` | `201` |
| `DELETE` | `/api/secrets/:name` | — | `204` |
| `GET` | `/api/config` | — | `McclawdConfig` (JSON) |
| `PUT` | `/api/config` | `McclawdConfig` (partial) | `204` |
| `GET` | `/api/mcp/servers` | — | `[McpServerConfig]` |

## WebSocket API

| Path | Direction | Message Types |
|------|-----------|---------------|
| `WS /api/tasks/:id/stream` | Server → Client | `TextDelta`, `TextBlock`, `ToolStart`, `ToolEnd`, `Done`, `Error` |

Messages match the existing `ChannelChunk` enum from `mcclawd-channels`. The WebSocket streams in real-time as the agent executes.

## Component Tree

```
App
├── LoginPage
├── Layout
│   ├── Sidebar
│   │   ├── NavItem (Tasks)
│   │   └── NavSection (Configuration)
│   │       ├── NavItem (Workspace)
│   │       ├── NavItem (Skills)
│   │       ├── NavItem (MCP Servers)
│   │       ├── NavItem (Secrets)
│   │       └── NavItem (Settings)
│   └── Content (outlet)
│       ├── TasksPage
│       │   ├── TaskCard
│       │   └── NewTaskDialog
│       ├── TaskDetailPage
│       │   ├── StreamTimeline
│       │   │   ├── ThinkingEntry
│       │   │   ├── ToolCallEntry
│       │   │   └── ResponseEntry
│       │   └── TaskMetaSidebar
│       ├── WorkspacePage
│       │   └── MarkdownEditor
│       ├── SkillsPage
│       │   └── SkillCard
│       ├── McpServersPage
│       │   └── McpServerCard
│       ├── SecretsPage
│       │   └── SecretRow
│       └── SettingsPage
│           └── SettingsForm
```

## Security

- JWT required for all API calls (except `/api/auth/login`)
- JWT stored in memory only (not localStorage/cookies)
- Passed via `Authorization: Bearer <token>` header
- Secrets values never returned by API — only names
- WebSocket authenticated via token query param
- CORS restricted to UI origin in production

## Phase Scope

**Phase 1 (this design):**
- Login, Tasks, Task Detail (streaming), Configuration pages
- Axum API with all endpoints above
- WebSocket streaming for task execution
- Basic CRUD for workspace files, secrets, config

**Phase 2+ (future):**
- Scheduled/queued tasks
- ClawHub skill marketplace integration
- Multi-workspace management
- Agent swarm visualization
- Telegram/Discord/Slack channel management
