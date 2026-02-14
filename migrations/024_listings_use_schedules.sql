-- Migrate listing timing into schedules, then drop inline timing columns.

-- Backfill: create schedule records from existing timing data
INSERT INTO schedules (scheduleable_type, scheduleable_id, valid_from, valid_to)
SELECT 'listing', id, timing_start, timing_end
FROM listings
WHERE timing_start IS NOT NULL
ON CONFLICT (scheduleable_type, scheduleable_id, dtstart) DO NOTHING;

-- Drop the inline timing columns and index
DROP INDEX IF EXISTS idx_listings_timing;
ALTER TABLE listings DROP COLUMN IF EXISTS timing_start;
ALTER TABLE listings DROP COLUMN IF EXISTS timing_end;
