-- Composite index for flow-type-scoped busy checks.
-- Only indexes active (unfinished) runs so the index stays small as completed runs accumulate.
CREATE INDEX IF NOT EXISTS idx_runs_busy_check
    ON runs (region_id, flow_type)
    WHERE finished_at IS NULL;
