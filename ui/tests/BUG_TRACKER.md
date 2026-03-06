# McClawd Bug Tracker

> Bugs discovered during E2E testing. Last updated: 2026-03-06.

## Summary

| Severity | Open | Fixed | Won't Fix |
|----------|------|-------|-----------|
| Critical | 0 | 2 | 0 |
| Major | 6 | 15 | 0 |
| Minor | 1 | 7 | 0 |

## Bugs

### BUG-001: Mic button aria-label mismatch in file-upload test
- **Severity:** minor
- **Status:** fixed
- **Page:** /tasks/new
- **Discovered:** 2026-03-06
- **Test:** file-upload.spec.ts > "mic button visible on new task page"
- **Steps:**
  1. Go to /tasks/new
  2. Look for mic button via `button[aria-label*='mic' i]` selector
  3. Button exists but aria-label doesn't match the test locator
- **Expected:** Mic button found by aria-label containing "mic", "record", or "voice"
- **Actual:** Button exists but aria-label uses a different naming convention
- **Console errors:** None
- **Fix:** Update aria-label in MicButton.tsx to include "mic" or update test locator

### BUG-002: Skills Create panel lacks ARIA dialog role
- **Severity:** minor
- **Status:** fixed
- **Page:** /config/skills
- **Discovered:** 2026-03-06
- **Test:** skills.spec.ts > "create skill dialog can be closed"
- **Steps:**
  1. Go to /config/skills
  2. Click "Create" button
  3. Panel opens but has no `role=dialog` attribute
- **Expected:** Create Skill panel has `role=dialog` or `data-testid`
- **Actual:** Panel renders without semantic dialog role
- **Console errors:** None
- **Fix:** Add `role="dialog"` or `data-testid="create-skill-dialog"` to the Create Skill panel root

### BUG-003: Skills Detail panel lacks ARIA dialog role
- **Severity:** minor
- **Status:** fixed
- **Page:** /config/skills
- **Discovered:** 2026-03-06
- **Test:** skills.spec.ts > "skill card click opens detail view"
- **Steps:**
  1. Go to /config/skills
  2. Click a skill card
  3. Detail panel opens but has no `role=dialog` or `role=complementary`
- **Expected:** Skill detail panel has `role=dialog` or `data-testid="skill-detail"`
- **Actual:** Panel renders without semantic role
- **Console errors:** None
- **Fix:** Add `role="dialog"` or `data-testid="skill-detail"` to the Skill Detail panel root

### BUG-004: useAuth must be used within AuthProvider (HMR race condition)
- **Severity:** major
- **Status:** fixed
- **Page:** All pages (Layout.tsx)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed in browser console during development
- **Steps:**
  1. Run dev server with `make dev`
  2. Edit any source file to trigger HMR
  3. Observe console: `Uncaught Error: useAuth must be used within AuthProvider`
- **Expected:** HMR refresh should re-render within the AuthProvider context
- **Actual:** Layout.tsx calls `useAuth()` at line 7 but during HMR, the component tree momentarily renders outside AuthProvider
- **Console errors:**
  - `useAuth must be used within AuthProvider` (useAuth.tsx:68)
  - `An error occurred in the <Layout> component. Consider adding an error boundary.`
- **Fix:** useAuth() now returns a safe default (AUTH_DEFAULT) in dev mode instead of throwing when called outside AuthProvider. Production still throws. (useAuth.tsx)
- **Note:** This error only triggers during Vite HMR, not during cold page loads. Playwright E2E tests do cold loads so they won't catch this. The WebSocket errors that follow are a cascade from this — the auth token becomes unavailable, so WS connections fail.

### BUG-005: Streaming fails with 401 — invalid x-api-key
- **Severity:** critical
- **Status:** fixed
- **Page:** /tasks/:id (all task streaming)
- **Discovered:** 2026-03-06
- **Fixed:** 2026-03-06
- **Root cause:** Two issues: (1) `.env` file was not loaded at startup — `dotenvy` crate was missing. (2) WebAuthn `register_finish` generates a new vault key and deletes `secrets.enc`, wiping all stored secrets including `ANTHROPIC_API_KEY`. The env var fallback in `tasks.rs` only works if `.env` is loaded by dotenvy.
- **Fix:** (1) Added `dotenvy = "0.15"` to mcclawd-api Cargo.toml + `dotenvy::dotenv().ok()` in main.rs. (2) `run_agent_host()` in tasks.rs has vault→env var fallback chain. (3) `global-setup.ts` now re-seeds `ANTHROPIC_API_KEY` from `process.env` into the fresh vault after WebAuthn registration, preventing key loss during E2E runs.

### BUG-006: Setup biometric registration failing
- **Severity:** critical
- **Status:** fixed
- **Page:** /setup
- **Discovered:** 2026-03-06
- **Fixed:** 2026-03-06
- **Root cause:** Likely transient vault state mismatch from earlier sessions (stale vault.key + secrets.enc conflicting with fresh webauthn_credentials.json). The cleanup flow in `global-setup.ts` and `reset_credentials` endpoint now properly handle this. Additionally, `enableUI: true` in CDP `WebAuthn.enable` could cause intermittent failures in headless mode by triggering Chrome's passkey dialog overlay.
- **Fix:** (1) Changed `enableUI: true` to `enableUI: false` in global-setup.ts CDP call — virtual authenticator now handles ceremonies silently without UI overlay. (2) Added ANTHROPIC_API_KEY re-seeding after registration to prevent BUG-005 regression. (3) Verified: global-setup WebAuthn flow completes successfully (16 navigation tests pass with auth).

### BUG-007: Workspace files not showing populated default content
- **Severity:** major
- **Status:** fixed
- **Page:** /workspace
- **Discovered:** 2026-03-06
- **Test:** workspace.spec.ts
- **Steps:**
  1. Go to /workspace
  2. Click on SOUL.md, AGENTS.md, or USER.md tabs
  3. Content is empty or minimal placeholder text
  4. IDENTITY.md, TOOLS.md, HEARTBEAT.md tabs show empty content
- **Expected:** All 6 workspace files show rich OpenClaw-compatible default content (personality, agents config, user preferences, identity, tool guidelines, heartbeat schedule)
- **Actual:** SOUL.md/AGENTS.md/USER.md had ~30 byte stubs, IDENTITY.md/TOOLS.md/HEARTBEAT.md didn't exist on disk. API returned empty string for missing files.
- **Root cause:** Two issues: (1) `get_file()` in `workspace.rs` returned empty string when files were missing instead of auto-scaffolding. (2) `scaffold()` unconditionally overwrote files, so it couldn't safely be called on existing workspaces. (3) On-disk files had been saved with minimal content at some point, replacing the rich defaults.
- **Fix:** (1) `get_file()` now auto-scaffolds via `WorkspaceLoader::scaffold()` when file is missing or empty. (2) `scaffold()` now uses `write_if_missing_or_empty()` to preserve user edits while populating blank files. Files: `crates/mcclawd-api/src/server/workspace.rs`, `crates/mcclawd-agent/src/workspace.rs`.

### BUG-008: Usage data and available credits not updating
- **Severity:** major
- **Status:** fixed
- **Page:** /settings (SpendingDashboard)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed in Settings page SpendingDashboard
- **Steps:**
  1. Run one or more tasks that consume LLM tokens
  2. Navigate to Settings page
  3. Observe the "API Usage & Budget" SpendingDashboard section
  4. Usage data shows $0.00 / zero tokens despite completed tasks
  5. Wait 30+ seconds (the refetchInterval) — data remains stale
- **Expected:** SpendingDashboard reflects real token usage, cost, and remaining credits after each LLM call. Per-model and per-task breakdowns should populate. Budget spent/remaining should update.
- **Actual:** Usage data and available credits remain at zero or stale values. The `by_model` and `by_task` arrays in the API response are empty. Daily/monthly spent values do not increment.
- **Console errors:** None (API returns 200 with zero/empty data)
- **Root cause:** `ProviderPool::record_usage()` and `record_usage_detailed()` are never called from the agent/task execution path. The methods exist in `pool.rs` (line 348) and are covered by unit tests, but no code in `mcclawd-agent` or `mcclawd-api` task streaming calls them after LLM completions. The Rig agent loop completes requests but token counts are never fed back to the `ProviderPool`'s `UsageRecord` atomics or `BudgetTracker`.
- **Affected files:**
  - `crates/mcclawd-core/src/providers/pool.rs` — has `record_usage()` / `record_usage_detailed()` but only called in tests
  - `crates/mcclawd-api/src/server/providers.rs` — `provider_usage_detailed()` and `budget_info()` read from pool, return zeros
  - `crates/mcclawd-api/src/server/tasks.rs` — task streaming code does not call `record_usage()` after LLM responses
  - `ui/packages/app/src/pages/SettingsPage.tsx` — `SpendingDashboard` queries `/api/providers/usage/detailed` and `/api/providers/budget/info` every 30s, correctly displays whatever the backend returns
- **Fix:** After each Rig agent completion (streaming or non-streaming), extract token usage from the Rig response metadata and call `provider_pool.record_usage_detailed(provider, input_tokens, output_tokens, 0, estimated_cost, Some(task_id))`. This requires:
  1. Passing a reference to `AppState.provider_pool` into the task execution context
  2. Hooking into the Rig stream completion callback or post-processing the `FinalResponse`
  3. Extracting `usage.input_tokens` and `usage.output_tokens` from Rig's completion response
  4. Computing estimated cost using the pricing table in `providers.rs` (`known_pricing()`)

### BUG-009: Server autostart on code changes not working
- **Severity:** major
- **Status:** fixed
- **Page:** N/A (development tooling)
- **Discovered:** 2026-03-06
- **Test:** Manual — developer workflow
- **Steps:**
  1. Run `make dev` to start the development environment
  2. Edit a Rust source file in any crate (e.g., `crates/mcclawd-api/src/`)
  3. Save the file
  4. Observe that the backend server does NOT automatically rebuild and restart
- **Expected:** `cargo-watch` (or equivalent file watcher) detects source changes, rebuilds `mcclawd-api`, and restarts the `mc serve` process automatically
- **Actual:** Server continues running with the old binary. Developer must manually stop and rebuild (`cargo build --release -p mcclawd-api && ./target/release/mc serve`)
- **Console errors:** None
- **Root cause candidates:**
  1. `Makefile` `dev` target may not use `cargo-watch` or file watcher for the backend
  2. cargo-watch may not be installed or configured correctly
  3. Watch pattern may exclude relevant source directories
  4. PID file / process management may interfere with restart
- **Fix:** Verify `make dev` target uses `cargo-watch -x 'run -p mcclawd-api -- serve'` or similar. Check Makefile for the backend watch command. Ensure `cargo-watch` is installed (`cargo install cargo-watch`). Consider using `cargo-watch -w crates/ -x 'run -p mcclawd-api -- serve'` to watch all crate source dirs.
- **Suggested test file:** N/A (infrastructure, not E2E testable)

### BUG-010: Action bar mic button not recording speech
- **Severity:** major
- **Status:** fixed
- **Page:** /tasks/new, /tasks/:id (CommandBar, NewTaskPage, TaskDetailPage)
- **Discovered:** 2026-03-06
- **Test:** Manual — mic button interaction
- **Steps:**
  1. Navigate to /tasks/new (or open an existing task)
  2. Click or hold the mic button (MicButton component)
  3. Grant microphone permission if prompted
  4. Speak into the microphone
  5. Release / click again to stop
- **Expected:** Speech is captured via Web Speech API (SpeechRecognition) or MediaRecorder, transcribed, and inserted into the prompt textarea
- **Actual:** Mic button appears but does not capture or transcribe speech. No text is inserted into the prompt field after speaking.
- **Console errors:** TBD — check for `SpeechRecognition` API errors, permission denials, or missing browser support
- **Root cause candidates:**
  1. `MicButton.tsx` may only have volume visualization (AudioContext) without actual speech-to-text integration
  2. Web Speech API (`webkitSpeechRecognition`) may not be initialized or event handlers may be missing
  3. `onTranscript` callback may not be wired to the parent component's state setter
  4. Browser may not support SpeechRecognition (Chrome/Edge only, not Firefox/Safari)
- **Fix:** Check `MicButton.tsx` for SpeechRecognition initialization. Verify `onTranscript` prop is passed and connected. If only AudioContext (volume viz) is implemented, add SpeechRecognition API integration. Fallback: use MediaRecorder + Whisper API for cross-browser support.
- **Suggested test file:** file-upload.spec.ts or new mic.spec.ts

### BUG-011: No "clear all" action for tasks list
- **Severity:** minor
- **Status:** fixed
- **Type:** feature request
- **Page:** /tasks (TasksPage)
- **Discovered:** 2026-03-06
- **Test:** N/A — feature not yet implemented
- **Steps:**
  1. Navigate to /tasks
  2. Observe multiple completed/failed tasks in the list
  3. No way to bulk-clear or delete all tasks at once
- **Expected:** A "Clear All" link/button (e.g., in the page header or as a secondary action) that deletes all completed tasks, or all tasks with a confirmation dialog
- **Actual:** User must delete tasks one by one via the individual task delete button
- **Console errors:** None
- **Implementation notes:**
  1. Add a "Clear All" link/button to TasksPage header (near the "New Task" button)
  2. Show confirmation dialog: "Delete all N tasks? This cannot be undone."
  3. Backend: add `DELETE /api/tasks` endpoint (batch delete) in tasks.rs
  4. Frontend: add `api.tasks.clearAll()` method in client.ts
  5. Consider filtering options: "Clear completed", "Clear all", "Clear failed"
- **Suggested test file:** tasks.spec.ts

### BUG-012: Auto-seed ANTHROPIC_ADMIN_KEY from .env like ANTHROPIC_API_KEY
- **Severity:** minor
- **Status:** fixed
- **Type:** feature request
- **Page:** /settings (SpendingDashboard — credits display)
- **Discovered:** 2026-03-06
- **Test:** N/A — feature not yet implemented
- **Steps:**
  1. User adds `ANTHROPIC_ADMIN_KEY=sk-ant-admin-...` to `.env` file
  2. Start the server with `make dev` or `mc serve`
  3. Navigate to Settings → SpendingDashboard
  4. Credits card shows "estimated from local tracking" instead of real Anthropic data
- **Expected:** `ANTHROPIC_ADMIN_KEY` should be auto-seeded from `.env` into the encrypted secrets vault on startup (same as `ANTHROPIC_API_KEY` is handled via dotenvy). The `/api/providers/credits` endpoint should then use it to fetch real cost data from Anthropic Admin API.
- **Actual:** Only `ANTHROPIC_API_KEY` is auto-seeded from `.env`. The admin key must be manually added via the Secrets page.
- **Implementation notes:**
  1. In `crates/mcclawd-api/src/commands/serve.rs` (or wherever env auto-seed happens), add `ANTHROPIC_ADMIN_KEY` to the list of keys auto-seeded from env/`.env` into the vault
  2. The `provider_credits()` handler in `providers.rs` already reads `ANTHROPIC_ADMIN_KEY` from vault — just needs the key to be there
  3. This key is used for `GET /v1/organizations/cost_report` on the Anthropic Admin API to get real monthly cost data
- **Suggested test file:** settings.spec.ts

### BUG-013: Use ANTHROPIC_ADMIN_KEY to populate available models list
- **Severity:** minor
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Type:** feature request
- **Page:** /settings (Model selector), /tasks/new (Model dropdown)
- **Discovered:** 2026-03-06
- **Test:** N/A
- **Steps:**
  1. Set ANTHROPIC_ADMIN_KEY in .env / vault
  2. Navigate to Settings or New Task page
  3. Model dropdown shows hardcoded list or 503 error
- **Expected:** `/api/providers/models` uses ANTHROPIC_ADMIN_KEY (or ANTHROPIC_API_KEY) to call `GET /v1/models` on the Anthropic API and return the live model list
- **Actual:** Endpoint returns 503 when API key is missing/invalid; doesn't try admin key as fallback
- **Implementation notes:**
  1. In `providers.rs` `list_models()`, try ANTHROPIC_API_KEY first, fall back to ANTHROPIC_ADMIN_KEY
  2. Both keys can call `GET /v1/models` on Anthropic API
  3. Cache the result (models don't change often) — 1hr TTL
- **Suggested test file:** settings.spec.ts

### BUG-014: Account cost /api/providers/credits returns 400 error
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /settings (SpendingDashboard — CreditsCard)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed in browser console
- **Steps:**
  1. Set ANTHROPIC_ADMIN_KEY in .env / vault
  2. Navigate to Settings page
  3. CreditsCard shows error or falls back to local tracking
- **Expected:** Credits endpoint returns real cost data from Anthropic Admin API
- **Actual:** `GET /api/providers/credits` triggers a 400 from Anthropic Admin API. The handler falls back to local tracking but shows an error message.
- **Root cause:** The Anthropic Admin API `/v1/organizations/cost_report` requires a `starting_at` query parameter. The current `fetch_admin_cost_report()` in `providers.rs` does not send this field. Error: `{"type":"error","error":{"type":"invalid_request_error","message":"starting_at: Field required"}}`
- **Fix:** Add `starting_at` (and likely `ending_at`) query params to the Admin API call in `fetch_admin_cost_report()`. Format: ISO 8601 date strings. If Admin API is still inaccessible after fix, gracefully hide the error and show local tracking only.
- **Suggested test file:** settings.spec.ts

### BUG-015: Streaming text renders on single line instead of multiline content
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /tasks/:id (TaskDetailPage — streaming response)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed during live task streaming
- **Steps:**
  1. Create a new task (e.g., "Write a poem about Lulu in Bali looking after 7 kids")
  2. Watch the streaming response in TaskDetailPage
  3. Text accumulates on a single line with a spinner, showing only the latest word/fragment
  4. Content does NOT render as multiline markdown paragraphs
- **Expected:** Streaming TextDelta chunks accumulate into a growing block of text rendered via react-markdown, showing full paragraphs, line breaks, and formatting as the response builds up
- **Actual:** Only the latest streaming fragment is visible on one line (e.g., "vast") with a spinner. Previous text is not shown. The response appears to overwrite rather than append.
- **Screenshot:** Shows single word "vast" with spinner instead of accumulated poem text
- **Root cause candidates:**
  1. `useTaskStream.ts` may be replacing accumulated text instead of appending TextDelta chunks
  2. The streaming state variable may be reset on each chunk instead of concatenated
  3. CSS may be hiding overflow (single-line truncation)
  4. StatusIndicator "Typing" may be replacing the text content with just the latest delta
  5. The `newBlockRef` logic may be creating a new block per delta instead of accumulating
- **Fix:** Check `useTaskStream.ts` TextDelta handler — ensure it concatenates to accumulated text (`prev + delta`). Check TaskDetailPage rendering — ensure the accumulated text block is displayed, not just the status line.
- **Suggested test file:** task-detail.spec.ts

### BUG-016: CreditsCard shows API error text and wrong label
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /settings (SpendingDashboard — CreditsCard)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed in Settings page
- **Steps:**
  1. Navigate to Settings page
  2. Observe the third card in the SpendingDashboard row
  3. Card label says "Account Cost" — should say "Usage"
  4. Card shows error text: `Admin API error: API returned 400 Bad Request: {"type":"error",...,"message":"starting_at: Field required"}`
  5. Below the error: "estimated from local tracking"
- **Expected:** Card labeled "Usage", no error text visible when gracefully falling back to local tracking. Error should be logged to console only, not shown to user.
- **Actual:** Card labeled "Account Cost", raw API error JSON displayed to user, confusing UX
- **Root cause:** Two issues: (1) `CreditsCard` renders `credits.error` directly in the UI (line ~436 SettingsPage.tsx). (2) The `provider_credits()` handler populates the error field even on successful fallback to local tracking.
- **Fix:** (1) Rename "Account Cost" label to "Usage" in CreditsCard. (2) Don't display `credits.error` to the user — log it to console instead. (3) When falling back to local tracking, clear the error field so UI shows clean state. (4) Fix the underlying BUG-014 (add `starting_at` param) so Admin API works when key is present.
- **Suggested test file:** settings.spec.ts

### BUG-018: SpendingDashboard has unnecessary "This All" and "All Time" cards
- **Severity:** minor
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /settings (SpendingDashboard)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed in Settings page
- **Steps:**
  1. Navigate to Settings page
  2. Observe SpendingDashboard has 3 cards: "This All" ($0.00), "All Time" ($0.46), "Usage (Month)" ($0.46)
- **Expected:** Only 2 cards: "Credits Available" (credit balance) and "Usage" (the existing Usage card which looks good as-is). No "This All" or "All Time" cards.
- **Actual:** 3 cards shown, "This All" and "All Time" are redundant/confusing
- **Screenshot:** Shows 3-column grid with "This All", "All Time", "Usage (Month)" cards
- **Fix:** In `SettingsPage.tsx` SpendingDashboard, replace the 3-column card grid with a 2-column layout: (1) CreditsCard showing available credits, (2) the existing Usage/SpendCard filtered by period. Remove the "This All" and "All Time" SpendCards.
- **Suggested test file:** settings.spec.ts

### BUG-017: Microphone not working in task conversations
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /tasks/:id (TaskDetailPage — follow-up input)
- **Discovered:** 2026-03-06
- **Test:** Manual — mic button in task conversations
- **Steps:**
  1. Create and complete a task (or open an existing completed task)
  2. Click the mic button in the follow-up input area
  3. Speak into microphone
  4. No transcription appears in the follow-up input
- **Expected:** Speech is transcribed via Web Speech API and inserted into the follow-up text input
- **Actual:** Mic button appears but speech is not transcribed or inserted into the follow-up field
- **Root cause candidates:**
  1. `MicButton` component may use `onTranscript` callback but TaskDetailPage may not wire it correctly
  2. Web Speech API (`webkitSpeechRecognition`) may not be initialized in MicButton
  3. The `handleFollowUpTranscript` / `handleFollowUpInterim` handlers may not update the follow-up input state
  4. Browser compatibility — SpeechRecognition only works in Chrome/Edge
- **Related:** BUG-010 (action bar mic) was marked fixed — check if the fix also covers task conversation mic
- **Files:** `ui/packages/app/src/components/MicButton.tsx`, `ui/packages/app/src/pages/TaskDetailPage.tsx`
- **Suggested test file:** task-detail.spec.ts

### FEATURE-001: MCPorter MCP server management + per-task Docker containers
- **Severity:** N/A (feature request)
- **Status:** open
- **Discovered:** 2026-03-06
- **Description:** Import or reimplement MCPorter MCP functionality. MCP servers should run in Docker containers per agent/task (isolated). Key capabilities needed: MCP server discovery, installation with dependency resolution, per-task container lifecycle, config management.
- **Options:**
  1. Import MCPorter from source (https://github.com/steipete/mcporter) — Rust/TS, integrate as dependency or fork
  2. Use Docker MCP Toolkit (https://docs.docker.com/ai/mcp-catalog-and-toolkit/toolkit/) — 200+ servers, Docker Desktop integration
  3. Build native McClawd version using existing ClawHub + AgentGateway infrastructure
- **Architecture notes:** Currently MCP servers run as shared Docker containers via AgentGateway. Need per-task isolation: each task gets its own MCP container set, torn down on completion. Requires changes to sandbox/container.rs, tasks.rs, and MCP config.
- **Scope:** Large — needs dedicated planning session (`/gsd:new-project` or architecture plan)

### BUG-019: Agent responses have repetitive content and poor structure
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /tasks/:id (TaskDetailPage — streaming response)
- **Discovered:** 2026-03-06
- **Root cause:** `useTaskStream` did not clear `events` state before history replay. Two paths caused duplication:
  1. **Auto-reconnect** (`ws.onclose` → `connect()`): old accumulated TextDelta events remained in state; history replay then appended TextBlock again → same text shown twice.
  2. **React StrictMode double-mount** (dev): `useEffect` fired twice; second mount replayed history onto non-empty events state.
- **Fix:** Added `setEvents([])` + streaming ref resets in both `useEffect` (fresh mount/taskId change) and `ws.onclose` auto-reconnect handler. The intentional `reconnect()` (follow-ups) already managed events correctly and was unchanged.
- **Files:** `ui/packages/app/src/hooks/useTaskStream.ts`

### BUG-020: Markdown rendering is poor — no mermaid, tables, code blocks lack quality
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /tasks/:id (TaskDetailPage — response rendering)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed in rendered agent responses
- **Steps:**
  1. Ask agent to generate content with code blocks, tables, mermaid diagrams
  2. Observe the rendered output
  3. Code blocks mix with text, mermaid diagrams not rendered, tables poorly formatted
- **Expected:** First-class markdown rendering: syntax-highlighted code, rendered mermaid diagrams, proper tables, good typography
- **Actual:** Basic react-markdown with remark-gfm and rehype-highlight — no mermaid support, poor table styling, code/text mixing
- **Fix:** Replace or augment current markdown stack:
  1. Add mermaid diagram rendering (mermaid.js or rehype-mermaid)
  2. Improve code block styling (copy button, language label, line numbers)
  3. Better table CSS (borders, alternating rows, responsive)
  4. Consider using a more capable renderer like @uiw/react-markdown-preview or marked + DOMPurify
- **Files:** `ui/packages/app/src/pages/TaskDetailPage.tsx`, `ui/packages/app/src/components/StreamEntry.tsx`, `ui/packages/app/src/index.css`

### BUG-021: Credits amount wrong + spend graph lacks resolution-based units
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /settings (SpendingDashboard — CreditsCard + UsageBarChart)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed in Settings page
- **Steps:**
  1. Navigate to Settings page → SpendingDashboard
  2. Compare "Credits Available" value with Anthropic Console
  3. McClawd shows $5.56 but Claude Console reports $36.75
  4. Spend graph (UsageBarChart) shows only a single bar regardless of period selected
  5. Changing period filter (Day/Week/Month/Year) does not change graph granularity
- **Expected:**
  - Credits amount matches Anthropic Console balance ($36.75)
  - Spend graph adjusts its X-axis units based on selected period: Day→hourly bars, Week→daily bars, Month→daily bars, Year→monthly bars, All→monthly bars
- **Actual:**
  - Credits show $5.56 (likely showing only current month spend from Admin API cost_report, not actual credit balance)
  - Graph shows a single aggregated bar for any period — no time-series breakdown
- **Root cause candidates:**
  1. **Credits mismatch:** The `/api/providers/credits` endpoint returns `monthly_cost_usd` from the Admin API cost_report, but this is cumulative spend — NOT remaining credit balance. Anthropic has no public credit balance API. The $5.56 is likely the monthly spend amount being displayed as "credits" incorrectly. The $36.75 in Claude Console is the actual prepaid credit balance (not available via API).
  2. **Graph single bar:** `DailyUsage` tracking in `pool.rs` likely stores only daily aggregates keyed by date string. The `UsageBarChart` component reads this flat list and doesn't sub-divide into hourly/weekly buckets. Need: (a) backend to return usage bucketed by the requested granularity, or (b) frontend to re-bucket the daily data into the appropriate time units.
- **Fix:**
  1. **Credits:** Either (a) label the card honestly as "Monthly Spend" instead of "Credits Available" since Anthropic doesn't expose credit balance via API, or (b) compute `credits_remaining = starting_balance - monthly_spend` if the user configures their starting balance in settings.
  2. **Graph resolution:** Add a `granularity` query param to `/api/providers/usage/detailed` (values: `hourly`, `daily`, `monthly`). Backend buckets `DailyUsage` records accordingly. Frontend maps period→granularity: Day→hourly, Week→daily, Month→daily, Year→monthly. UsageBarChart renders one bar per bucket.
- **Files:**
  - `crates/mcclawd-core/src/providers/pool.rs` — DailyUsage storage, granularity bucketing
  - `crates/mcclawd-api/src/server/providers.rs` — credits endpoint label, usage granularity param
  - `ui/packages/app/src/pages/SettingsPage.tsx` — CreditsCard label, UsageBarChart time buckets
- **Suggested test file:** settings.spec.ts

### BUG-022: Installed skill detail view shows partial info, not scrollable
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /config/skills (Skill Detail dialog for installed skills)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed when clicking an installed skill card
- **Steps:**
  1. Navigate to /config/skills
  2. Click on an installed skill in the "Installed" sidebar
  3. Detail dialog opens but content is cut off / truncated
  4. Cannot scroll to see the full SKILL.md content
- **Expected:** Full SKILL.md content visible in a scrollable dialog — all sections (frontmatter, instructions, tools, examples, config) readable
- **Actual:** Dialog shows partial content, overflow is hidden or clipped, no scroll mechanism
- **Root cause candidates:**
  1. Dialog container has `overflow: hidden` or fixed height without `overflow-y: auto`
  2. Content height exceeds dialog max-height but scrollbar is suppressed
  3. Installed skill detail may use a different code path than browse skill detail
  4. The skill content fetch may return truncated data for installed skills
- **Fix:** Check the skill detail dialog component in SkillsPage.tsx. Ensure the content container has `overflow-y: auto` and the dialog has `max-h-[90vh]` with proper flex layout. Verify the `/api/skills/{name}/content` endpoint returns full content for installed skills.
- **Files:** `ui/packages/app/src/pages/SkillsPage.tsx`
- **Suggested test file:** skills.spec.ts

### BUG-023: Skill scanner broken — says "not installed", no auto-scan on install/preview
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /config/skills (SecurityBadge / scanner)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed when scanning skills
- **Steps:**
  1. Navigate to /config/skills
  2. Click scan button on a skill card (SecurityBadge pill)
  3. Scanner reports "not installed" even for installed skills
  4. Scanner no longer runs successfully
  5. Skills are not auto-scanned when installed or previewed
  6. No auto-generated tags from scanner results
- **Expected:**
  - Scanner runs for both installed and uninstalled skills (downloads SKILL.md for uninstalled)
  - Auto-scan triggers on install and preview
  - Scanner generates security tags (safe/caution/warning) displayed as badge
- **Actual:** Scanner fails with "not installed" error, doesn't run, no tags generated
- **Root cause candidates:**
  1. `skills_routes.rs` scan endpoint may check installation status incorrectly
  2. The scan temp-dir download path for uninstalled skills may be broken
  3. `scanner.rs` uvx subprocess may have changed or timed out
  4. Frontend SecurityBadge may not pass correct params to scan endpoint
- **Fix:**
  1. Check `/api/skills/{name}/scan` endpoint in skills_routes.rs — verify it handles both installed and uninstalled skills
  2. Check scanner.rs for uvx subprocess errors or timeout issues
  3. Verify auto-scan is wired into install and preview flows in SkillsPage.tsx
  4. Add tag generation from scan results (safe/caution/warning → skill metadata)
- **Files:**
  - `crates/mcclawd-api/src/server/skills_routes.rs` — scan endpoint
  - `crates/mcclawd-core/src/skill_parser.rs` — scanner.rs
  - `ui/packages/app/src/pages/SkillsPage.tsx` — SecurityBadge, auto-scan hooks

### BUG-024: CreditsCard should show "Estimated Usage" matching period selection
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /settings (SpendingDashboard — CreditsCard)
- **Discovered:** 2026-03-06
- **Test:** Manual — user preference
- **Steps:**
  1. Navigate to Settings page
  2. CreditsCard shows "API Spend (Month)" or "Estimated Spend"
  3. User wants "Credits Available" showing actual remaining credit balance
- **Expected:** Card labeled "Credits Available" showing the user's remaining Anthropic credit balance (e.g., $36.75 as shown in Claude Console)
- **Actual:** Card shows monthly spend amount labeled as "API Spend" or "Estimated Spend"
- **Root cause:** Anthropic has no public credit balance API. BUG-021 fix relabeled the card honestly as "spend" — but user wants credits.
- **Fix:** Use Admin API cost_report to get cumulative spend. Add a user-configurable "Credit Balance" field in Settings (editable like Max Turns). Store in config. Compute: `Credits Available = user_balance - cumulative_spend`. The cost_report with bucket_width=1d from account start gives total spend. User enters their known balance from Console (e.g., $36.75). Note: Anthropic has NO credit balance API endpoint — only cost/usage reporting. See https://github.com/anthropics/anthropic-sdk-python/issues/505
- **Files:**
  - `ui/packages/app/src/pages/SettingsPage.tsx` — CreditsCard, add balance config
  - `crates/mcclawd-core/src/config.rs` — add credit_balance field to config
  - `crates/mcclawd-api/src/server/config_routes.rs` — persist balance
- **Suggested test file:** settings.spec.ts

### BUG-025: E2E tests should tag ALL tasks "e2e-test" (currently only 1/51 tagged)
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Screenshot:** Shows 51 completed tasks, only "E2E test: cmdbar hidden" has e2e-test tag — all others untagged
- **Page:** /tasks (all task creation flows)
- **Discovered:** 2026-03-06
- **Test:** All E2E test files that create tasks
- **Steps:**
  1. E2E tests create tasks without tags (or inconsistently)
  2. User-created tasks have no automatic tags
  3. No way to distinguish test tasks from real tasks
- **Expected:**
  - All E2E test-created tasks auto-tagged `["e2e-test"]`
  - User-created tasks (via UI) auto-tagged `["user", "interactive"]`
  - Scheduled tasks auto-tagged `["user", "scheduled"]`
  - Global teardown can delete all `e2e-test` tagged tasks
- **Fix:**
  1. Update all E2E test files that call `page.getByRole("button", { name: "Run Task" }).click()` to ensure the createTask helper passes `tags: ["e2e-test"]`
  2. In NewTaskPage.tsx, auto-add `["user", "interactive"]` tags when creating via UI
  3. In schedule_routes.rs, auto-add `["user", "scheduled"]` tags for scheduled tasks
  4. Add global teardown in playwright config that calls `DELETE /api/tasks?tag=e2e-test`
- **Files:**
  - `ui/tests/helpers.ts` — createTask helper with default tags
  - `ui/tests/*.spec.ts` — all test files that create tasks
  - `ui/packages/app/src/pages/NewTaskPage.tsx` — auto-tag user tasks
  - `ui/playwright.config.ts` — global teardown
- **Suggested test file:** tasks.spec.ts

### BUG-026: Command bar navigation says "Navigated" but page doesn't change
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** All pages (CommandBar / system agent)
- **Discovered:** 2026-03-06
- **Test:** Manual — observed via screenshot
- **Steps:**
  1. Open command bar (Cmd+K)
  2. Type a navigation command like "go to settings"
  3. Toast shows "Navigated to the settings page (/config)." with Dismiss
  4. Page stays on the current page — does NOT navigate
- **Expected:** After command bar action, page actually navigates to /config/settings
- **Actual:** Success toast appears but `window.location` / React Router doesn't change
- **Root cause candidates:**
  1. Command bar handler shows success toast but doesn't call `navigate()` from React Router
  2. System agent returns a "navigated" response but the frontend doesn't act on it
  3. The navigation intent is detected but the router push/replace is missing
- **Files:** `ui/packages/app/src/components/CommandBar.tsx`, `ui/packages/app/src/components/Layout.tsx`
- **Suggested test file:** command-bar.spec.ts

### BUG-027: Mic input still not working anywhere (BUG-010/017 regression)
- **Severity:** major
- **Status:** open
- **Page:** /tasks/new, /tasks/:id, CommandBar
- **Discovered:** 2026-03-06
- **Test:** Manual — mic button click/hold produces no transcription
- **Steps:**
  1. Navigate to /tasks/new or any task detail page
  2. Click or hold the mic button
  3. No speech transcription occurs — no text inserted
- **Expected:** Web Speech API captures speech and inserts transcription into textarea
- **Actual:** Mic button shows volume visualization but no transcription
- **Root cause:** BUG-010/017 were marked fixed but mic still non-functional. MicButton.tsx likely has AudioContext for volume viz but SpeechRecognition API is not properly initialized or onresult handler doesn't update parent state.
- **Files:** `ui/packages/app/src/components/MicButton.tsx`, `ui/packages/app/src/pages/NewTaskPage.tsx`, `ui/packages/app/src/pages/TaskDetailPage.tsx`
- **Suggested test file:** file-upload.spec.ts or new mic.spec.ts

### BUG-028: Estimated usage should extrapolate spending to selected period
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /settings (SpendingDashboard — CreditsCard + UsageBarChart)
- **Discovered:** 2026-03-06
- **Test:** Manual — user expectation
- **Steps:**
  1. Navigate to Settings page → SpendingDashboard
  2. Select a period filter (Day, Week, Month, Year)
  3. CreditsCard shows raw cumulative spend for that period
  4. No extrapolation or projection is shown
- **Expected:**
  - When "Day" is selected: show today's spend AND extrapolate to daily rate (e.g., "$0.50 today → ~$15/month at this rate")
  - When "Month" is selected: show month-to-date spend AND extrapolate to full month (e.g., "$5.56 so far → ~$18.53 projected for full month")
  - When "Year" is selected: show year-to-date spend AND extrapolate to full year
  - The graph should show both actual bars and a projected/dotted line for the remainder of the period
- **Actual:** Only raw cumulative spend shown — no extrapolation, no projection, no rate calculation
- **Root cause:** CreditsCard and UsageBarChart only display raw `monthly_cost_usd` or `daily_usage` data. No extrapolation logic exists. Need to compute: `projected = (spend_so_far / days_elapsed) * total_days_in_period`
- **Fix:**
  1. In SettingsPage.tsx CreditsCard: compute projected spend based on elapsed fraction of period
  2. Show both actual and projected amounts: "$5.56 actual → $18.53 projected (Month)"
  3. In UsageBarChart: add dotted/dashed bars for projected remaining days
  4. Backend: no changes needed — frontend can compute from existing daily usage data
- **Files:**
  - `ui/packages/app/src/pages/SettingsPage.tsx` — CreditsCard extrapolation, UsageBarChart projection
- **Suggested test file:** settings.spec.ts

### BUG-029: Usage bar chart shows no bars despite real usage data
- **Severity:** major
- **Status:** fixed
- **Fixed:** 2026-03-06
- **Page:** /settings (SpendingDashboard — UsageBarChart)
- **Discovered:** 2026-03-06
- **Screenshot:** Month selected, $3.41 usage (103 requests), By Model table populated — but Daily Spend chart shows empty bars across 30-day range (2/4–3/1)
- **Root cause:** Two likely issues:
  1. **Field mismatch:** Backend `DailyUsage` struct has `{ date, cost_usd, tokens }` but frontend type expects `{ date, cost_usd, input_tokens, output_tokens, request_count }` — serde may serialize `tokens` not `cost_usd`, or cost_usd is always 0
  2. **Not called:** `record_usage_detailed()` in pool.rs populates `daily_history` but may not be called from the task execution path in tasks.rs (BUG-008 was marked fixed but may be incomplete)
  3. **In-memory only:** `daily_history` is a `Mutex<Vec<DailyUsage>>` — it resets on server restart. All usage data lost when cargo-watch restarts the server.
- **Fix:**
  1. Check tasks.rs for `record_usage_detailed()` calls after FinalResponse
  2. Ensure DailyUsage serde fields match frontend type expectations
  3. Consider persisting daily_history to disk (JSON file) so it survives restarts
- **Files:**
  - `crates/mcclawd-core/src/providers/pool.rs` — DailyUsage struct, daily_history storage
  - `crates/mcclawd-api/src/server/tasks.rs` — record_usage_detailed() call site
  - `crates/mcclawd-api/src/server/providers.rs` — usage endpoint
  - `ui/packages/app/src/api/types.ts` — DailyUsage frontend type
- **Suggested test file:** settings.spec.ts

## Console Error Log

| Page | Error Message | Frequency | Bug ID |
|------|--------------|-----------|--------|
| /tasks/00000000-... | WebSocket connection failed | Every task-detail test with fake UUID | N/A (benign — filtered) |
| Various | ResizeObserver loop | Intermittent | N/A (benign — filtered) |
