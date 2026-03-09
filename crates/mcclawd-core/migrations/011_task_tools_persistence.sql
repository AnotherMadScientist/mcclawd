-- Persist selected skills, resolved allowed tools, and tool profile per task.
-- This provides an audit trail and enables container restart/retry with the
-- same tool set even if the skill catalog has changed.

ALTER TABLE tasks ADD COLUMN IF NOT EXISTS selected_skills TEXT[] DEFAULT '{}';
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS allowed_tools TEXT[] DEFAULT '{}';
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS tool_profile TEXT DEFAULT NULL;
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS skill_context TEXT DEFAULT '';
