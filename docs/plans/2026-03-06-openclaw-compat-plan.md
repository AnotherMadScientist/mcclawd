# OpenClaw Compatibility Plan

**Date:** 2026-03-06
**Status:** Draft
**Scope:** Close 6 identified compatibility gaps between McClawd v5 and OpenClaw ecosystem

---

## Current State Summary

McClawd v5 already supports:
- 3 of 6 workspace files: SOUL.md, AGENTS.md, USER.md
- SKILL.md parsing via `skill_parser.rs` (header, frontmatter, ## sections)
- ClawHub integration: search, download, install, uninstall, upgrade
- `openclaw.json` + `.mcp.json` config parsing and migration
- Security hooks: DLP (7 patterns), SecretScanner (entropy), AuditHook, HookPipeline
- Skill context injection into system prompt via `ContextBuilder`
- Version tracking in `.installed.json` (name, version, source, installed_at)

---

## Gap 1: Missing Workspace Files (IDENTITY.md, TOOLS.md, HEARTBEAT.md)

**Priority:** P0 (core compat)
**Complexity:** M
**OpenClaw Reference:** OpenClaw uses 6 workspace markdown files for agent configuration. IDENTITY.md defines persona identity separate from soul/personality. TOOLS.md declares tool usage preferences and restrictions. HEARTBEAT.md defines scheduled/periodic tasks.

### Files to Read First
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-agent/src/workspace.rs` -- Workspace struct with `soul`, `agents`, `user` fields
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-agent/src/context.rs` -- ContextBuilder assembles system prompt from workspace files
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-api/src/server/workspace.rs` -- API routes, hardcoded `WORKSPACE_FILES: &[&str] = &["SOUL.md", "AGENTS.md", "USER.md"]`
- `/Users/velniukas/dev/macleodlabs/mcclawd/ui/packages/app/src/pages/WorkspacePage.tsx` -- UI with `const files = ["SOUL.md", "AGENTS.md", "USER.md"]`

### Files to Modify
| File | Change |
|------|--------|
| `crates/mcclawd-agent/src/workspace.rs` | Add `identity: Option<String>`, `tools: Option<String>`, `heartbeat: Option<String>` to `Workspace` struct; load them in `WorkspaceLoader::load()`; scaffold defaults in `scaffold()` |
| `crates/mcclawd-agent/src/context.rs` | Add IDENTITY.md injection after SOUL.md (section 1.5), TOOLS.md injection after AGENTS.md (section 3.5) in `build_system_prompt()`. HEARTBEAT.md is not injected into LLM context -- it is structural config for the task scheduler. |
| `crates/mcclawd-api/src/server/workspace.rs` | Update `WORKSPACE_FILES` constant to include all 6 files |
| `ui/packages/app/src/pages/WorkspacePage.tsx` | Update `files` array to include all 6; add tab icons and descriptions for new files |

### Implementation Steps
1. **Workspace struct** -- Add 3 new `Option<String>` fields. Update `WorkspaceLoader::load()` to call `read_optional()` for each new file path.
2. **Scaffold defaults** -- Write sensible default content for IDENTITY.md ("Agent identity and persona details"), TOOLS.md ("Tool preferences and restrictions"), HEARTBEAT.md ("Scheduled tasks -- cron-like definitions").
3. **Context injection** -- IDENTITY.md goes right after SOUL.md with `## Identity` header. TOOLS.md goes after AGENTS.md with `## Tool Preferences` header. HEARTBEAT.md is NOT injected into the prompt (it defines scheduled tasks, not LLM instructions).
4. **Heartbeat parser** -- Create `crates/mcclawd-agent/src/heartbeat_parser.rs`: parse HEARTBEAT.md into structured `HeartbeatTask` entries (cron expression, task description, enabled flag). Export from `lib.rs`.
5. **API** -- Update `WORKSPACE_FILES` constant. No new routes needed since `get_file`/`write_file` already accept any filename (with path traversal protection).
6. **UI** -- Add IDENTITY.md, TOOLS.md, HEARTBEAT.md to the `files` array. Consider grouping tabs: "Identity" (SOUL.md + IDENTITY.md), "Agent" (AGENTS.md + TOOLS.md), "User" (USER.md), "Schedule" (HEARTBEAT.md).

### Test Cases

**Rust unit tests** (`crates/mcclawd-agent/src/workspace.rs`):
- `test_load_workspace_with_all_6_files` -- All files present, all fields populated
- `test_load_workspace_missing_new_files` -- Only SOUL/AGENTS/USER exist, new fields are `None`
- `test_scaffold_creates_all_6_files` -- After scaffold, all 6 .md files exist on disk
- `test_identity_md_in_context` -- ContextBuilder includes IDENTITY.md content after SOUL.md
- `test_tools_md_in_context` -- ContextBuilder includes TOOLS.md content after AGENTS.md
- `test_heartbeat_md_not_in_context` -- ContextBuilder does NOT inject HEARTBEAT.md into system prompt

**Rust unit tests** (`crates/mcclawd-agent/src/heartbeat_parser.rs`):
- `test_parse_empty_heartbeat` -- Empty file returns empty vec
- `test_parse_heartbeat_with_tasks` -- Parses cron + description entries
- `test_parse_heartbeat_disabled_task` -- Respects enabled/disabled flag

**Playwright E2E** (`ui/tests/workspace.spec.ts`):
- `test('shows all 6 workspace file tabs')` -- Navigate to /workspace, verify 6 tabs visible
- `test('can edit and save IDENTITY.md')` -- Click IDENTITY.md tab, type content, save, reload, verify
- `test('can edit and save TOOLS.md')` -- Same flow for TOOLS.md
- `test('can edit and save HEARTBEAT.md')` -- Same flow for HEARTBEAT.md

---

## Gap 2: User-Defined Hooks

**Priority:** P1 (nice to have)
**Complexity:** L
**OpenClaw Reference:** OpenClaw supports user-defined hooks that fire at lifecycle points: pre-task, post-task, on-error, on-tool-call. Users configure hooks via config files. Hooks can execute shell commands, call APIs, or run scripts.

### Files to Read First
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/hooks/mod.rs` -- `SecurityHook` trait with `before_tool_call` and `after_tool_call`
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/hooks/pipeline.rs` -- `HookPipeline` chains hooks, first error stops `before_tool_call`, all run for `after_tool_call`
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/hooks/dlp.rs` -- DLP patterns with Block/Warn/Redact actions
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/hooks/audit.rs` -- AuditHook with JSONL sink

### Files to Create
| File | Purpose |
|------|---------|
| `crates/mcclawd-core/src/hooks/user_hook.rs` | `UserDefinedHook` struct implementing `SecurityHook` trait. Executes shell commands or HTTP calls based on config. |
| `crates/mcclawd-core/src/hooks/user_hook_config.rs` | Serde types for user hook TOML config: trigger point, command/url, timeout, retry policy |

### Files to Modify
| File | Change |
|------|--------|
| `crates/mcclawd-core/src/hooks/mod.rs` | Add `pub mod user_hook; pub mod user_hook_config;` and re-exports |
| `crates/mcclawd-core/src/config.rs` | Add `user_hooks: Vec<UserHookConfig>` to `McclawdConfig` |
| `crates/mcclawd-api/src/server/` | Add `/api/hooks` CRUD routes for managing user hooks |
| `ui/packages/app/src/pages/` | Add `HooksPage.tsx` or section in SettingsPage for hook management |

### Implementation Steps
1. **Config format** -- Define `UserHookConfig` in TOML:
   ```toml
   [[hooks]]
   name = "notify-slack"
   trigger = "post-task"       # pre-task | post-task | on-error | before-tool | after-tool
   type = "shell"              # shell | http
   command = "curl -X POST ..."
   timeout_ms = 5000
   enabled = true
   ```
2. **UserDefinedHook** -- Implements `SecurityHook`. For `before-tool` trigger, runs in `before_tool_call()`. For `after-tool` trigger, runs in `after_tool_call()`. Shell commands execute via `tokio::process::Command` with timeout. HTTP calls via `reqwest` with timeout.
3. **Task lifecycle hooks** -- Extend the task lifecycle in `mcclawd-tasks` to call pre-task/post-task/on-error hooks. This requires a new `TaskHook` trait (separate from `SecurityHook` which is tool-scoped).
4. **Pipeline integration** -- `HookPipeline::add()` already accepts `Arc<dyn SecurityHook>`. User hooks are instantiated from config at startup and added to the pipeline.
5. **API routes** -- CRUD for hook configs: list, create, update, delete, test (dry-run).
6. **UI** -- Simple form: name, trigger dropdown, type dropdown, command textarea, timeout, enabled toggle.

### Test Cases

**Rust unit tests** (`crates/mcclawd-core/src/hooks/user_hook.rs`):
- `test_shell_hook_executes_command` -- Hook runs `echo test` and captures output
- `test_shell_hook_timeout` -- Hook with 100ms timeout on `sleep 10` returns error
- `test_http_hook_calls_url` -- Mock HTTP server, verify POST received
- `test_disabled_hook_skipped` -- Hook with `enabled: false` does not execute
- `test_hook_trigger_filtering` -- before-tool hook only fires in `before_tool_call`, not `after_tool_call`

**Rust unit tests** (`crates/mcclawd-core/src/hooks/user_hook_config.rs`):
- `test_deserialize_shell_hook` -- TOML roundtrip
- `test_deserialize_http_hook` -- TOML roundtrip with url/method/headers
- `test_invalid_trigger_rejected` -- Unknown trigger string errors

**Playwright E2E** (`ui/tests/hooks.spec.ts` -- new file):
- `test('can create a shell hook')` -- Fill form, save, verify in list
- `test('can toggle hook enabled/disabled')` -- Toggle switch, verify state persists
- `test('can delete a hook')` -- Create, delete, verify removed

---

## Gap 3: JSON5 Config Support

**Priority:** P0 (core compat)
**Complexity:** S
**OpenClaw Reference:** OpenClaw uses JSON5 for all config files (`openclaw.json`, `.mcp.json`). JSON5 allows comments, trailing commas, unquoted keys, and single-quoted strings. Many users add comments to explain config choices.

### Files to Read First
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/compat/openclaw_config.rs` -- Uses `serde_json::from_str()` to parse configs
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/Cargo.toml` -- Dependencies list (no json5 crate present)

### Files to Modify
| File | Change |
|------|--------|
| `crates/mcclawd-core/Cargo.toml` | Add `json5 = "0.4"` dependency |
| `Cargo.toml` (workspace) | Add `json5 = "0.4"` to `[workspace.dependencies]` |
| `crates/mcclawd-core/src/compat/openclaw_config.rs` | Replace `serde_json::from_str()` with `json5::from_str()` in `load_openclaw_config()` and `load_mcp_json()`. Keep `serde_json` for output/serialization. |

### Implementation Steps
1. **Add dependency** -- `json5 = "0.4"` to workspace deps and mcclawd-core.
2. **Replace parser calls** -- In `load_openclaw_config()`: change `serde_json::from_str(&content)` to `json5::from_str(&content)`. Same for `load_mcp_json()`.
3. **Error mapping** -- Map `json5::Error` to `McclawdError::Config` (same as current `serde_json::Error` mapping).
4. **Backward compat** -- json5 is a superset of JSON, so existing valid JSON configs continue to work unchanged.

### Test Cases

**Rust unit tests** (`crates/mcclawd-core/src/compat/openclaw_config.rs`):
- `test_parse_json5_with_comments` -- Config with `// comment` and `/* block */` parses correctly
- `test_parse_json5_trailing_commas` -- Config with trailing commas in arrays and objects
- `test_parse_json5_unquoted_keys` -- Config with unquoted keys (e.g., `{name: "test"}`)
- `test_parse_json5_single_quotes` -- Config with single-quoted strings
- `test_standard_json_still_works` -- Existing test configs continue to parse (regression guard)

**Playwright E2E:** Not needed -- this is a backend parsing change with no UI impact.

---

## Gap 4: Skill Dependency Resolution

**Priority:** P1 (nice to have)
**Complexity:** L
**OpenClaw Reference:** OpenClaw SKILL.md files can declare dependencies on other skills via a `## Dependencies` section listing required skill names. When installing a skill, OpenClaw auto-installs its dependencies. Dependency cycles are detected and rejected.

### Files to Read First
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/clawhub/installer.rs` -- `SkillInstaller` with `install_from_registry()`, `upgrade()`, `uninstall()`
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/skill_parser.rs` -- Parses `## Description`, `## MCP Tools`, `## Install`, `## Context` sections
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/skills.rs` -- `LoadedSkill` struct (no dependencies field)

### Files to Modify
| File | Change |
|------|--------|
| `crates/mcclawd-core/src/skills.rs` | Add `dependencies: Vec<String>` to `LoadedSkill` |
| `crates/mcclawd-core/src/skill_parser.rs` | Parse `## Dependencies` section into list of skill names |
| `crates/mcclawd-core/src/clawhub/installer.rs` | Add `install_with_deps()` that resolves dependency tree, detects cycles, installs in order |

### Files to Create
| File | Purpose |
|------|---------|
| `crates/mcclawd-core/src/clawhub/dep_resolver.rs` | Dependency graph builder (topological sort), cycle detection, resolution order |

### Implementation Steps
1. **Parser update** -- Add `## Dependencies` parsing in `skill_parser.rs`. List items under this section are skill names (e.g., `- filesystem-tools`). Populate `LoadedSkill.dependencies`.
2. **Dependency resolver** -- `DepResolver` builds a DAG from skill dependencies. Uses topological sort (petgraph is already in the workspace for mcclawd-swarm). Detects cycles and returns error with cycle path.
3. **Install with deps** -- `SkillInstaller::install_with_deps(name, version)`:
   a. Download and parse target skill's SKILL.md
   b. Extract dependencies list
   c. For each dependency, check if already installed (skip if so)
   d. Recursively resolve (with cycle detection)
   e. Install in topological order (leaves first)
4. **Uninstall guard** -- `uninstall()` checks if any installed skill depends on the target. If so, return error listing dependents (or `--force` flag to override).
5. **UI update** -- Show dependency badges on skill detail dialog. "Requires: filesystem-tools, langextract" with links.

### Test Cases

**Rust unit tests** (`crates/mcclawd-core/src/skill_parser.rs`):
- `test_parse_dependencies_section` -- SKILL.md with `## Dependencies` listing 3 skills
- `test_parse_no_dependencies` -- SKILL.md without `## Dependencies` returns empty vec

**Rust unit tests** (`crates/mcclawd-core/src/clawhub/dep_resolver.rs`):
- `test_resolve_linear_chain` -- A depends on B, B depends on C -> install order: C, B, A
- `test_resolve_diamond` -- A depends on B and C, both depend on D -> installs D once
- `test_detect_cycle` -- A depends on B, B depends on A -> error
- `test_already_installed_skipped` -- Dependency already on disk is not re-downloaded

**Rust integration tests** (`crates/mcclawd-core/tests/dep_resolution.rs`):
- `test_install_with_deps_end_to_end` -- Mock ClawHub server, install skill with 2 deps, verify all 3 on disk

**Playwright E2E** (`ui/tests/skills.spec.ts` -- extend existing):
- `test('shows dependency list on skill detail')` -- Open skill detail, verify "Dependencies" section visible

---

## Gap 5: ClawHub Skill Versioning

**Priority:** P0 (core compat)
**Complexity:** M
**OpenClaw Reference:** ClawHub skills are versioned. Users can pin to specific versions. The `upgrade` command checks for newer versions. `.installed.json` tracks the installed version for comparison.

### Files to Read First
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/clawhub/installer.rs` -- `InstalledSkillInfo` has `version: String`, `upgrade()` method exists
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/clawhub/client.rs` -- `ClawHubSkillMeta` has `version: String`, `get_skill()` resolves latest

### Current State Analysis
InstalledSkillInfo already tracks version. However:
- No version comparison logic (semver or string compare)
- No `check_for_updates()` method to batch-check all installed skills
- No version pinning in config
- No `--version` flag on CLI install command
- UI does not show installed version vs latest version

### Files to Modify
| File | Change |
|------|--------|
| `crates/mcclawd-core/src/clawhub/installer.rs` | Add `check_for_updates()` -> `Vec<SkillUpdate>`, add `pin_version()`, add version comparison |
| `crates/mcclawd-core/src/clawhub/client.rs` | Ensure `get_skill(name, None)` always returns the latest version from API |
| `crates/mcclawd-core/src/config.rs` | Add `pinned_versions: HashMap<String, String>` to `SkillsConfig` |
| `crates/mcclawd-api/src/server/skills_routes.rs` | Add `GET /api/skills/updates` endpoint |
| `crates/mcclawd-api/src/commands/skills.rs` | Add `mc skills check-updates` subcommand |
| `ui/packages/app/src/pages/SkillsPage.tsx` | Show version badges (installed vs latest), "Update available" indicator |

### Implementation Steps
1. **Version comparison** -- Add `semver = "1"` to workspace deps. Parse versions as `semver::Version` where possible, fall back to string comparison for non-semver versions.
2. **Check for updates** -- `SkillInstaller::check_for_updates()`: iterate installed skills, call `client.get_skill(name, None)` for each, compare versions, return list of `SkillUpdate { name, installed_version, latest_version }`.
3. **Version pinning** -- `SkillsConfig` gains `pinned_versions: HashMap<String, String>`. When installing, if pinned version exists, use it instead of latest. `pin_version(name, version)` writes to config.
4. **CLI** -- `mc skills check-updates`: prints table of skills with available updates. `mc skills install foo@1.2.3`: installs specific version.
5. **API** -- `GET /api/skills/updates` returns JSON array of `SkillUpdate`. Called by UI on skills page load.
6. **UI** -- Each installed skill card shows current version. If update available, show badge with "v1.2 -> v1.3" and "Update" button.

### Test Cases

**Rust unit tests** (`crates/mcclawd-core/src/clawhub/installer.rs`):
- `test_check_for_updates_finds_newer` -- Installed v1.0, registry has v1.1 -> returns update
- `test_check_for_updates_up_to_date` -- Installed v1.1, registry has v1.1 -> no update
- `test_version_pinning_respected` -- Pinned to v1.0, install uses v1.0 even if v1.1 exists
- `test_install_specific_version` -- `install_from_registry("foo", Some("1.0.0"))` installs exact version
- `test_upgrade_to_specific_version` -- `upgrade("foo", Some("1.1.0"))` installs the specified version

**Rust unit tests** (`crates/mcclawd-core/src/config.rs`):
- `test_skills_config_with_pinned_versions` -- TOML roundtrip with pinned_versions map

**Playwright E2E** (`ui/tests/skills.spec.ts` -- extend existing):
- `test('shows version on installed skills')` -- Verify version badge visible
- `test('shows update available indicator')` -- Mock API to return update, verify UI badge

---

## Gap 6: Skill Context Injection Completeness

**Priority:** P0 (core compat)
**Complexity:** S
**OpenClaw Reference:** OpenClaw injects the full SKILL.md content into the agent's system prompt, including all sections: Description, Instructions, Tools, Examples, Config, Context. The agent uses these instructions when the skill is relevant.

### Files to Read First
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-agent/src/context.rs` -- `load_installed_skills()` reads raw SKILL.md content and concatenates it
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/skill_parser.rs` -- Parses into `LoadedSkill` struct with limited fields
- `/Users/velniukas/dev/macleodlabs/mcclawd/crates/mcclawd-core/src/skills.rs` -- `LoadedSkill` struct

### Current State Analysis
`load_installed_skills()` in `context.rs` reads raw SKILL.md content from disk and concatenates it into the system prompt. This means ALL sections are already injected (it uses the raw markdown, not the parsed struct). The gap is not in injection but in **structured handling**:
- No filtering: ALL installed skills are always injected, even if irrelevant
- No size management: 50 installed skills could overwhelm the context window
- `LoadedSkill` struct lacks: `instructions: String`, `examples: String`, `config: String` fields

### Files to Modify
| File | Change |
|------|--------|
| `crates/mcclawd-core/src/skills.rs` | Add `instructions`, `examples`, `config_section` fields to `LoadedSkill` |
| `crates/mcclawd-core/src/skill_parser.rs` | Parse `## Instructions`, `## Examples`, `## Config` sections |
| `crates/mcclawd-agent/src/context.rs` | Add skill relevance filtering: only inject skills referenced in AGENTS.md or matched by task prompt keywords. Add token budget: truncate skill context if total exceeds threshold. |

### Implementation Steps
1. **Parser completeness** -- Add parsing for `## Instructions`, `## Examples`, `## Config` sections in `skill_parser.rs`. Populate new `LoadedSkill` fields.
2. **Skill relevance filtering** -- In `build_system_prompt()`, if AGENTS.md assigns specific skills to the agent, only inject those. If no assignment exists, inject all (current behavior).
3. **Token budget** -- Add `max_skill_context_chars: usize` to config (default: 50000). In `load_installed_skills()`, truncate with note if total exceeds budget. Priority: skills assigned in AGENTS.md first, then alphabetical.
4. **Structured injection** -- Instead of raw markdown dump, inject each skill with clear delimiters:
   ```
   ### Skill: filesystem-tools (v1.2.0)
   **Instructions:** ...
   **Tools:** filesystem.read, filesystem.write
   **Context:** ...
   ```

### Test Cases

**Rust unit tests** (`crates/mcclawd-core/src/skill_parser.rs`):
- `test_parse_instructions_section` -- SKILL.md with `## Instructions` correctly parsed
- `test_parse_examples_section` -- SKILL.md with `## Examples` correctly parsed
- `test_parse_config_section` -- SKILL.md with `## Config` correctly parsed
- `test_parse_all_sections` -- SKILL.md with all known sections, all fields populated

**Rust unit tests** (`crates/mcclawd-agent/src/context.rs`):
- `test_skill_filtering_by_agents_md` -- Only assigned skills injected when AGENTS.md has assignments
- `test_all_skills_injected_when_no_assignments` -- No AGENTS.md -> all skills included
- `test_skill_context_budget_truncation` -- 100 skills, 50000 char budget -> truncated with note
- `test_structured_skill_injection_format` -- Verify output format has skill name header and sections

**Playwright E2E:** Not needed -- this is internal prompt construction with no direct UI impact.

---

## Implementation Order

| Phase | Gap | Priority | Est. Days | Dependency |
|-------|-----|----------|-----------|------------|
| 1 | Gap 3: JSON5 Config | P0 | 0.5 | None |
| 1 | Gap 6: Skill Context Injection | P0 | 1 | None |
| 2 | Gap 1: Missing Workspace Files | P0 | 2 | None |
| 2 | Gap 5: Skill Versioning | P0 | 2 | None |
| 3 | Gap 4: Skill Dependencies | P1 | 3 | Gap 5 (versioning) |
| 4 | Gap 2: User-Defined Hooks | P1 | 4 | None |

**Phase 1** (1.5 days): Quick wins -- JSON5 parsing and skill context improvements. Both are small, self-contained changes.

**Phase 2** (4 days): Core compat -- workspace files and versioning. These are the most visible gaps for OpenClaw users migrating to McClawd.

**Phase 3** (3 days): Dependency resolution. Builds on versioning (needs version-aware install).

**Phase 4** (4 days): User hooks. Lower priority, more complex, can be deferred.

**Total estimate:** ~12.5 days of focused implementation.

---

## Test Coverage Summary

| Gap | New Rust Unit Tests | New E2E Tests | Modified E2E Tests |
|-----|--------------------:|-------------:|-----------------:|
| Gap 1: Workspace Files | 9 | 4 (workspace.spec.ts) | 0 |
| Gap 2: User Hooks | 8 | 3 (hooks.spec.ts -- new) | 0 |
| Gap 3: JSON5 Config | 5 | 0 | 0 |
| Gap 4: Skill Deps | 5 + 1 integration | 0 | 1 (skills.spec.ts) |
| Gap 5: Versioning | 6 | 0 | 2 (skills.spec.ts) |
| Gap 6: Skill Context | 7 | 0 | 0 |
| **Total** | **41** | **7** | **3** |

### Missing Test Coverage (Existing Code)

Beyond the gaps above, these areas of existing code lack test coverage:

1. **`crates/mcclawd-agent/src/context.rs`** -- Zero unit tests. `build_system_prompt()` and `load_installed_skills()` are untested. Should have at least 5 tests covering: empty workspace, full workspace, skills loading, section ordering, response guidelines injection.

2. **`crates/mcclawd-agent/src/workspace.rs`** -- Zero unit tests. `WorkspaceLoader::load()` and `scaffold()` are untested. Should have at least 4 tests: load existing, load missing, scaffold creates files, scaffold idempotent.

3. **`crates/mcclawd-api/src/server/workspace.rs`** -- Zero unit tests. API handlers for workspace CRUD untested. Should have at least 3 integration tests: list files, get file, write file.

4. **`crates/mcclawd-core/src/skill_loader.rs`** -- Zero unit tests. `SkillLoader::discover_all()` and `resolve_for_agent()` untested. Should have at least 4 tests: empty dir, multiple skills, missing SKILL.md, agent-specific resolution.

5. **`ui/tests/workspace.spec.ts`** -- File exists but needs verification that it covers tab switching, content editing, save persistence, and error states.

6. **`crates/mcclawd-core/src/hooks/pipeline.rs`** -- Has tests but missing: empty pipeline pass-through, mixed hook types (DLP + audit + user), error aggregation in `after_tool_call`.

### Recommended Test Infrastructure Additions

- **Mock ClawHub server** -- Create `crates/mcclawd-core/tests/fixtures/mock_clawhub.rs` with a `wiremock`-based ClawHub mock for integration tests. Currently installer tests use filesystem mocks but not HTTP mocks.
- **Workspace test fixtures** -- Create `crates/mcclawd-agent/tests/fixtures/` with sample workspace directories containing all 6 files for deterministic testing.
- **SKILL.md test fixtures** -- Create `crates/mcclawd-core/tests/fixtures/skills/` with sample SKILL.md files covering: minimal, full, with-dependencies, malformed.

---

## Verification Checklist

Before marking each gap as complete:

- [ ] All new Rust unit tests pass (`cargo test --workspace`)
- [ ] All existing tests still pass (no regressions)
- [ ] New E2E tests pass (`make test-e2e`)
- [ ] Existing E2E tests still pass
- [ ] Existing `openclaw.json` configs parse correctly with JSON5 parser
- [ ] `mc import openclaw` still works end-to-end
- [ ] Workspace page shows all 6 files and allows CRUD
- [ ] Skill install/upgrade/uninstall still works
- [ ] System prompt includes all expected sections in correct order
- [ ] No clippy warnings (`cargo clippy --workspace`)
- [ ] Documentation updated (CLAUDE.md, architecture doc if structural changes)
