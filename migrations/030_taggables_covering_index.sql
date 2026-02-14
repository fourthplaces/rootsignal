-- Covering index on taggables for tag filter queries
DROP INDEX IF EXISTS idx_taggables_target;
CREATE INDEX idx_taggables_covering ON taggables(taggable_type, taggable_id, tag_id);
