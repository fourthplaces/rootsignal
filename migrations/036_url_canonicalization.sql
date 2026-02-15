-- URL canonicalization: prevent duplicate crawls by normalizing URLs.

-- Add canonical_url to page_snapshots
ALTER TABLE page_snapshots ADD COLUMN canonical_url TEXT;

-- Backfill from url
UPDATE page_snapshots SET canonical_url = url WHERE canonical_url IS NULL;

-- Set NOT NULL after backfill
ALTER TABLE page_snapshots ALTER COLUMN canonical_url SET NOT NULL;

-- Replace unique constraint: deduplicate by canonical_url + content_hash
ALTER TABLE page_snapshots DROP CONSTRAINT page_snapshots_url_content_hash_key;
ALTER TABLE page_snapshots ADD CONSTRAINT page_snapshots_canonical_url_content_hash_key UNIQUE(canonical_url, content_hash);

CREATE INDEX idx_page_snapshots_canonical_url ON page_snapshots(canonical_url);

-- URL aliases: track redirects so we don't re-crawl known aliases
CREATE TABLE url_aliases (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    original_url TEXT NOT NULL UNIQUE,
    canonical_url TEXT NOT NULL,
    redirect_type TEXT,
    discovered_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_url_aliases_canonical ON url_aliases(canonical_url);
CREATE INDEX idx_url_aliases_discovered ON url_aliases(discovered_at);
