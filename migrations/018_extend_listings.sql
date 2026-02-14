-- Extend listings with computed/temporal fields that need indexing.
-- All classification dimensions live in the tag system (tags + taggables).

ALTER TABLE listings ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;
ALTER TABLE listings ADD COLUMN IF NOT EXISTS freshness_score REAL NOT NULL DEFAULT 1.0;
ALTER TABLE listings ADD COLUMN IF NOT EXISTS relevance_score INTEGER;
ALTER TABLE listings ADD COLUMN IF NOT EXISTS relevance_breakdown TEXT;

CREATE INDEX IF NOT EXISTS idx_listings_expires_at ON listings(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_listings_relevance ON listings(relevance_score DESC NULLS LAST);
