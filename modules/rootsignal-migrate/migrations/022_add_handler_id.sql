ALTER TABLE events ADD COLUMN handler_id TEXT;

CREATE INDEX idx_events_handler_id ON events (handler_id) WHERE handler_id IS NOT NULL;
