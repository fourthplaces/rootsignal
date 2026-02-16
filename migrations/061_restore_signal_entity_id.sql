-- Restore entity_id on signals (accidentally dropped by migration 058).
ALTER TABLE signals ADD COLUMN entity_id UUID REFERENCES entities(id) ON DELETE SET NULL;
CREATE INDEX idx_signals_entity ON signals(entity_id);
