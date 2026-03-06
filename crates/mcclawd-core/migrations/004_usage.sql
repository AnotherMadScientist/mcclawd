-- Usage persistence: daily usage, per-model, per-task usage tracking.

CREATE TABLE IF NOT EXISTS daily_usage (
    date DATE PRIMARY KEY,
    cost_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
    tokens BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS model_usage (
    model TEXT PRIMARY KEY,
    input_tokens BIGINT NOT NULL DEFAULT 0,
    output_tokens BIGINT NOT NULL DEFAULT 0,
    total_tokens BIGINT NOT NULL DEFAULT 0,
    estimated_cost_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
    request_count BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS task_usage (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL,
    prompt_preview TEXT NOT NULL,
    model TEXT NOT NULL,
    total_tokens BIGINT NOT NULL DEFAULT 0,
    estimated_cost_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_task_usage_created ON task_usage(created_at DESC);
