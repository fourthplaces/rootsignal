BEGIN;

-- Drop listing-related tables
DROP TABLE IF EXISTS listing_extractions CASCADE;
DROP TABLE IF EXISTS listings CASCADE;

-- Drop listing-related functions/triggers
DROP FUNCTION IF EXISTS listings_search_vector_update() CASCADE;

-- Drop listing_type tag kind and all its tag values
DELETE FROM taggables WHERE tag_id IN (SELECT id FROM tags WHERE kind = 'listing_type');
DELETE FROM tags WHERE kind = 'listing_type';
DELETE FROM tag_kinds WHERE slug = 'listing_type';

-- Remove 'listing' from CHECK constraints on clusters and cluster_items
ALTER TABLE clusters DROP CONSTRAINT IF EXISTS clusters_cluster_type_check;
ALTER TABLE clusters ADD CONSTRAINT clusters_cluster_type_check CHECK (cluster_type IN ('entity', 'signal'));

ALTER TABLE cluster_items DROP CONSTRAINT IF EXISTS cluster_items_item_type_check;
ALTER TABLE cluster_items ADD CONSTRAINT cluster_items_item_type_check CHECK (item_type IN ('entity', 'signal'));

ALTER TABLE cluster_items DROP CONSTRAINT IF EXISTS cluster_items_cluster_type_check;
ALTER TABLE cluster_items ADD CONSTRAINT cluster_items_cluster_type_check CHECK (cluster_type IN ('entity', 'signal'));

-- Drop listing_type column from heat_map_points
ALTER TABLE heat_map_points DROP COLUMN IF EXISTS listing_type;

-- Rename query_logs.clicked_listing_id -> clicked_signal_id
ALTER TABLE query_logs RENAME COLUMN clicked_listing_id TO clicked_signal_id;

COMMIT;
