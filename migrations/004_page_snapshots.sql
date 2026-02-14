-- Layer 2: Raw data (immutable cache)
CREATE TABLE page_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    url TEXT NOT NULL,
    content_hash BYTEA NOT NULL,
    html TEXT,
    markdown TEXT,
    fetched_via TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    crawled_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    extraction_status TEXT NOT NULL DEFAULT 'pending',
    extraction_completed_at TIMESTAMPTZ,
    UNIQUE(url, content_hash)
);

CREATE INDEX idx_page_snapshots_url ON page_snapshots(url);
CREATE INDEX idx_page_snapshots_status ON page_snapshots(extraction_status);
CREATE INDEX idx_page_snapshots_crawled ON page_snapshots(crawled_at DESC);

-- Links sources to pages we've crawled
CREATE TABLE domain_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    page_url TEXT NOT NULL,
    page_snapshot_id UUID REFERENCES page_snapshots(id) ON DELETE SET NULL,
    last_scraped_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    scrape_status TEXT NOT NULL DEFAULT 'pending',
    scrape_error TEXT,
    UNIQUE(source_id, page_url)
);

CREATE INDEX idx_domain_snapshots_source ON domain_snapshots(source_id);
