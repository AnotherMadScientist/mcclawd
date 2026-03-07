-- Persistent container tracking: survives server restarts.
-- Containers have restart_policy=unless-stopped, so they keep running
-- even when the API server goes down. On startup we reconnect to them.

CREATE TABLE IF NOT EXISTS persistent_containers (
    container_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    agent_type TEXT NOT NULL DEFAULT 'task',  -- 'task' or 'system'
    workspace_dir TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_persistent_containers_task ON persistent_containers(task_id);
CREATE INDEX IF NOT EXISTS idx_persistent_containers_type ON persistent_containers(agent_type);
