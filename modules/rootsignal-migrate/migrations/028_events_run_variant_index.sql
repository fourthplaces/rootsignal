CREATE INDEX idx_events_run_variant
ON events (run_id, (payload->>'type'))
WHERE run_id IS NOT NULL;
