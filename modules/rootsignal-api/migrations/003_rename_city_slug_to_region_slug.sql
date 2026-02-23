-- Rename city_slug → region_slug to remove city concept.
ALTER TABLE web_interactions RENAME COLUMN city_slug TO region_slug;

-- Recreate the index with the new column name.
DROP INDEX IF EXISTS idx_web_interactions_city_time;
CREATE INDEX idx_web_interactions_region_time ON web_interactions (region_slug, fetched_at DESC);
