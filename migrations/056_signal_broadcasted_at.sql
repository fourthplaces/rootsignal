ALTER TABLE signals ADD COLUMN broadcasted_at TIMESTAMPTZ;
CREATE INDEX idx_signals_broadcasted ON signals(broadcasted_at DESC) WHERE broadcasted_at IS NOT NULL;
