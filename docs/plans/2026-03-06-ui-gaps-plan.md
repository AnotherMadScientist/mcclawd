# McClawd UI Gaps Implementation Plan

**Date:** 2026-03-06
**Author:** Planning Agent
**Status:** Draft

---

## Summary

Six UI feature gaps identified in the McClawd React frontend. This plan covers exact files to modify, backend API changes needed, component designs, and Playwright test cases for each gap. Gaps are ordered by priority.

---

## Gap 1: Settings Page is Read-Only

**Priority:** P0 | **Complexity:** S | **Effort:** ~2 hours

### Current State

- `SettingsPage.tsx` uses `useQuery` to fetch config via `api.config.get()` (`GET /api/config`)
- Renders five read-only `<Field>` components (Model, Max Turns, Default Workspace, Data Directory, AgentGateway URL)
- `api.config.update()` exists in `client.ts` calling `PUT /api/config`
- Backend `put_config()` in `config_routes.rs` returns `StatusCode::NOT_IMPLEMENTED`

### Implementation

#### Backend (Rust)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/config_routes.rs`

- Implement `put_config()` handler:
  - Accept `Json<PartialConfigUpdate>` (new struct with `Option<>` fields for each editable value)
  - Validate: `max_turns > 0`, `model` is non-empty, `default_workspace` is non-empty
  - Acquire write lock on `state.config`, apply updates, write back to config file
  - Return `200 OK` with updated config
  - Data Directory and AgentGateway URL should remain read-only (not in `PartialConfigUpdate`)

```rust
#[derive(Debug, Deserialize)]
pub struct PartialConfigUpdate {
    pub model: Option<String>,
    pub max_turns: Option<usize>,
    pub default_workspace: Option<String>,
}
```

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/config.rs`

- Add `McclawdConfig::save(&self, path: &Path) -> Result<()>` method (serialize to TOML/JSON, atomic write)
- Store config file path in `McclawdConfig` or pass via `AppState`

#### Frontend (React)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/pages/SettingsPage.tsx`

- Replace `<Field>` with `<EditableField>` for editable fields (Model, Max Turns, Default Workspace)
- Keep Data Directory and AgentGateway URL as read-only `<Field>`
- `<EditableField>` component:
  - Displays value with a pencil icon button
  - Click toggles inline `<input>` with save/cancel buttons
  - Save calls `useMutation` with `api.config.update()`
  - On success: invalidate `["config"]` query, show success toast
  - On error: show error toast, revert to previous value
  - Model field: use `<select>` dropdown with known models (claude-sonnet-4-20250514, claude-opus-4-20250514, claude-haiku-4-20250514)
  - Max Turns field: `<input type="number" min="1" max="100">`
  - Default Workspace field: `<input type="text">`

#### Tests

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/tests/settings.spec.ts` (extend existing)

| Test Name | What It Verifies |
|-----------|-----------------|
| `can edit Model field` | Click edit on Model, select new value, save, verify new value displayed |
| `can edit Max Turns field` | Click edit, type new number, save, verify update |
| `can edit Default Workspace field` | Click edit, type new workspace name, save, verify |
| `edit persists after page reload` | Edit Model, reload page, verify saved value persists |
| `cancel edit reverts value` | Click edit, change value, click cancel, verify original value |
| `Data Directory is not editable` | Verify no edit button on Data Directory field |
| `AgentGateway URL is not editable` | Verify no edit button on AgentGateway URL field |
| `invalid Max Turns shows error` | Try to save 0 or negative, verify error message |

---

## Gap 2: NewTaskPage Lacks Model/Skill/Workspace Selectors

**Priority:** P0 | **Complexity:** M | **Effort:** ~4 hours

### Current State

- `NewTaskPage.tsx` has prompt textarea, file attach, mic button, and resource cards (display-only)
- `api.tasks.create(prompt, workspace?, model?, delayStart?)` already accepts `model` and `workspace` params
- Backend `CreateTaskRequest` has `prompt`, `workspace`, `model`, `delay_start` fields
- No `skills` field in `CreateTaskRequest` -- skills are loaded from installed skills at agent startup
- Resource cards show system status (LLM health, MCP servers, skills count, workspace) but are not interactive

### Implementation

#### Backend (Rust)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/tasks.rs`

- Add `skills: Option<Vec<String>>` to `CreateTaskRequest`
- When skills are specified, pass them to the agent engine (filter which installed skills to load)
- Add `GET /api/workspaces` endpoint (list available workspace directories)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/routes.rs`

- Add route: `.route("/api/workspaces", get(workspace::list_workspaces))`

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/workspace.rs`

- Add `list_workspaces()` handler: scan `data_dir/workspaces/` for subdirectories

#### Frontend (React)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/pages/NewTaskPage.tsx`

Add a collapsible "Advanced Options" panel below the prompt textarea:

1. **Model Dropdown** (`<select>`)
   - Default from `config.agent.model`
   - Options: fetch from config or hardcode known Anthropic models
   - State: `const [selectedModel, setSelectedModel] = useState<string | undefined>()`

2. **Skills Multi-Select** (checkbox list or multi-select dropdown)
   - Fetch from `api.skills.list()` (already queried for resource card)
   - Each skill has a checkbox; default: all enabled
   - State: `const [selectedSkills, setSelectedSkills] = useState<string[]>([])`

3. **Workspace Selector** (`<select>`)
   - Default from `config.agent.default_workspace`
   - Options: "default" plus any additional workspaces from `GET /api/workspaces`
   - State: `const [selectedWorkspace, setSelectedWorkspace] = useState<string | undefined>()`

4. **Wire into createTask mutation:**
   ```typescript
   const task = await api.tasks.create(
     prompt,
     selectedWorkspace,
     selectedModel,
     hasFiles
   );
   ```

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/api/client.ts`

- Update `api.tasks.create()` signature to include `skills?: string[]`
- Add `api.workspaces.list()` method

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/api/types.ts`

- Add `Workspace` type if needed

#### Tests

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/tests/new-task.spec.ts` (extend existing)

| Test Name | What It Verifies |
|-----------|-----------------|
| `shows Advanced Options panel` | Verify collapsible panel exists with Model, Skills, Workspace selectors |
| `model dropdown defaults to config value` | Verify dropdown shows configured model |
| `can select different model` | Select alternate model, verify selection |
| `skills multi-select lists installed skills` | Verify installed skills appear as options |
| `can toggle skill selection` | Uncheck a skill, verify deselected |
| `workspace selector defaults to config value` | Verify "default" workspace selected |
| `task creation sends selected model` | Intercept API call, verify model param sent |
| `task creation sends selected workspace` | Intercept API call, verify workspace param sent |

---

## Gap 3: MCP Server Management Missing

**Priority:** P1 | **Complexity:** L | **Effort:** ~6 hours

### Current State

- `McpServersPage.tsx` is read-only: lists servers from `api.mcp.servers()` (`GET /api/mcp/servers`)
- Backend `mcp_routes.rs` has only `list_mcp_servers()` -- no add/remove/restart
- `McpServerConfig` struct: `name`, `image`, `port`, `env: Vec<String>`, `volumes: Vec<String>`
- MCP servers run as Docker containers managed by `docker compose`

### Implementation

#### Backend (Rust)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/mcp_routes.rs`

Add three new handlers:

1. **`add_mcp_server()`** -- `POST /api/mcp/servers`
   - Accept `Json<McpServerConfig>`
   - Validate: name unique, image non-empty, port not in use
   - Acquire write lock on `state.config`, push to `mcp.servers`, save config
   - Do NOT auto-start the container (user must `docker compose up`)
   - Return `201 Created` with new server config

2. **`remove_mcp_server()`** -- `DELETE /api/mcp/servers/{name}`
   - Remove server from config by name
   - Save config
   - Return `204 No Content`

3. **`restart_mcp_server()`** -- `POST /api/mcp/servers/{name}/restart`
   - Execute `docker compose restart {name}` via `tokio::process::Command`
   - Return `200 OK` with status message
   - This is a convenience action; full docker management stays with docker compose

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/routes.rs`

Add routes:
```rust
.route("/api/mcp/servers", get(mcp_routes::list_mcp_servers).post(mcp_routes::add_mcp_server))
.route("/api/mcp/servers/{name}", delete(mcp_routes::remove_mcp_server))
.route("/api/mcp/servers/{name}/restart", post(mcp_routes::restart_mcp_server))
```

#### Frontend (React)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/pages/McpServersPage.tsx`

1. **Add Server Form** (collapsible panel at top):
   - Fields: Name (text), Image (text), Port (number), Env Vars (tag input), Volumes (tag input)
   - "Add Server" button calls `useMutation` with `api.mcp.add()`
   - On success: invalidate `["mcp-servers"]` query, clear form, show toast

2. **Server Card Actions**:
   - Add "Restart" button (circular arrow icon) per server card
   - Add "Remove" button (trash icon) per server card with confirmation dialog
   - Restart calls `api.mcp.restart(name)`
   - Remove calls `api.mcp.remove(name)` after confirmation

3. **Server Status Indicator**:
   - Optional: ping server URL to show green/red dot (stretch goal)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/api/client.ts`

Add to `api.mcp`:
```typescript
mcp: {
  servers: () => apiFetch<McpServer[]>("/api/mcp/servers"),
  add: (server: McpServer) => apiFetch<McpServer>("/api/mcp/servers", {
    method: "POST", body: JSON.stringify(server),
  }),
  remove: (name: string) => apiFetch<void>(`/api/mcp/servers/${name}`, { method: "DELETE" }),
  restart: (name: string) => apiFetch<void>(`/api/mcp/servers/${name}/restart`, { method: "POST" }),
},
```

#### Tests

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/tests/mcp-servers.spec.ts` (extend existing)

| Test Name | What It Verifies |
|-----------|-----------------|
| `shows Add Server form` | Verify form appears with name, image, port fields |
| `can add a new MCP server` | Fill form, submit, verify new server in list |
| `validates required fields on add` | Submit empty form, verify validation errors |
| `shows remove button per server` | Verify trash icon on each server card |
| `can remove an MCP server` | Click remove, confirm dialog, verify removed from list |
| `shows restart button per server` | Verify restart icon on each server card |
| `can restart an MCP server` | Click restart, verify success toast |
| `duplicate name shows error` | Try to add server with existing name, verify error |

---

## Gap 4: Create Skill Doesn't Save

**Priority:** P1 | **Complexity:** M | **Effort:** ~3 hours

### Current State

- `SkillsPage.tsx` has a `CreateSkillDialog` with a textarea for SKILL.md content
- Dialog has real-time preview with `parseSkillSections()` color bars
- No save/persist functionality -- dialog is display-only
- Backend has `POST /api/skills/install` for ClawHub installs but no endpoint for creating local skills
- Skills are stored at `~/.mcclawd/skills/{skill-name}/SKILL.md`

### Implementation

#### Backend (Rust)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/skills_routes.rs`

Add `create_local_skill()` handler:

```rust
#[derive(Debug, Deserialize)]
pub struct CreateSkillRequest {
    pub name: String,           // skill folder name (slugified)
    pub content: String,        // raw SKILL.md content
}
```

- `POST /api/skills` (new route, distinct from `/api/skills/install`)
- Validate: name is slug-safe (alphanumeric + hyphens), content non-empty, no existing skill with same name
- Create directory `{managed_dir}/{name}/`
- Write `SKILL.md` to that directory
- Create `.installed.json` metadata file (source: "Local", version: "0.1.0")
- Return `201 Created` with `InstalledSkillInfo`

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/routes.rs`

Update skills route:
```rust
.route("/api/skills", get(skills_routes::list_installed).post(skills_routes::create_local_skill))
```

#### Frontend (React)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/pages/SkillsPage.tsx`

Modify `CreateSkillDialog`:

1. Add skill name input field (auto-slugified from first heading or manual entry)
2. Add "Save Skill" button at bottom of dialog
3. Save button:
   - Extracts skill name from frontmatter or input field
   - Calls `useMutation` with `api.skills.create(name, content)`
   - On success: invalidate `["skills"]` query, close dialog, show success toast
   - On error: show error toast with backend message
4. Add validation: name required, content must have at least `# Title` line

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/api/client.ts`

Add to `api.skills`:
```typescript
create: (name: string, content: string) =>
  apiFetch<InstalledSkill>("/api/skills", {
    method: "POST",
    body: JSON.stringify({ name, content }),
  }),
```

#### Tests

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/tests/skills.spec.ts` (extend existing)

| Test Name | What It Verifies |
|-----------|-----------------|
| `Create dialog has Save button` | Verify Save Skill button exists |
| `Create dialog has skill name input` | Verify name input field exists |
| `can create a local skill` | Fill name + content, save, verify appears in installed list |
| `created skill content persists` | Create skill, open detail dialog, verify content matches |
| `duplicate name shows error` | Try to create skill with existing name, verify error |
| `empty content shows validation error` | Try to save with empty textarea, verify error |

---

## Gap 5: Task Filtering/Search Missing

**Priority:** P1 | **Complexity:** M | **Effort:** ~3 hours

### Current State

- `TasksPage.tsx` shows all tasks with stats row (Running, Completed, Failed counts)
- Already computes `running`, `completed`, `failed` arrays via `.filter()` client-side
- No search input or filter controls
- `GET /api/tasks` returns all tasks, no query params

### Implementation

#### Backend (Rust)

No backend changes required for Phase 1 -- client-side filtering is sufficient given task volumes are low (dozens, not thousands). Future optimization: add `?status=Running&q=search` query params to `GET /api/tasks`.

#### Frontend (React)

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/pages/TasksPage.tsx`

1. **Search Input** (above task list):
   - Text input with search icon, placeholder "Search tasks..."
   - Filters tasks by prompt text (case-insensitive `includes()`)
   - State: `const [searchQuery, setSearchQuery] = useState("")`
   - Debounce 300ms for smooth typing

2. **Status Filter Tabs** (below stats row):
   - Clickable stat cards: "All", "Running", "Completed", "Failed"
   - Active tab highlighted with primary color
   - State: `const [statusFilter, setStatusFilter] = useState<"all" | "Running" | "Completed" | "Failed">("all")`

3. **Combined Filtering Logic**:
   ```typescript
   const filteredTasks = useMemo(() => {
     let result = tasks;
     if (statusFilter !== "all") {
       result = result.filter(t => {
         if (statusFilter === "Failed") return typeof t.status === "object" && "Failed" in t.status;
         return t.status === statusFilter;
       });
     }
     if (searchQuery.trim()) {
       const q = searchQuery.toLowerCase();
       result = result.filter(t => t.prompt.toLowerCase().includes(q));
     }
     return result;
   }, [tasks, statusFilter, searchQuery]);
   ```

4. **Empty State for Filters**:
   - When filtered results are empty: "No tasks match your filters" with clear filters button

5. **Stats Row as Filter Tabs**:
   - Make existing stat cards clickable (toggle `statusFilter`)
   - Add ring/border highlight on active filter card
   - Add "All" card showing total count

#### Tests

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/tests/tasks.spec.ts` (extend existing)

| Test Name | What It Verifies |
|-----------|-----------------|
| `shows search input` | Verify search input with placeholder exists |
| `search filters tasks by prompt text` | Type query, verify only matching tasks shown |
| `search is case-insensitive` | Type lowercase query, verify matches regardless of case |
| `status filter shows only Running tasks` | Click Running stat card, verify only running tasks |
| `status filter shows only Completed tasks` | Click Completed, verify |
| `status filter shows only Failed tasks` | Click Failed, verify |
| `All filter shows all tasks` | Click All, verify all tasks visible |
| `combined search and status filter works` | Apply both, verify intersection |
| `empty filter state shows message` | Filter to impossible combo, verify empty state message |
| `clear filters button resets view` | Apply filters, click clear, verify all tasks shown |

---

## Gap 6: No Error/Loading States on Several Pages

**Priority:** P2 | **Complexity:** M | **Effort:** ~4 hours

### Current State

- Pages use `useQuery` but most don't handle `isLoading` or `isError` states
- `SettingsPage` renders nothing (undefined values) while loading
- `McpServersPage` shows "No MCP servers configured" while loading (misleading)
- `TasksPage` has `isLoading` variable but only uses it to show "No tasks yet"
- No global error boundary
- No loading skeleton components

### Implementation

#### Frontend (React)

**File (NEW):** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/components/LoadingSkeleton.tsx`

Reusable skeleton components:
```typescript
export function CardSkeleton() { /* animate-pulse rounded bg-muted */ }
export function ListSkeleton({ count = 3 }) { /* N CardSkeletons */ }
export function FieldSkeleton() { /* label + value placeholder */ }
export function PageSkeleton({ title }: { title: string }) { /* heading + ListSkeleton */ }
```

**File (NEW):** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/components/ErrorState.tsx`

Reusable error display:
```typescript
export function ErrorState({ message, onRetry }: { message: string; onRetry?: () => void }) {
  // AlertTriangle icon + message + optional "Retry" button
}
```

**File (NEW):** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/components/ErrorBoundary.tsx`

React error boundary wrapper:
```typescript
export class ErrorBoundary extends React.Component<Props, State> {
  // Catches render errors, shows ErrorState with "Reload" button
}
```

**Files to modify (add loading/error handling):**

| Page File | Loading State | Error State |
|-----------|--------------|-------------|
| `SettingsPage.tsx` | `<FieldSkeleton>` x5 | ErrorState with retry (refetch) |
| `McpServersPage.tsx` | `<ListSkeleton count={3}>` | ErrorState with retry |
| `TasksPage.tsx` | `<PageSkeleton title="Tasks">` | ErrorState with retry |
| `SkillsPage.tsx` | `<ListSkeleton>` for browse grid | ErrorState per section |
| `NewTaskPage.tsx` | Skeleton for resource cards | Inline error for LLM health |
| `WorkspacePage.tsx` | `<FieldSkeleton>` for file editors | ErrorState with retry |

**File:** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/App.tsx` (or Layout.tsx)

- Wrap main content area in `<ErrorBoundary>`

**Pattern for each page:**
```typescript
const { data, isLoading, isError, error, refetch } = useQuery({...});

if (isLoading) return <PageSkeleton title="Settings" />;
if (isError) return <ErrorState message={error.message} onRetry={refetch} />;
```

#### Tests

**File (NEW):** `/Users/velniukas/dev/macleodlabs/mcclawd/ui/tests/error-states.spec.ts`

| Test Name | What It Verifies |
|-----------|-----------------|
| `Settings page shows loading skeleton` | Mock slow API, verify skeleton visible then content |
| `Settings page shows error on API failure` | Mock 500 response, verify error message + retry button |
| `Settings retry button refetches` | Click retry, verify API called again |
| `MCP page shows loading skeleton` | Mock slow API, verify skeleton |
| `MCP page shows error on API failure` | Mock 500, verify error UI |
| `Tasks page shows loading skeleton` | Mock slow API, verify skeleton |
| `Tasks page shows error on API failure` | Mock 500, verify error UI |
| `error boundary catches render errors` | Inject component error, verify boundary UI |
| `error boundary shows reload button` | After error, verify reload button exists |

**Note:** These tests require Playwright route interception to mock API failures:
```typescript
await page.route("/api/config", (route) =>
  route.fulfill({ status: 500, body: "Internal Server Error" })
);
```

---

## Implementation Order

```
Week 1:
  [P0] Gap 1: Settings Page Editing          (S, ~2h)
  [P0] Gap 2: NewTaskPage Selectors          (M, ~4h)

Week 2:
  [P1] Gap 4: Create Skill Save              (M, ~3h)
  [P1] Gap 5: Task Filtering/Search          (M, ~3h)
  [P1] Gap 3: MCP Server Management          (L, ~6h)

Week 3:
  [P2] Gap 6: Error/Loading States           (M, ~4h)
```

**Total estimated effort:** ~22 hours

---

## File Change Summary

### New Files (3)
| File | Purpose |
|------|---------|
| `ui/packages/app/src/components/LoadingSkeleton.tsx` | Reusable loading skeleton components |
| `ui/packages/app/src/components/ErrorState.tsx` | Reusable error display with retry |
| `ui/packages/app/src/components/ErrorBoundary.tsx` | React error boundary wrapper |

### Modified Backend Files (4)
| File | Changes |
|------|---------|
| `crates/mcclawd-api/src/server/config_routes.rs` | Implement `put_config()` with `PartialConfigUpdate` |
| `crates/mcclawd-api/src/server/mcp_routes.rs` | Add `add_mcp_server()`, `remove_mcp_server()`, `restart_mcp_server()` |
| `crates/mcclawd-api/src/server/skills_routes.rs` | Add `create_local_skill()` handler |
| `crates/mcclawd-api/src/server/routes.rs` | Add new routes for MCP CRUD, skill create, workspaces |

### Modified Frontend Files (7)
| File | Changes |
|------|---------|
| `ui/packages/app/src/pages/SettingsPage.tsx` | Add inline editing for Model, Max Turns, Default Workspace |
| `ui/packages/app/src/pages/NewTaskPage.tsx` | Add Advanced Options panel with model/skills/workspace selectors |
| `ui/packages/app/src/pages/McpServersPage.tsx` | Add server form, remove/restart buttons |
| `ui/packages/app/src/pages/SkillsPage.tsx` | Add save functionality to CreateSkillDialog |
| `ui/packages/app/src/pages/TasksPage.tsx` | Add search input and status filter tabs |
| `ui/packages/app/src/api/client.ts` | Add `mcp.add/remove/restart`, `skills.create`, `workspaces.list` |
| `ui/packages/app/src/api/types.ts` | Add types if needed |

### New/Extended Test Files (2 new, 5 extended)
| File | Test Count |
|------|-----------|
| `ui/tests/error-states.spec.ts` (NEW) | 9 tests |
| `ui/tests/settings.spec.ts` (extend) | +8 tests |
| `ui/tests/new-task.spec.ts` (extend) | +8 tests |
| `ui/tests/mcp-servers.spec.ts` (extend) | +8 tests |
| `ui/tests/skills.spec.ts` (extend) | +6 tests |
| `ui/tests/tasks.spec.ts` (extend) | +10 tests |

**Total new tests:** ~49

---

## Dependencies & Risks

| Risk | Mitigation |
|------|-----------|
| `PUT /api/config` needs config file path in AppState | Add `config_path: PathBuf` to AppState during server init |
| MCP server restart requires Docker access from API server | Gate behind feature flag; return 501 if Docker not available |
| Config save race conditions | Use write lock + atomic file write (write to `.tmp`, rename) |
| No `POST /api/skills` route exists | New route; ensure it doesn't conflict with `/api/skills/install` |
| Task filtering client-side may lag with 1000+ tasks | Acceptable for Phase 1; add server-side `?status=&q=` later |
| Error boundary catches too broadly | Scope to page content area, not sidebar/nav |

---

## Verification Checklist

Before marking each gap as complete:

- [ ] Rust backend compiles: `cargo build --workspace`
- [ ] Rust tests pass: `cargo test --workspace`
- [ ] UI builds: `cd ui && pnpm build`
- [ ] Existing E2E tests pass: `cd ui && pnpm exec playwright test`
- [ ] New E2E tests pass for the gap
- [ ] Manual smoke test: `make dev`, navigate to page, test feature
- [ ] No console errors in browser DevTools
