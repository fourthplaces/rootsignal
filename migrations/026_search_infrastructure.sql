-- Search infrastructure: FTS tsvector, temporal indexes, heatmap enrichment

-- Full-text search vector on listings (DB-only, NOT added to Listing Rust struct)
ALTER TABLE listings ADD COLUMN IF NOT EXISTS search_vector tsvector;

UPDATE listings SET search_vector =
    setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
    setweight(to_tsvector('english', coalesce(description, '')), 'B');

CREATE INDEX IF NOT EXISTS idx_listings_search_vector
    ON listings USING GIN (search_vector);

-- Auto-update trigger (no app code needed for maintenance)
CREATE OR REPLACE FUNCTION listings_search_vector_update() RETURNS trigger AS $$
BEGIN
    NEW.search_vector :=
        setweight(to_tsvector('english', coalesce(NEW.title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.description, '')), 'B');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_listings_search_vector ON listings;
CREATE TRIGGER trg_listings_search_vector
    BEFORE INSERT OR UPDATE OF title, description ON listings
    FOR EACH ROW EXECUTE FUNCTION listings_search_vector_update();

-- Schedule indexes for temporal queries
CREATE INDEX IF NOT EXISTS idx_schedules_temporal
    ON schedules(valid_from, valid_to)
    WHERE scheduleable_type = 'listing';

-- Heatmap enrichment columns
ALTER TABLE heat_map_points ADD COLUMN IF NOT EXISTS signal_domain TEXT;
ALTER TABLE heat_map_points ADD COLUMN IF NOT EXISTS category TEXT;
ALTER TABLE heat_map_points ADD COLUMN IF NOT EXISTS listing_type TEXT;

CREATE INDEX IF NOT EXISTS idx_heat_map_points_signal_domain
    ON heat_map_points(signal_domain) WHERE signal_domain IS NOT NULL;
