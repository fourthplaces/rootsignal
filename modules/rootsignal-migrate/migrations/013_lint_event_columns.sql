-- Add columns for signal lint audit trail events.
-- These columns store per-field correction details and batch signal counts.

ALTER TABLE scout_run_events ADD COLUMN field TEXT;
ALTER TABLE scout_run_events ADD COLUMN old_value TEXT;
ALTER TABLE scout_run_events ADD COLUMN new_value TEXT;
ALTER TABLE scout_run_events ADD COLUMN signal_count INT;
