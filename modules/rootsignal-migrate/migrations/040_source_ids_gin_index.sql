-- GIN index for source_ids JSONB containment queries (is_source_busy).
-- jsonb_path_ops supports only @> but uses ~3x less space than default GIN.
CREATE INDEX IF NOT EXISTS idx_scout_runs_source_ids_gin
    ON scout_runs USING gin (source_ids jsonb_path_ops)
    WHERE source_ids IS NOT NULL;
