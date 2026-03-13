-- Phase 3: Add columns for chain orchestration, scheduling, and failure tracking.
-- All new columns are nullable — populated from events via projection.

ALTER TABLE runs ADD COLUMN IF NOT EXISTS parent_run_id TEXT REFERENCES runs(run_id);
ALTER TABLE runs ADD COLUMN IF NOT EXISTS schedule_id TEXT;
ALTER TABLE runs ADD COLUMN IF NOT EXISTS run_at TIMESTAMPTZ;
ALTER TABLE runs ADD COLUMN IF NOT EXISTS error TEXT;
ALTER TABLE runs ADD COLUMN IF NOT EXISTS cancelled_at TIMESTAMPTZ;

-- Backfill run_at from started_at for existing rows
UPDATE runs SET run_at = COALESCE(run_at, started_at, now()) WHERE run_at IS NULL;

-- Indexes
CREATE INDEX IF NOT EXISTS idx_runs_parent_run_id ON runs(parent_run_id) WHERE parent_run_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_runs_schedule_id ON runs(schedule_id) WHERE schedule_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_runs_deferred ON runs(run_at) WHERE started_at IS NULL AND cancelled_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_runs_run_at_desc ON runs(run_at DESC);
