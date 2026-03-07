# McClawd E2E Test Coverage Audit

> Last updated: 2026-03-07 | Total tests: 251 | Skipped: 10 | Console monitoring: 12/13 files | Flaky: 0

## Summary

| Page | Tests | Actions Covered | Actions Total | Coverage |
|------|-------|-----------------|---------------|----------|
| /login | 7 | 7 | 8 | 88% |
| /setup | 3 | 3 | 6 | 50% |
| / (Tasks) | 12 | 8 | 9 | 89% |
| /tasks/new | 16 | 11 | 13 | 85% |
| /tasks/:id | 16 | 11 | 15 | 73% |
| /config/workspace | 15 | 10 | 11 | 91% |
| /config/skills | 12 | 10 | 15 | 67% |
| /config/mcp | 7 | 6 | 7 | 86% |
| /config/secrets | 16 | 13 | 13 | 100% |
| /config/settings | 11 | 8 | 8 | 100% |
| Command Bar | 14 | 11 | 12 | 92% |
| Navigation | 13 | 9 | 10 | 90% |
| File Upload | 7 | 6 | 8 | 75% |

## Per-Page Coverage

### /login (login.spec.ts) — 7 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Show biometric login button | renders login page with Biometric ID button | DONE |
| Show fingerprint icon | shows fingerprint icon on unlock button | DONE |
| Redirect authenticated to / | authenticated user is redirected away from login page | DONE |
| Sign out clears token | sign out clears token and returns to login | DONE |
| Redirect unauthenticated to /login | unauthenticated user visiting / redirects to /login | DONE |
| Invalid token shows login | invalid/garbage token redirects to login | DONE |
| Dev-only reset link visible | dev reset link visible in dev mode | DONE (skips if not found) |
| Show error on failed auth | show error on failed biometric auth | MISSING — requires real authenticator failure |

### /setup (setup.spec.ts) — 3 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Show setup heading | shows setup heading when no credentials | DONE |
| Show avatar image | shows avatar image | DONE |
| Show register button | shows register or login button | DONE |
| Register with biometric | register biometric flow | MISSING — covered by global-setup |
| Redirect after setup | redirect to login after setup | MISSING — covered by global-setup |
| Show error on failed setup | setup error handling | MISSING |

### / Tasks Dashboard (tasks.spec.ts) — 12 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Show heading + description | shows Tasks heading and description | DONE |
| Show stats (Running/Completed/Failed) | shows stats row | DONE |
| New Task button visible | has New Task button | DONE |
| New Task navigates to /tasks/new | New Task button navigates | DONE |
| Show recent tasks or empty state | shows Recent heading or empty state | DONE |
| Task card shows prompt + status | task card shows prompt and status after creation | DONE |
| Task card click navigates to detail | task card click navigates to task detail | DONE |
| Stats show numeric values | stats cards contain numeric values | DONE |
| Empty state shows helpful message | empty state when no tasks | MISSING — depends on fresh state |

### /tasks/new (new-task.spec.ts) — 16 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Show heading + description | shows New Task heading | DONE |
| Show prompt textarea | shows prompt textarea | DONE |
| Show Available Resources | shows Available Resources section | DONE |
| Show model resource | shows model resource card | DONE |
| Show workspace resource | shows workspace resource card | DONE |
| Show builtin tools | shows builtin tools resource card | DONE |
| Show MCP tools | shows MCP tools resource card | DONE |
| Submit prompt redirects | submit prompt redirects to task detail | DONE |
| Empty prompt does not submit | empty prompt does not submit | DONE |
| Shift+Enter adds newline | shift+enter adds newline in prompt | DONE |
| Enter does not submit textarea | enter in textarea does not submit | DONE |
| File attach shows thumbnail | attach file shows thumbnail | MISSING |
| Remove attached file | remove attached file before submit | MISSING |

### /tasks/:id (task-detail.spec.ts) — 16 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Show task heading | shows task fallback heading | DONE |
| Show complete status | shows Complete status | DONE |
| Completion state | shows completion state for non-existent task | DONE |
| Back button navigates | back button navigates to tasks list | DONE |
| Show real task prompt | shows real task prompt in heading | DONE |
| Streaming content appears | streaming content visible | DONE |
| Follow-up input visible | follow-up input after completion or failure | DONE |
| Status indicator | shows status indicator (Running/Complete/Connected) | DONE |
| Streaming text blocks | streaming content appears as text blocks | DONE |
| Follow-up input after complete | follow-up input visible after task completes | DONE |
| Cancel button during running | cancel/stop button visible during running | DONE |
| Markdown code blocks | markdown code blocks render (skips without API key) | DONE (conditional) |
| Send follow-up message | follow-up sends and gets response | MISSING |
| Edit message truncates | edit message truncates conversation | MISSING |
| Retry message resends | retry message resends same message | MISSING |
| Attachment thumbnails | attachment thumbnails in conversation | MISSING |

### /config/workspace (workspace.spec.ts) — 15 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Show heading | shows Workspace Files heading | DONE |
| Show tabs (SOUL/AGENTS/USER) | shows tab buttons | DONE |
| SOUL.md selected by default | SOUL.md default selected | DONE |
| Switch tabs | clicking tab switches file | DONE |
| Textarea has content | textarea has initial content | DONE |
| Save button visible | save button visible | DONE |
| Edit + save persists | edit and save persists content | DONE |
| Each tab loads different content | each tab loads different content | DONE |
| Saved content persists across reload | saved content persists across reload | DONE |
| Switching tabs preserves edits | switching tabs preserves unsaved edits | DONE |
| Dirty warning on tab switch | unsaved changes warning | MISSING |

### /config/skills (skills.spec.ts) — 12 tests (2 skipped)
| Action | Test Name | Status |
|--------|-----------|--------|
| Show heading | shows Skills heading | DONE |
| Show search input | shows search input | DONE |
| Show Sync button | shows Sync button | DONE |
| Show Create button | shows Create button | DONE |
| Search filters skills | search input filters | DONE |
| Skill cards show name | skill cards show name text | DONE |
| Sync triggers refresh | sync button triggers catalog refresh | DONE |
| Create skill dialog opens | create skill dialog opens | DONE |
| Create skill dialog closes | create skill dialog can be closed | SKIPPED — needs role=dialog |
| Skill card opens detail | skill card click opens detail view | SKIPPED — needs data-testid |
| Installed sidebar | installed skills sidebar shows section | DONE |
| Install a skill | install skill flow | MISSING |
| Uninstall a skill | uninstall skill flow | MISSING |
| Security scan badge | security scan badge appears | MISSING |
| Close dialogs (Escape/X) | dialog close methods | MISSING |

### /config/mcp (mcp-servers.spec.ts) — 7 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Show heading | shows MCP Servers heading | DONE |
| List configured servers | lists configured servers | DONE |
| Show server images | shows server image names | DONE |
| Show server ports | shows server ports | DONE |
| Server status indicators | server cards show status indicators | DONE |
| Server cards clickable | server cards are clickable or expandable | DONE |
| Empty state | empty state when no servers | MISSING |

### /config/secrets (secrets.spec.ts) — 16 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Show heading + description | shows heading and description | DONE |
| Show input fields | shows name and value inputs | DONE |
| Show existing secret | shows ANTHROPIC_API_KEY | DONE |
| Values hidden by default | secret values hidden | DONE |
| Create a new secret | can create a new secret | DONE |
| Delete a secret | can delete a secret | DONE |
| Reveal/hide secret | toggle secret visibility | DONE |
| Edit secret value | can edit a secret value | DONE |
| Edit mode shows save/cancel | edit mode buttons | DONE |
| API-created secret visible | secret created via API visible | DONE |
| Empty name validation | add secret with empty name validation | DONE |
| Special characters | special characters in secret name | DONE |
| Many secrets render | many secrets render and page scrolls | DONE |

### /config/settings (settings.spec.ts) — 11 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Show heading | shows Settings heading | DONE |
| Show Model field | shows Model field with value | DONE |
| Show Max Turns | shows Max Turns field | DONE |
| Show Default Workspace | shows Default Workspace field | DONE |
| Show Data Directory | shows Data Directory field | DONE |
| All fields non-empty | all config fields have non-empty values | DONE |
| No console errors | page renders without console errors | DONE |
| MCP gateway config | settings shows MCP gateway config | DONE (soft assert) |

### Command Bar (command-bar.spec.ts) — 14 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Visible on dashboard | CommandBar visible on dashboard | DONE |
| Cmd+K focuses input | Cmd+K focuses the command bar | DONE |
| Send button disabled empty | send button disabled when empty | DONE |
| Can type in input | can type in command bar | DONE |
| Visible on config pages | visible on config pages | DONE |
| Hidden on /tasks/new | command bar hidden on /tasks/new | DONE |
| Hidden on /tasks/:id | command bar hidden on task detail page | DONE |
| Escape blurs input | escape blurs command bar input | DONE |
| Visible on workspace | command bar visible on workspace page | DONE |
| Visible on skills | command bar visible on skills page | DONE |
| Visible on secrets | command bar visible on secrets page | DONE |
| Type + Enter submits | enter key submits message | MISSING |

### Navigation (navigation.spec.ts) — 13 tests
| Action | Test Name | Status |
|--------|-----------|--------|
| Shows branding | sidebar shows McClawd branding | DONE |
| Tasks link visible | sidebar has Tasks link | DONE |
| All config links | sidebar has Configuration links | DONE |
| Tasks link navigates | clicking Tasks navigates to / | DONE |
| Workspace link navigates | clicking Workspace navigates | DONE |
| Skills link navigates | clicking Skills navigates | DONE |
| MCP link navigates | clicking MCP navigates | DONE |
| Secrets link navigates | clicking Secrets navigates | DONE |
| Settings link navigates | clicking Settings navigates | DONE |
| Active link highlight | active sidebar link has visual highlight | DONE |
| Browser back/forward | browser back/forward navigation works | DONE |

### File Upload (file-upload.spec.ts) — 7 tests (1 skipped, 1 pre-existing failure)
| Action | Test Name | Status |
|--------|-----------|--------|
| Attach button visible | attach button visible | DONE |
| File input exists | file input exists for upload | DONE |
| Prompt + Run Task visible | has prompt input and button | DONE |
| Mic button visible | mic button visible | PRE-EXISTING FAILURE — aria-label mismatch |
| Attach file shows thumbnail | attaching file shows thumbnail | DONE |
| Remove attached file | can remove attached file | SKIPPED — remove button UI differs |
| Multiple files attach | multiple files can be attached | DONE |
| Preview dialog opens | preview dialog on thumbnail click | MISSING |

## Skipped Tests Summary

| File | Test | Reason |
|------|------|--------|
| login.spec.ts | dev reset link visible | Soft-skips when link not present in DOM |
| skills.spec.ts | create skill dialog can be closed | Create Skill panel lacks role=dialog; needs ARIA role |
| skills.spec.ts | skill card click opens detail | Skill detail panel lacks role=dialog; needs data-testid |
| file-upload.spec.ts | can remove attached file | Remove button not found; UI may differ |
| task-detail.spec.ts | markdown code blocks render | Skips when no ANTHROPIC_API_KEY available |

## Known Issues

| Issue | Impact | Recommendation |
|-------|--------|----------------|
| Skills page panels lack ARIA roles | 2 tests skipped | Add role=dialog or data-testid to Create/Detail panels |
| Mic button aria-label mismatch | 1 pre-existing failure | Update aria-label in MicButton.tsx or fix locator |
| File remove button UI differs | 1 test skipped | Standardize remove button for attachments |
