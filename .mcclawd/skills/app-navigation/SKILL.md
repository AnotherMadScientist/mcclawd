---
name: app-navigation
version: 1.0.0
author: mcclawd-team
description: Use when the user wants to navigate to a page in the McClawd app. Knows all available pages and routes.
tags:
  - system
  - navigation
---
# App Navigation

## Description
Navigate the McClawd web interface to the correct page. Use the `navigate_to` tool with the appropriate route path.

## Context
You control navigation for the McClawd web app. When the user asks to go somewhere, view something, or manage a resource, call `navigate_to` with the correct path.

## Instructions
Use the `navigate_to` tool to take the user to the right page. Match their intent to one of these routes:

| Intent | Route | Description |
|--------|-------|-------------|
| Home / task list / dashboard | `/` | Main task list |
| New task / create task | `/tasks/new` | New task creation form |
| View a specific task | `/tasks/{id}` | Task detail + chat |
| Workspace / personality / SOUL | `/workspace` | Workspace file editor |
| Settings / config | `/config` | App settings |
| Skills / browse skills | `/config/skills` | Skill browser + installer |
| Secrets / API keys | `/config/secrets` | Secret management |
| MCP servers / tools | `/config/mcp` | MCP server config |
| Docker / containers | `/config/docker` | Container management |
| Usage / spending / costs | `/config/usage` | Usage tracking |
| Security / audit | `/config/security` | Security events |

Always call the tool — never just respond with text. After calling `navigate_to`, confirm in one short sentence.

## Examples
User: Show me my tasks
Agent: *calls navigate_to with path="/"* — Navigated to the task list.

User: I want to add a new API key
Agent: *calls navigate_to with path="/config/secrets"* — Navigated to secrets management.

User: Let me see the skills
Agent: *calls navigate_to with path="/config/skills"* — Navigated to the skills browser.
