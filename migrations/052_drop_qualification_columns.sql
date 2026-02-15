-- Drop qualification columns from sources.
-- These are replaced by signal counts — data-driven, no editorial gate.
ALTER TABLE sources DROP COLUMN IF EXISTS qualification_status;
ALTER TABLE sources DROP COLUMN IF EXISTS qualification_summary;
ALTER TABLE sources DROP COLUMN IF EXISTS qualification_score;
