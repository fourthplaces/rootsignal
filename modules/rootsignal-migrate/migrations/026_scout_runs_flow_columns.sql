-- Add region_id, flow_type, and source_ids to scout_runs for decoupled flows.
-- region_id replaces task_id for region-scoped flows.
-- flow_type identifies which flow produced this run (bootstrap, scrape, weave, scout_source).
-- source_ids stores targeted source IDs for scout_source flows.

ALTER TABLE scout_runs ADD COLUMN IF NOT EXISTS region_id TEXT;
ALTER TABLE scout_runs ADD COLUMN IF NOT EXISTS flow_type TEXT;
ALTER TABLE scout_runs ADD COLUMN IF NOT EXISTS source_ids JSONB;

CREATE INDEX IF NOT EXISTS idx_scout_runs_region_id
    ON scout_runs (region_id) WHERE region_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_scout_runs_flow_type
    ON scout_runs (flow_type) WHERE flow_type IS NOT NULL;
