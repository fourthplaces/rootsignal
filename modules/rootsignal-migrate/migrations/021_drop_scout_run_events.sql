-- scout_run_events is superseded by the unified `events` table (which has run_id + index).
-- The scout_runs table is kept — now populated by the seesaw scout_runs_handler.
DROP TABLE IF EXISTS scout_run_events;
