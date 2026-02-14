-- Locations (HSDS-aligned)
CREATE TABLE locations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id UUID REFERENCES entities(id) ON DELETE SET NULL,
    name TEXT,
    address_line_1 TEXT,
    city TEXT,
    state TEXT,
    postal_code TEXT,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    location_type TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_locations_entity ON locations(entity_id);
CREATE INDEX idx_locations_city ON locations(city);
CREATE INDEX idx_locations_geo ON locations(latitude, longitude) WHERE latitude IS NOT NULL;

-- Polymorphic join for locations
CREATE TABLE locationables (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    location_id UUID NOT NULL REFERENCES locations(id) ON DELETE CASCADE,
    locatable_type TEXT NOT NULL,
    locatable_id UUID NOT NULL,
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE(location_id, locatable_type, locatable_id)
);

CREATE INDEX idx_locationables_target ON locationables(locatable_type, locatable_id);
