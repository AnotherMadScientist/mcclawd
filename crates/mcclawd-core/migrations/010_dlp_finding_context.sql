-- Add source context columns to dlp_findings so users can click a finding
-- and see the surrounding text with the match highlighted.

ALTER TABLE dlp_findings
    ADD COLUMN IF NOT EXISTS source_text TEXT,
    ADD COLUMN IF NOT EXISTS match_offset INTEGER,
    ADD COLUMN IF NOT EXISTS match_length INTEGER;
