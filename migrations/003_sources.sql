-- Layer 1: Sources (what we monitor)
CREATE TABLE sources (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id UUID REFERENCES entities(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    source_type TEXT NOT NULL,
    adapter TEXT NOT NULL,
    url TEXT,
    handle TEXT,
    cadence_hours INT NOT NULL DEFAULT 24,
    last_scraped_at TIMESTAMPTZ,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    config JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sources_entity ON sources(entity_id);
CREATE INDEX idx_sources_type ON sources(source_type);
CREATE INDEX idx_sources_active ON sources(is_active) WHERE is_active = TRUE;

-- Website-specific config
CREATE TABLE website_sources (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL UNIQUE REFERENCES sources(id) ON DELETE CASCADE,
    domain TEXT NOT NULL UNIQUE,
    max_crawl_depth INT NOT NULL DEFAULT 2,
    is_trusted BOOLEAN NOT NULL DEFAULT FALSE
);

-- Social-specific config
CREATE TABLE social_sources (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL UNIQUE REFERENCES sources(id) ON DELETE CASCADE,
    platform TEXT NOT NULL,
    handle TEXT NOT NULL,
    UNIQUE(platform, handle)
);
