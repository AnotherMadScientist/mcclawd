-- Migration 008: Security events, DLP findings, and DLP policies
-- Supports the HookPipeline security scanning infrastructure

-- Security events: every scan event (allowed, warned, blocked)
CREATE TABLE IF NOT EXISTS security_events (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT,
    user_id TEXT NOT NULL DEFAULT 'admin',
    agent_id TEXT,
    trace_id TEXT,
    span_id TEXT,
    event_type TEXT NOT NULL,
    tool_name TEXT,
    direction TEXT,
    threat_level TEXT,
    details JSONB NOT NULL DEFAULT '{}',
    action_taken TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_security_events_task ON security_events(task_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_security_events_type ON security_events(event_type, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_security_events_user ON security_events(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_security_events_trace ON security_events(trace_id);

-- DLP findings: individual tagged detections
CREATE TABLE IF NOT EXISTS dlp_findings (
    id BIGSERIAL PRIMARY KEY,
    security_event_id BIGINT NOT NULL REFERENCES security_events(id) ON DELETE CASCADE,
    finding_type TEXT NOT NULL,
    tag TEXT NOT NULL,
    pattern_name TEXT,
    confidence REAL,
    data_hash TEXT,
    redacted_preview TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_dlp_findings_event ON dlp_findings(security_event_id);
CREATE INDEX IF NOT EXISTS idx_dlp_findings_tag ON dlp_findings(tag);
CREATE INDEX IF NOT EXISTS idx_dlp_findings_type ON dlp_findings(finding_type);

-- DLP policies: configurable detection/action rules
CREATE TABLE IF NOT EXISTS dlp_policies (
    id SERIAL PRIMARY KEY,
    name TEXT UNIQUE NOT NULL,
    description TEXT,
    tag_pattern TEXT NOT NULL,
    tool_pattern TEXT,
    action TEXT NOT NULL DEFAULT 'warn',
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed default policies
INSERT INTO dlp_policies (name, description, tag_pattern, action) VALUES
    ('block_private_keys', 'Block private keys in tool calls', 'PRIVATE_KEY', 'block'),
    ('block_db_urls', 'Block database connection strings', 'DATABASE_URL', 'block'),
    ('warn_pii', 'Warn on PII detection', 'PERSON_NAME|EMAIL_ADDRESS|PHONE_NUMBER|US_SSN|CREDIT_CARD', 'warn'),
    ('warn_api_keys', 'Warn on API key detection', 'AWS_.*|GITHUB_TOKEN|OPENAI_KEY|ANTHROPIC_KEY|STRIPE_KEY', 'warn'),
    ('block_injection', 'Block prompt/command injection attempts', 'PROMPT_INJECTION|COMMAND_INJECTION', 'block')
ON CONFLICT (name) DO NOTHING;
