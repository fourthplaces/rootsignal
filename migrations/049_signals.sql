-- Signal types: ask, give, event, informative
-- Location via locationables (locatable_type = 'signal') — reuses existing geo infra
-- Temporal via schedules (scheduleable_type = 'signal') — reuses existing iCal infra
-- Embeddings via embeddings (embeddable_type = 'signal') — reuses existing vector infra
CREATE TABLE signals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    signal_type TEXT NOT NULL CHECK (signal_type IN ('ask', 'give', 'event', 'informative')),
    content TEXT NOT NULL,
    about TEXT,                          -- schema.org: subject matter (what's being asked/given/discussed)
    entity_id UUID REFERENCES entities(id) ON DELETE SET NULL,
    source_url TEXT,

    -- Provenance
    page_snapshot_id UUID REFERENCES page_snapshots(id) ON DELETE SET NULL,
    extraction_id UUID REFERENCES extractions(id) ON DELETE SET NULL,
    institutional_source TEXT,          -- 'usaspending', 'epa_echo', etc. (NULL for community)
    institutional_record_id TEXT,       -- external ID (award_id, frs_id, etc.)
    source_citation_url TEXT,           -- direct link to government source

    -- Quality
    confidence REAL NOT NULL DEFAULT 0.7,
    fingerprint BYTEA NOT NULL,
    schema_version INT NOT NULL DEFAULT 1,

    -- Language (schema.org: inLanguage, BCP 47)
    in_language TEXT NOT NULL DEFAULT 'en',

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE(fingerprint, schema_version)
);

CREATE INDEX idx_signals_type ON signals(signal_type);
CREATE INDEX idx_signals_entity ON signals(entity_id);
CREATE INDEX idx_signals_created ON signals(created_at DESC);
CREATE INDEX idx_signals_institutional ON signals(institutional_source, institutional_record_id)
    WHERE institutional_source IS NOT NULL;
CREATE INDEX idx_signals_snapshot ON signals(page_snapshot_id);

-- Full-text search on content + about
ALTER TABLE signals ADD COLUMN search_vector tsvector
    GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(content, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(about, '')), 'B')
    ) STORED;

CREATE INDEX idx_signals_search ON signals USING GIN(search_vector);
