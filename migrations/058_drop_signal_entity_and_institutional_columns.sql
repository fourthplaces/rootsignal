-- Remove institutional_source and institutional_record_id from signals.
-- Institutional sources should be modeled as Source records.
-- entity_id is kept as a denormalized cache of source.entity_id for clustering performance.

ALTER TABLE signals DROP COLUMN IF EXISTS institutional_source;
ALTER TABLE signals DROP COLUMN IF EXISTS institutional_record_id;

DROP INDEX IF EXISTS idx_signals_institutional;
