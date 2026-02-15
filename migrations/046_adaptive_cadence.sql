ALTER TABLE sources ADD COLUMN consecutive_misses INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sources DROP COLUMN cadence_hours;
