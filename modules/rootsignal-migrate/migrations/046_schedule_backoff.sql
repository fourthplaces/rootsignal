-- Rename cadence_seconds → timeout for clarity.
-- Add base_timeout (reset value for exponential backoff) and recurring flag.

ALTER TABLE schedules RENAME COLUMN cadence_seconds TO timeout;

ALTER TABLE schedules ADD COLUMN base_timeout INTEGER;
UPDATE schedules SET base_timeout = timeout;
ALTER TABLE schedules ALTER COLUMN base_timeout SET NOT NULL;

ALTER TABLE schedules ADD COLUMN recurring BOOLEAN NOT NULL DEFAULT true;
