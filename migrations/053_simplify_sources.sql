-- Simplify sources: drop source_type column and child tables.
-- Everything is now derived from the URL at runtime.

-- Step 1: Backfill URLs from child tables for any sources missing them
UPDATE sources s
SET url = 'https://' || ss.platform || '.com/' || ss.handle
FROM social_sources ss
WHERE ss.source_id = s.id
  AND s.url IS NULL;

UPDATE sources s
SET url = 'https://' || ws.domain
FROM website_sources ws
WHERE ws.source_id = s.id
  AND s.url IS NULL;

-- Step 2: Drop source_type column
ALTER TABLE sources DROP COLUMN IF EXISTS source_type;

-- Step 3: Drop child tables
DROP TABLE IF EXISTS social_sources;
DROP TABLE IF EXISTS website_sources;

-- Step 4: Add unique constraint on normalized URL
-- NULL urls (web_search sources) are fine — Postgres UNIQUE allows multiple NULLs
CREATE UNIQUE INDEX IF NOT EXISTS idx_sources_url_unique ON sources (url) WHERE url IS NOT NULL;

-- Step 5: Drop the old source_type index
DROP INDEX IF EXISTS idx_sources_type;
