ALTER TABLE heat_map_points ADD COLUMN IF NOT EXISTS signal_type TEXT;
CREATE INDEX IF NOT EXISTS idx_heat_map_points_signal_type ON heat_map_points(signal_type);
