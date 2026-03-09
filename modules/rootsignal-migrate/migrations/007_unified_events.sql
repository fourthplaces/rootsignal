-- Unified event stream: append-only fact log.
-- The single source of truth. Everything else is derived.

CREATE TABLE events (
    seq           BIGSERIAL    PRIMARY KEY,
    ts            TIMESTAMPTZ  NOT NULL DEFAULT now(),
    event_type    TEXT         NOT NULL,
    -- Causal structure
    parent_seq    BIGINT       REFERENCES events(seq),
    caused_by_seq BIGINT       REFERENCES events(seq),
    -- Context
    run_id        TEXT,
    actor         TEXT,
    -- The fact itself
    payload       JSONB        NOT NULL,
    -- Forward compatibility
    schema_v      SMALLINT     NOT NULL DEFAULT 1
);

-- Composite index for type-filtered sequential reads (reducer skipping irrelevant events)
CREATE INDEX idx_events_type_seq ON events (event_type, seq);

-- Temporal queries (admin, debugging)
CREATE INDEX idx_events_ts ON events (ts);

-- Run-scoped queries (replacing scout_run_events FK)
CREATE INDEX idx_events_run ON events (run_id) WHERE run_id IS NOT NULL;

-- Causal tree traversal
CREATE INDEX idx_events_parent ON events (parent_seq) WHERE parent_seq IS NOT NULL;
CREATE INDEX idx_events_caused_by ON events (caused_by_seq) WHERE caused_by_seq IS NOT NULL;

-- Embedding cache: keyed by hash(model_version + input_text).
-- Used by the embedding enrichment pass to avoid redundant API calls.
CREATE TABLE embedding_cache (
    input_hash    TEXT         PRIMARY KEY,
    model_version TEXT         NOT NULL,
    embedding     FLOAT4[]     NOT NULL,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);
