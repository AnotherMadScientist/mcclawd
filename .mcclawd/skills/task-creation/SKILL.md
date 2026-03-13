---
name: task-creation
version: 1.0.0
author: mcclawd-team
description: Use when the user wants to create a new agent task. Accepts a prompt and optional configuration like model, workspace, skills, and tags.
tags:
  - system
  - tasks
---
# Task Creation

## Description
Create new agent tasks in McClawd. Use the `create_task` tool to start an agent with the user's prompt.

## Context
You can create agent tasks that run inside Docker containers. Each task gets its own sandbox with access to MCP tools and optionally selected skills.

## Instructions
Use the `create_task` tool when the user wants to:
- Run a task / do something / ask the agent to work on something
- Analyze a document, write code, research a topic, etc.
- Start a new agent conversation

Parameters:
- `prompt` (required): The task description — what the agent should do
- `model` (optional): LLM model to use (defaults to config setting)
- `workspace` (optional): Workspace name (defaults to "default")
- `skills` (optional): List of skill names to include
- `tags` (optional): Tags for organizing tasks

After creating the task, confirm with the task ID and navigate to it.

## Examples
User: Analyze the latest quarterly report
Agent: *calls create_task with prompt="Analyze the latest quarterly report"* — Created task t-abc123. Navigating to it now.

User: Write a Python script that fetches weather data
Agent: *calls create_task with prompt="Write a Python script that fetches weather data"* — Created task t-def456.

User: Research competitors using the web tools
Agent: *calls create_task with prompt="Research competitors" and skills=["web-search"]* — Created task with web-search skill enabled.
