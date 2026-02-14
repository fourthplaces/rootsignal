-- Heat map points for geographic signal density visualization
CREATE TABLE heat_map_points (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    latitude DOUBLE PRECISION NOT NULL,
    longitude DOUBLE PRECISION NOT NULL,
    weight DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    entity_type TEXT NOT NULL,
    entity_id UUID NOT NULL,
    generated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_heat_map_points_generated ON heat_map_points(generated_at);
CREATE INDEX idx_heat_map_points_entity_type ON heat_map_points(entity_type);
CREATE INDEX idx_heat_map_points_coords ON heat_map_points(latitude, longitude);
