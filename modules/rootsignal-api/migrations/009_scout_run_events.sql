-- Scout run events: proper event storage with parent-child tree structure.
-- Replaces the JSONB `events` blob in scout_runs with a flat table.
-- No backfill of old data — clean break.

CREATE TABLE scout_run_events (
    id              UUID            PRIMARY KEY DEFAULT gen_random_uuid(),
    parent_id       UUID            REFERENCES scout_run_events(id),
    run_id          TEXT            NOT NULL REFERENCES scout_runs(run_id) ON DELETE CASCADE,
    seq             INT             NOT NULL,
    ts              TIMESTAMPTZ     NOT NULL DEFAULT now(),
    event_type      TEXT            NOT NULL,
    source_url      TEXT,
    -- type-specific columns (nullable)
    query           TEXT,
    url             TEXT,
    provider        TEXT,
    platform        TEXT,
    identifier      TEXT,
    signal_type     TEXT,
    title           TEXT,
    result_count    INT,
    post_count      INT,
    items           INT,
    content_bytes   BIGINT,
    content_chars   BIGINT,
    signals_extracted INT,
    implied_queries INT,
    similarity      DOUBLE PRECISION,
    confidence      DOUBLE PRECISION,
    success         BOOLEAN,
    action          TEXT,
    node_id         TEXT,
    matched_id      TEXT,
    existing_id     TEXT,
    new_source_url  TEXT,
    canonical_key   TEXT,
    gatherings      BIGINT,
    needs           BIGINT,
    stale           BIGINT,
    sources_created BIGINT,
    spent_cents     BIGINT,
    remaining_cents BIGINT,
    topics          TEXT[],
    posts_found     INT,
    reason          TEXT,
    strategy        TEXT,
    UNIQUE (run_id, seq)
);

CREATE INDEX idx_sre_run_id ON scout_run_events (run_id, seq);
CREATE INDEX idx_sre_parent ON scout_run_events (parent_id) WHERE parent_id IS NOT NULL;
CREATE INDEX idx_sre_run_type ON scout_run_events (run_id, event_type);

-- Drop the events JSONB blob from scout_runs (stats stays as JSONB)
ALTER TABLE scout_runs DROP COLUMN events;
