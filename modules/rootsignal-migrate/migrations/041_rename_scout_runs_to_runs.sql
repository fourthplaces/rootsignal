-- Phase 2a: Rename scout_runs → runs with backward-compat view for zero-downtime.
-- Old instances continue reading/writing via the view during rolling deploy.

ALTER TABLE scout_runs RENAME TO runs;

-- Rename indexes for clarity (they still function under old names, but confusing)
ALTER INDEX IF EXISTS idx_scout_runs_region_finished RENAME TO idx_runs_region_finished;
ALTER INDEX IF EXISTS idx_scout_runs_region_id RENAME TO idx_runs_region_id;
ALTER INDEX IF EXISTS idx_scout_runs_flow_type RENAME TO idx_runs_flow_type;
ALTER INDEX IF EXISTS idx_scout_runs_source_ids_gin RENAME TO idx_runs_source_ids_gin;

-- Backward-compat view so old instances don't 500 during rolling deploy.
-- This view is updatable (single table, no aggregates) so INSERTs work too.
CREATE OR REPLACE VIEW scout_runs AS SELECT * FROM runs;

-- Backfill flow_type for historical rows that predate the flow_type column.
UPDATE runs SET flow_type = 'scrape' WHERE flow_type IS NULL;
ALTER TABLE runs ALTER COLUMN flow_type SET NOT NULL;
