CREATE TABLE scout_runs (
    run_id      TEXT        PRIMARY KEY,
    region      TEXT        NOT NULL,
    started_at  TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    stats       JSONB       NOT NULL DEFAULT '{}',
    events      JSONB       NOT NULL DEFAULT '[]'
);

CREATE INDEX idx_scout_runs_region_finished
    ON scout_runs (region, finished_at DESC);
