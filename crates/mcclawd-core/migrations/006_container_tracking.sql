-- Container tracking: persist container_id and execution_mode per task
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS container_id TEXT;
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS execution_mode TEXT NOT NULL DEFAULT 'docker';
