CREATE INDEX IF NOT EXISTS idx_scout_run_events_node_id ON scout_run_events(node_id);
CREATE INDEX IF NOT EXISTS idx_scout_run_events_matched_id ON scout_run_events(matched_id);
CREATE INDEX IF NOT EXISTS idx_scout_run_events_existing_id ON scout_run_events(existing_id);
