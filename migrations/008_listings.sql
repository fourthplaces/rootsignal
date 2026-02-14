-- Layer 4: Listings (derived, searchable)
CREATE TABLE listings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'active',

    -- Links
    entity_id UUID REFERENCES entities(id) ON DELETE SET NULL,
    service_id UUID REFERENCES services(id) ON DELETE SET NULL,
    source_url TEXT,

    -- Location (denormalized for fast geo queries)
    location_text TEXT,
    latitude FLOAT,
    longitude FLOAT,

    -- Timing
    timing_start TIMESTAMPTZ,
    timing_end TIMESTAMPTZ,

    -- Embedding for semantic search/dedup
    embedding VECTOR(1536),

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_listings_status ON listings(status);
CREATE INDEX idx_listings_entity ON listings(entity_id);
CREATE INDEX idx_listings_service ON listings(service_id);
CREATE INDEX idx_listings_timing ON listings(timing_start, timing_end);
CREATE INDEX idx_listings_geo ON listings(latitude, longitude) WHERE latitude IS NOT NULL;

-- Provenance: how a listing was created
CREATE TABLE listing_extractions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    listing_id UUID NOT NULL REFERENCES listings(id) ON DELETE CASCADE,
    extraction_id UUID REFERENCES extractions(id) ON DELETE SET NULL,
    page_snapshot_id UUID REFERENCES page_snapshots(id) ON DELETE SET NULL,
    fingerprint TEXT,
    extraction_confidence TEXT,
    content_hash TEXT,
    source_id UUID REFERENCES sources(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_listing_extractions_listing ON listing_extractions(listing_id);
CREATE INDEX idx_listing_extractions_fingerprint ON listing_extractions(fingerprint);
