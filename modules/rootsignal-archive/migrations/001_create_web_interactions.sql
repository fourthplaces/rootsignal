-- Web interactions archive: records every web fetch the scout makes.

CREATE TABLE web_interactions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id          UUID NOT NULL,
    city_slug       TEXT NOT NULL,
    kind            TEXT NOT NULL,    -- 'page', 'feed', 'search', 'social', 'pdf', 'raw'
    target          TEXT NOT NULL,    -- normalized URL or query string (lookup key)
    target_raw      TEXT NOT NULL,    -- original target as passed to fetch()
    fetcher         TEXT NOT NULL,    -- 'chrome', 'browserless', 'serper', 'apify', 'reqwest'
    raw_html        TEXT,             -- page scrapes only
    markdown        TEXT,             -- page scrapes only (post-Readability)
    response_json   JSONB,            -- search/social/feed results
    raw_bytes       BYTEA,            -- PDFs and binary content
    content_hash    TEXT NOT NULL,    -- FNV-1a hash
    fetched_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    duration_ms     INTEGER NOT NULL,
    error           TEXT,             -- null on success, error message on failure
    metadata        JSONB             -- extensible: platform, limit, topics, etc.
) PARTITION BY RANGE (fetched_at);

-- Initial partition (monthly)
CREATE TABLE web_interactions_2026_02 PARTITION OF web_interactions
    FOR VALUES FROM ('2026-02-01') TO ('2026-03-01');

CREATE TABLE web_interactions_2026_03 PARTITION OF web_interactions
    FOR VALUES FROM ('2026-03-01') TO ('2026-04-01');

-- Lookup indexes
CREATE INDEX idx_web_interactions_target ON web_interactions (target, fetched_at DESC);
CREATE INDEX idx_web_interactions_run ON web_interactions (run_id);
CREATE INDEX idx_web_interactions_hash ON web_interactions (content_hash);
CREATE INDEX idx_web_interactions_city_time ON web_interactions (city_slug, fetched_at DESC);
