-- Add correlation_id, aggregate_type, aggregate_id columns to events table.
-- Required by rootsignal-events store for seesaw integration.

ALTER TABLE events ADD COLUMN IF NOT EXISTS correlation_id UUID;
ALTER TABLE events ADD COLUMN IF NOT EXISTS aggregate_type TEXT;
ALTER TABLE events ADD COLUMN IF NOT EXISTS aggregate_id UUID;

CREATE INDEX IF NOT EXISTS idx_events_correlation_id ON events (correlation_id) WHERE correlation_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_events_aggregate ON events (aggregate_type, aggregate_id) WHERE aggregate_type IS NOT NULL;
