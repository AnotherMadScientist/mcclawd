-- Workspace profiles: store custom user profiles (built-in profiles are in Rust code)
CREATE TABLE IF NOT EXISTS workspace_profiles (
    id SERIAL PRIMARY KEY,
    user_id TEXT NOT NULL DEFAULT 'admin',
    profile_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    filename TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, profile_name, filename)
);
