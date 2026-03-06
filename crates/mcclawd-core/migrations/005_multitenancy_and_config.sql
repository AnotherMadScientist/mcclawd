-- Multitenancy (user_id on all tables) + config/workspace/skills/mcp persistence.
-- Phase 0: single user 'admin'. Phase 2+: real multitenancy.

-- -----------------------------------------------------------------------
-- Add user_id + tags to existing tables
-- -----------------------------------------------------------------------

ALTER TABLE tasks ADD COLUMN IF NOT EXISTS user_id TEXT NOT NULL DEFAULT 'admin';
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS tags TEXT[] DEFAULT '{}';
CREATE INDEX IF NOT EXISTS idx_tasks_tags ON tasks USING GIN(tags);
CREATE INDEX IF NOT EXISTS idx_tasks_user ON tasks(user_id);

ALTER TABLE daily_usage ADD COLUMN IF NOT EXISTS user_id TEXT NOT NULL DEFAULT 'admin';
ALTER TABLE model_usage ADD COLUMN IF NOT EXISTS user_id TEXT NOT NULL DEFAULT 'admin';
ALTER TABLE task_usage ADD COLUMN IF NOT EXISTS user_id TEXT NOT NULL DEFAULT 'admin';

-- Rekey daily_usage to (user_id, date)
ALTER TABLE daily_usage DROP CONSTRAINT IF EXISTS daily_usage_pkey;
ALTER TABLE daily_usage ADD PRIMARY KEY (user_id, date);

-- Rekey model_usage to (user_id, model)
ALTER TABLE model_usage DROP CONSTRAINT IF EXISTS model_usage_pkey;
ALTER TABLE model_usage ADD PRIMARY KEY (user_id, model);

-- -----------------------------------------------------------------------
-- App config (key-value per tenant)
-- -----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS app_config (
    user_id TEXT NOT NULL DEFAULT 'admin',
    key TEXT NOT NULL,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, key)
);

-- -----------------------------------------------------------------------
-- Workspace files (SOUL.md, AGENTS.md, etc. per tenant per workspace)
-- -----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS workspace_files (
    user_id TEXT NOT NULL DEFAULT 'admin',
    workspace TEXT NOT NULL DEFAULT 'default',
    filename TEXT NOT NULL,
    content TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, workspace, filename)
);

-- -----------------------------------------------------------------------
-- Installed skills (per tenant)
-- -----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS installed_skills (
    user_id TEXT NOT NULL DEFAULT 'admin',
    name TEXT NOT NULL,
    version TEXT,
    skill_md TEXT,
    installed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, name)
);

-- -----------------------------------------------------------------------
-- Scan cache (skill security scan results per tenant)
-- -----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS scan_cache (
    user_id TEXT NOT NULL DEFAULT 'admin',
    skill_name TEXT NOT NULL,
    result JSONB NOT NULL,
    scanned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, skill_name)
);

-- -----------------------------------------------------------------------
-- Scheduled tasks (cron-based recurring tasks per tenant)
-- -----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id TEXT NOT NULL DEFAULT 'admin',
    name TEXT NOT NULL,
    cron_expr TEXT NOT NULL,
    prompt TEXT NOT NULL,
    tags TEXT[] DEFAULT '{}',
    model TEXT,
    workspace TEXT,
    enabled BOOLEAN NOT NULL DEFAULT true,
    last_run_at TIMESTAMPTZ,
    next_run_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_user ON scheduled_tasks(user_id);

-- -----------------------------------------------------------------------
-- Swarm runs (active/completed swarm executions per tenant)
-- -----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS swarm_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id TEXT NOT NULL DEFAULT 'admin',
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    config JSONB NOT NULL DEFAULT '{}',
    result JSONB,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_swarm_runs_user ON swarm_runs(user_id, started_at DESC);

-- -----------------------------------------------------------------------
-- MCP server configurations (per tenant)
-- -----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS mcp_servers (
    user_id TEXT NOT NULL DEFAULT 'admin',
    name TEXT NOT NULL,
    config JSONB NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, name)
);
